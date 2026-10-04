use crate::{
    config::AppConfig,
    harness::AgentId,
    ollama::{
        ChatMessage, OllamaChatResponse, OllamaChatTelemetry, OllamaClient, ToolCall,
        ToolDefinition,
    },
    session::Session,
    tools::ProjectToolRuntime,
};
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Instant;

pub const MAX_TOOL_ROUNDS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSelectionSource {
    SessionOverride,
    AgentConfig,
    InternalFixCoderSession,
    InternalFixCoderConfig,
    DefaultConfig,
    SingleInstalledBootstrap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModel {
    pub name: String,
    pub source: ModelSelectionSource,
}

fn non_empty_model(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub fn resolve_configured_model(
    session: &Session,
    config: &AppConfig,
    agent: AgentId,
) -> Option<ResolvedModel> {
    if let Some(model) = non_empty_model(session.model_override(agent)) {
        return Some(ResolvedModel {
            name: model.to_owned(),
            source: ModelSelectionSource::SessionOverride,
        });
    }

    if let Some(model) = non_empty_model(config.agent_model(agent)) {
        return Some(ResolvedModel {
            name: model.to_owned(),
            source: ModelSelectionSource::AgentConfig,
        });
    }

    if agent == AgentId::InternalFix {
        if let Some(model) = non_empty_model(session.model_override(AgentId::Coder)) {
            return Some(ResolvedModel {
                name: model.to_owned(),
                source: ModelSelectionSource::InternalFixCoderSession,
            });
        }
        if let Some(model) = non_empty_model(config.agent_model(AgentId::Coder)) {
            return Some(ResolvedModel {
                name: model.to_owned(),
                source: ModelSelectionSource::InternalFixCoderConfig,
            });
        }
    }

    non_empty_model(config.default_model.as_deref()).map(|model| ResolvedModel {
        name: model.to_owned(),
        source: ModelSelectionSource::DefaultConfig,
    })
}

pub fn resolve_model_from_installed(
    session: &Session,
    config: &AppConfig,
    agent: AgentId,
    installed_models: &[String],
) -> Result<ResolvedModel> {
    if let Some(resolved) = resolve_configured_model(session, config, agent) {
        return Ok(resolved);
    }

    match installed_models {
        [] => bail!(
            "MODEL_SELECTION_REQUIRED: no Ollama model is configured and no installed model is available"
        ),
        [only] => Ok(ResolvedModel {
            name: only.clone(),
            source: ModelSelectionSource::SingleInstalledBootstrap,
        }),
        _ => bail!(
            "MODEL_SELECTION_REQUIRED: multiple Ollama models are installed but no explicit agent/default model is configured"
        ),
    }
}

pub async fn resolve_model(
    ollama: &OllamaClient,
    session: &Session,
    config: &AppConfig,
    agent: AgentId,
) -> Result<ResolvedModel> {
    if let Some(resolved) = resolve_configured_model(session, config, agent) {
        return Ok(resolved);
    }
    let installed = ollama.list_models().await?;
    resolve_model_from_installed(session, config, agent, &installed)
}

pub async fn resolve_model_name(
    ollama: &OllamaClient,
    session: &Session,
    config: &AppConfig,
    agent: AgentId,
) -> Result<String> {
    Ok(resolve_model(ollama, session, config, agent).await?.name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimePolicy {
    pub max_action_rounds: usize,
    pub max_terminal_repairs: usize,
}

impl Default for RuntimePolicy {
    fn default() -> Self {
        Self {
            max_action_rounds: MAX_TOOL_ROUNDS,
            max_terminal_repairs: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructuredRunResult {
    pub terminal_arguments: Value,
    pub action_rounds: usize,
    pub terminal_repairs: usize,
    pub elapsed_ms: u128,
    pub invocations: Vec<OllamaChatTelemetry>,
}

pub struct StructuredAgentRuntime<'a> {
    ollama: &'a OllamaClient,
    model: &'a str,
    policy: RuntimePolicy,
}

impl<'a> StructuredAgentRuntime<'a> {
    pub fn new(ollama: &'a OllamaClient, model: &'a str, policy: RuntimePolicy) -> Self {
        Self {
            ollama,
            model,
            policy,
        }
    }

    pub async fn run<E, V, F>(
        &self,
        messages: &mut Vec<ChatMessage>,
        action_tools: &[ToolDefinition],
        terminal_tool: &ToolDefinition,
        mut execute_action: E,
        mut validate_terminal: V,
        mut on_token: F,
    ) -> Result<StructuredRunResult>
    where
        E: FnMut(&str, &Value) -> Result<Value>,
        V: FnMut(&Value) -> Result<()>,
        F: FnMut(&str),
    {
        validate_message_order(messages)?;
        let terminal_name = terminal_tool.function.name.as_str();
        if action_tools
            .iter()
            .any(|tool| tool.function.name == terminal_name)
        {
            bail!("terminal tool {terminal_name} must not also be an action tool");
        }

        let mut definitions = action_tools.to_vec();
        definitions.push(terminal_tool.clone());

        let started = Instant::now();
        let mut action_rounds = 0usize;
        let mut terminal_repairs = 0usize;
        let mut invocations = Vec::new();

        loop {
            let response = self
                .ollama
                .chat_stream_with_tools_response(
                    self.model,
                    messages,
                    &definitions,
                    &mut on_token,
                )
                .await?;
            invocations.push(response.telemetry.clone());

            if let Some(arguments) = process_structured_response(
                messages,
                response,
                terminal_name,
                self.policy,
                &mut action_rounds,
                &mut terminal_repairs,
                &mut execute_action,
                &mut validate_terminal,
            )? {
                return Ok(StructuredRunResult {
                    terminal_arguments: arguments,
                    action_rounds,
                    terminal_repairs,
                    elapsed_ms: started.elapsed().as_millis(),
                    invocations,
                });
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseDisposition {
    ProseOnly,
    ActionOnly,
    TerminalOnly,
    InvalidMixed,
}

fn classify_response(calls: &[ToolCall], terminal_name: &str) -> ResponseDisposition {
    if calls.is_empty() {
        return ResponseDisposition::ProseOnly;
    }
    let terminal_count = calls
        .iter()
        .filter(|call| call.function.name == terminal_name)
        .count();

    match (terminal_count, calls.len()) {
        (0, _) => ResponseDisposition::ActionOnly,
        (1, 1) => ResponseDisposition::TerminalOnly,
        _ => ResponseDisposition::InvalidMixed,
    }
}

fn validate_message_order(messages: &[ChatMessage]) -> Result<()> {
    if messages.first().map(|message| message.role.as_str()) != Some("system") {
        bail!("StructuredAgentRuntime requires one initial system message");
    }
    if messages
        .iter()
        .skip(1)
        .any(|message| message.role == "system")
    {
        bail!("system message must appear only at the beginning of StructuredAgentRuntime history");
    }
    Ok(())
}

fn consume_terminal_repair_budget(
    repairs: &mut usize,
    policy: RuntimePolicy,
    reason: &str,
) -> Result<()> {
    if *repairs >= policy.max_terminal_repairs {
        bail!(
            "structured terminal repair budget exhausted after {} repair(s): {}",
            policy.max_terminal_repairs,
            reason
        );
    }
    *repairs += 1;
    Ok(())
}

fn structured_tool_error(tool_name: &str, error: impl Into<String>) -> ChatMessage {
    ChatMessage::tool(
        tool_name,
        json!({
            "ok": false,
            "error": error.into()
        })
        .to_string(),
    )
}

fn process_structured_response<E, V>(
    messages: &mut Vec<ChatMessage>,
    response: OllamaChatResponse,
    terminal_name: &str,
    policy: RuntimePolicy,
    action_rounds: &mut usize,
    terminal_repairs: &mut usize,
    execute_action: &mut E,
    validate_terminal: &mut V,
) -> Result<Option<Value>>
where
    E: FnMut(&str, &Value) -> Result<Value>,
    V: FnMut(&Value) -> Result<()>,
{
    let calls = response.message.tool_calls.clone();
    let disposition = classify_response(&calls, terminal_name);
    messages.push(response.message);

    match disposition {
        ResponseDisposition::ProseOnly => {
            consume_terminal_repair_budget(
                terminal_repairs,
                policy,
                "model returned prose without the required terminal tool",
            )?;
            messages.push(ChatMessage::user(format!(
                "A structured terminal submission is required. Call {terminal_name} as the only terminal tool call. Do not answer with prose."
            )));
            Ok(None)
        }
        ResponseDisposition::ActionOnly => {
            if *action_rounds >= policy.max_action_rounds {
                bail!(
                    "structured action-tool budget exhausted after {} round(s)",
                    policy.max_action_rounds
                );
            }
            *action_rounds += 1;
            for call in calls {
                let name = call.function.name;
                let payload = match execute_action(&name, &call.function.arguments) {
                    Ok(result) => json!({"ok":true,"result":result}),
                    Err(error) => json!({"ok":false,"error":format!("{error:#}")}),
                };
                messages.push(ChatMessage::tool(name, payload.to_string()));
            }
            Ok(None)
        }
        ResponseDisposition::TerminalOnly => {
            let call = &calls[0];
            match validate_terminal(&call.function.arguments) {
                Ok(()) => Ok(Some(call.function.arguments.clone())),
                Err(error) => {
                    consume_terminal_repair_budget(
                        terminal_repairs,
                        policy,
                        "terminal arguments failed validation",
                    )?;
                    messages.push(structured_tool_error(
                        terminal_name,
                        format!(
                            "{error:#}. Correct only the structured terminal payload and resubmit {terminal_name}."
                        ),
                    ));
                    Ok(None)
                }
            }
        }
        ResponseDisposition::InvalidMixed => {
            consume_terminal_repair_budget(
                terminal_repairs,
                policy,
                "terminal submission was mixed with other tool calls or repeated",
            )?;
            let error = format!(
                "{terminal_name} must be the only terminal tool call in its response; no side effects were executed"
            );
            for call in calls {
                messages.push(structured_tool_error(&call.function.name, error.clone()));
            }
            Ok(None)
        }
    }
}

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

        ensure_tool_round_available(round, !calls.is_empty())?;

        let results = dispatch_tool_calls(agent, tools, &calls);
        messages.extend(results);
    }

    unreachable!("bounded tool loop must return or fail")
}

fn ensure_tool_round_available(round: usize, has_calls: bool) -> Result<()> {
    if has_calls && round + 1 >= MAX_TOOL_ROUNDS {
        bail!("agent exceeded maximum project-tool rounds ({MAX_TOOL_ROUNDS})");
    }
    Ok(())
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

    fn call(name: &str, arguments: Value) -> ToolCall {
        ToolCall {
            kind: Some("function".into()),
            function: ToolFunctionCall {
                index: None,
                name: name.into(),
                arguments,
            },
        }
    }

    fn response_with_calls(calls: Vec<ToolCall>) -> OllamaChatResponse {
        let mut message = ChatMessage::assistant("");
        message.tool_calls = calls;
        OllamaChatResponse {
            message,
            telemetry: OllamaChatTelemetry::default(),
        }
    }

    #[test]
    fn model_resolution_is_deterministic_and_internal_fix_inherits_coder() {
        let mut config = AppConfig::default();
        config.set_default_model(Some("default".into()));
        config.set_agent_model(AgentId::Coder, "coder-config".into());
        let mut session = Session::default();
        session.set_model_override(AgentId::Coder, "coder-session".into());

        let internal =
            resolve_model_from_installed(&session, &config, AgentId::InternalFix, &[]).unwrap();
        assert_eq!(internal.name, "coder-session");
        assert_eq!(
            internal.source,
            ModelSelectionSource::InternalFixCoderSession
        );

        session.set_model_override(AgentId::Reviewer, "reviewer-session".into());
        let reviewer =
            resolve_model_from_installed(&session, &config, AgentId::Reviewer, &[]).unwrap();
        assert_eq!(reviewer.name, "reviewer-session");
        assert_eq!(reviewer.source, ModelSelectionSource::SessionOverride);
    }

    #[test]
    fn model_resolution_bootstraps_only_one_installed_model() {
        let config = AppConfig::default();
        let session = Session::default();

        let one = resolve_model_from_installed(
            &session,
            &config,
            AgentId::Planner,
            &["only".into()],
        )
        .unwrap();
        assert_eq!(one.name, "only");
        assert_eq!(
            one.source,
            ModelSelectionSource::SingleInstalledBootstrap
        );

        let many = resolve_model_from_installed(
            &session,
            &config,
            AgentId::Planner,
            &["one".into(), "two".into()],
        )
        .unwrap_err();
        assert!(many.to_string().contains("MODEL_SELECTION_REQUIRED"));
    }

    #[test]
    fn structured_runtime_rejects_late_system_messages() {
        let messages = vec![
            ChatMessage::system("root"),
            ChatMessage::user("question"),
            ChatMessage::system("late"),
        ];
        assert!(validate_message_order(&messages).is_err());
    }

    #[test]
    fn action_only_response_executes_each_action_once() {
        let mut messages = vec![ChatMessage::system("root")];
        let response = response_with_calls(vec![
            call("project_read", json!({"path":"a"})),
            call("project_search", json!({"query":"x"})),
        ]);
        let mut action_rounds = 0;
        let mut repairs = 0;
        let mut executed = Vec::new();
        let mut executor = |name: &str, _args: &Value| {
            executed.push(name.to_owned());
            Ok(json!({"done":name}))
        };
        let mut validator = |_args: &Value| Ok(());

        let result = process_structured_response(
            &mut messages,
            response,
            "submit_plan",
            RuntimePolicy::default(),
            &mut action_rounds,
            &mut repairs,
            &mut executor,
            &mut validator,
        )
        .unwrap();

        assert!(result.is_none());
        assert_eq!(action_rounds, 1);
        assert_eq!(repairs, 0);
        assert_eq!(executed, vec!["project_read", "project_search"]);
    }

    #[test]
    fn mixed_terminal_and_action_executes_no_side_effects() {
        let mut messages = vec![ChatMessage::system("root")];
        let response = response_with_calls(vec![
            call("project_write", json!({"path":"x"})),
            call("submit_plan", json!({"goal":"x"})),
        ]);
        let mut action_rounds = 0;
        let mut repairs = 0;
        let mut executed = 0;
        let mut executor = |_name: &str, _args: &Value| {
            executed += 1;
            Ok(Value::Null)
        };
        let mut validator = |_args: &Value| Ok(());

        let result = process_structured_response(
            &mut messages,
            response,
            "submit_plan",
            RuntimePolicy::default(),
            &mut action_rounds,
            &mut repairs,
            &mut executor,
            &mut validator,
        )
        .unwrap();

        assert!(result.is_none());
        assert_eq!(executed, 0);
        assert_eq!(action_rounds, 0);
        assert_eq!(repairs, 1);
        assert_eq!(
            messages.iter().rev().take(2).filter(|m| m.role == "tool").count(),
            2
        );
    }

    #[test]
    fn invalid_terminal_payload_uses_tool_feedback_then_valid_payload_completes() {
        let mut messages = vec![ChatMessage::system("root")];
        let mut action_rounds = 0;
        let mut repairs = 0;
        let mut executor = |_name: &str, _args: &Value| Ok(Value::Null);
        let mut validator = |args: &Value| {
            if args.get("required").is_some() {
                Ok(())
            } else {
                bail!("missing required")
            }
        };

        let invalid = process_structured_response(
            &mut messages,
            response_with_calls(vec![call("submit_graph", json!({"status":"READY"}))]),
            "submit_graph",
            RuntimePolicy::default(),
            &mut action_rounds,
            &mut repairs,
            &mut executor,
            &mut validator,
        )
        .unwrap();
        assert!(invalid.is_none());
        assert_eq!(repairs, 1);
        assert_eq!(messages.last().unwrap().role, "tool");

        let valid_args = json!({"status":"READY","required":[]});
        let valid = process_structured_response(
            &mut messages,
            response_with_calls(vec![call("submit_graph", valid_args.clone())]),
            "submit_graph",
            RuntimePolicy::default(),
            &mut action_rounds,
            &mut repairs,
            &mut executor,
            &mut validator,
        )
        .unwrap();
        assert_eq!(valid, Some(valid_args));
        assert_eq!(repairs, 1);
    }

    #[test]
    fn prose_only_terminal_failure_uses_user_correction_and_is_bounded() {
        let mut messages = vec![ChatMessage::system("root")];
        let response = OllamaChatResponse {
            message: ChatMessage::assistant("not structured"),
            telemetry: OllamaChatTelemetry::default(),
        };
        let mut action_rounds = 0;
        let mut repairs = 0;
        let mut executor = |_name: &str, _args: &Value| Ok(Value::Null);
        let mut validator = |_args: &Value| Ok(());

        let result = process_structured_response(
            &mut messages,
            response,
            "submit_plan",
            RuntimePolicy {
                max_action_rounds: 1,
                max_terminal_repairs: 1,
            },
            &mut action_rounds,
            &mut repairs,
            &mut executor,
            &mut validator,
        )
        .unwrap();
        assert!(result.is_none());
        assert_eq!(repairs, 1);
        assert_eq!(messages.last().unwrap().role, "user");

        let next = OllamaChatResponse {
            message: ChatMessage::assistant("still prose"),
            telemetry: OllamaChatTelemetry::default(),
        };
        assert!(process_structured_response(
            &mut messages,
            next,
            "submit_plan",
            RuntimePolicy {
                max_action_rounds: 1,
                max_terminal_repairs: 1,
            },
            &mut action_rounds,
            &mut repairs,
            &mut executor,
            &mut validator,
        )
        .is_err());
    }

    #[test]
    fn tool_round_limit_fails_closed() {
        assert!(ensure_tool_round_available(MAX_TOOL_ROUNDS - 2, true).is_ok());
        assert!(ensure_tool_round_available(MAX_TOOL_ROUNDS - 1, true).is_err());
        assert!(ensure_tool_round_available(MAX_TOOL_ROUNDS - 1, false).is_ok());
    }

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
        assert!(results
            .iter()
            .all(|message| message.content.contains("\"ok\":true")));
    }
}
