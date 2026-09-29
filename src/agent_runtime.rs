use crate::{
    harness::AgentId,
    ollama::{ChatMessage, OllamaClient, ToolCall},
    tools::ProjectToolRuntime,
};
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::json;

pub const MAX_TOOL_ROUNDS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunResult {
    pub content: String,
    pub change_set_id: String,
}

pub async fn run_with_project_tools<F>(
    ollama: &OllamaClient,
    model: &str,
    agent: AgentId,
    messages: &mut Vec<ChatMessage>,
    tools: &mut ProjectToolRuntime,
    mut on_token: F,
) -> Result<AgentRunResult>
where
    F: FnMut(&str),
{
    let definitions = tools.tool_definitions(agent);

    for round in 0..MAX_TOOL_ROUNDS {
        let response = ollama
            .chat_stream_with_tools(model, messages, &definitions, &mut on_token)
            .await?;
        let calls = response.tool_calls.clone();
        let content = response.content.clone();
        messages.push(response);

        if calls.is_empty() {
            return Ok(AgentRunResult {
                content,
                change_set_id: tools.change_set_id()?,
            });
        }

        if round + 1 >= MAX_TOOL_ROUNDS {
            bail!("agent exceeded maximum project-tool rounds ({MAX_TOOL_ROUNDS})");
        }

        let results = dispatch_tool_calls(agent, tools, &calls);
        messages.extend(results);
    }

    unreachable!("bounded tool loop must return or fail")
}

pub fn dispatch_tool_calls(
    agent: AgentId,
    tools: &mut ProjectToolRuntime,
    calls: &[ToolCall],
) -> Vec<ChatMessage> {
    calls
        .iter()
        .map(|call| {
            let name = call.function.name.clone();
            let payload = match tools.execute(agent, &name, &call.function.arguments) {
                Ok(result) => ToolResult::ok(result),
                Err(error) => ToolResult::error(format!("{error:#}")),
            };
            ChatMessage::tool(
                name,
                serde_json::to_string(&payload).unwrap_or_else(|error| {
                    json!({"ok":false,"error":format!("failed to encode tool result: {error}")})
                        .to_string()
                }),
            )
        })
        .collect()
}

#[derive(Debug, Serialize)]
struct ToolResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl ToolResult {
    fn ok(result: serde_json::Value) -> Self {
        Self {
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    fn error(error: String) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::{ToolCall, ToolFunctionCall};
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn dispatch_returns_structured_error_for_disallowed_write() {
        let dir = tempdir().unwrap();
        let mut tools = ProjectToolRuntime::new(dir.path()).unwrap();
        let calls = vec![ToolCall {
            kind: Some("function".into()),
            function: ToolFunctionCall {
                index: Some(0),
                name: "project_write".into(),
                arguments: json!({
                    "path":"x.txt",
                    "content":"no",
                    "create_only":true
                }),
            },
        }];

        let results = dispatch_tool_calls(AgentId::Reviewer, &mut tools, &calls);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].role, "tool");
        assert!(results[0].content.contains("\"ok\":false"));
        assert!(!dir.path().join("x.txt").exists());
    }

    #[test]
    fn dispatch_executes_all_calls_in_order() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        let mut tools = ProjectToolRuntime::new(dir.path()).unwrap();
        let calls = vec![
            ToolCall {
                kind: Some("function".into()),
                function: ToolFunctionCall {
                    index: Some(0),
                    name: "project_read".into(),
                    arguments: json!({"path":"a.txt"}),
                },
            },
            ToolCall {
                kind: Some("function".into()),
                function: ToolFunctionCall {
                    index: Some(1),
                    name: "project_search".into(),
                    arguments: json!({"query":"hello"}),
                },
            },
        ];

        let results = dispatch_tool_calls(AgentId::Coder, &mut tools, &calls);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tool_name.as_deref(), Some("project_read"));
        assert_eq!(results[1].tool_name.as_deref(), Some("project_search"));
        assert!(results.iter().all(|message| message.content.contains("\"ok\":true")));
    }
}
