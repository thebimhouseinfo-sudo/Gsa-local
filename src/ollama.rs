use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thinking: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::plain("system", content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::plain("user", content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::plain("assistant", content)
    }

    pub fn tool(tool_name: impl Into<String>, content: impl Into<String>) -> Self {
        let mut message = Self::plain("tool", content);
        message.tool_name = Some(tool_name.into());
        message
    }

    fn plain(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            thinking: String::new(),
            tool_calls: Vec::new(),
            tool_name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub function: ToolFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolFunctionCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunctionDefinition,
}

impl ToolDefinition {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
    ) -> Self {
        Self {
            kind: "function".into(),
            function: ToolFunctionDefinition {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolFunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone)]
pub struct OllamaClient {
    base_url: String,
    num_ctx: usize,
    http: Client,
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ChatChunk {
    message: Option<ChatMessage>,
    #[serde(default)]
    done: bool,
}

impl OllamaClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_num_ctx(base_url, 32 * 1024)
    }

    pub fn with_num_ctx(base_url: impl Into<String>, num_ctx: usize) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            num_ctx,
            http: Client::new(),
        }
    }

    pub async fn list_models(&self) -> Result<Vec<String>> {
        let response = self
            .http
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
            .context("failed to connect to Ollama /api/tags")?
            .error_for_status()
            .context("Ollama /api/tags returned an error")?
            .json::<TagsResponse>()
            .await
            .context("failed to parse Ollama model list")?;
        Ok(response.models.into_iter().map(|m| m.name).collect())
    }

    pub async fn show_model(&self, model: &str) -> Result<Value> {
        self.http
            .post(format!("{}/api/show", self.base_url))
            .json(&json!({ "model": model }))
            .send()
            .await
            .context("failed to connect to Ollama /api/show")?
            .error_for_status()
            .context("Ollama /api/show returned an error")?
            .json::<Value>()
            .await
            .context("failed to parse Ollama model metadata")
    }

    pub async fn chat_stream<F>(
        &self,
        model: &str,
        messages: &[ChatMessage],
        on_token: F,
    ) -> Result<String>
    where
        F: FnMut(&str),
    {
        Ok(self
            .chat_stream_with_tools(model, messages, &[], on_token)
            .await?
            .content)
    }

    pub async fn chat_stream_with_tools<F>(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        mut on_token: F,
    ) -> Result<ChatMessage>
    where
        F: FnMut(&str),
    {
        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
            "options": {
                "num_ctx": self.num_ctx
            }
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)?;
        }

        let response = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .context("failed to connect to Ollama /api/chat")?;

        let status = response.status();
        if !status.is_success() {
            let detail = response
                .text()
                .await
                .unwrap_or_else(|error| format!("<failed to read Ollama error body: {error}>"));
            bail!("Ollama /api/chat returned {status}: {detail}");
        }

        let mut stream = response.bytes_stream();
        let mut pending = Vec::new();
        let mut assistant = ChatMessage::assistant("");

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed while reading Ollama stream")?;
            pending.extend_from_slice(&chunk);
            consume_complete_lines(&mut pending, &mut assistant, &mut on_token)?;
        }

        if !pending.is_empty() {
            consume_chat_bytes(&pending, &mut assistant, &mut on_token)?;
        }

        Ok(assistant)
    }
}

fn consume_complete_lines<F>(
    pending: &mut Vec<u8>,
    assistant: &mut ChatMessage,
    on_token: &mut F,
) -> Result<()>
where
    F: FnMut(&str),
{
    while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
        let mut line: Vec<u8> = pending.drain(..=newline).collect();
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if !line.iter().all(|byte| byte.is_ascii_whitespace()) {
            consume_chat_bytes(&line, assistant, on_token)?;
        }
    }
    Ok(())
}

fn consume_chat_bytes<F>(line: &[u8], assistant: &mut ChatMessage, on_token: &mut F) -> Result<()>
where
    F: FnMut(&str),
{
    let line = std::str::from_utf8(line).context("Ollama stream line was not valid UTF-8")?;
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    let chunk: ChatChunk = serde_json::from_str(line)
        .with_context(|| format!("invalid Ollama stream line: {line}"))?;
    if let Some(message) = chunk.message {
        if !message.content.is_empty() {
            on_token(&message.content);
            assistant.content.push_str(&message.content);
        }
        if !message.thinking.is_empty() {
            assistant.thinking.push_str(&message.thinking);
        }
        assistant.tool_calls.extend(message.tool_calls);
        if message.tool_name.is_some() {
            assistant.tool_name = message.tool_name;
        }
    }
    let _ = chunk.done;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_split_across_network_chunks_is_reassembled_before_decode() {
        let line = serde_json::to_string(&json!({
            "message": {
                "role": "assistant",
                "content": "Việt"
            },
            "done": false
        }))
        .unwrap()
            + "\n";

        let marker = line.find('ệ').unwrap();
        let split_inside_multibyte = marker + 1;
        let bytes = line.as_bytes();

        let mut pending = Vec::new();
        let mut assistant = ChatMessage::assistant("");
        let mut emitted = String::new();

        pending.extend_from_slice(&bytes[..split_inside_multibyte]);
        consume_complete_lines(&mut pending, &mut assistant, &mut |token| {
            emitted.push_str(token)
        })
        .unwrap();
        assert!(assistant.content.is_empty());

        pending.extend_from_slice(&bytes[split_inside_multibyte..]);
        consume_complete_lines(&mut pending, &mut assistant, &mut |token| {
            emitted.push_str(token)
        })
        .unwrap();

        assert_eq!(assistant.content, "Việt");
        assert_eq!(emitted, "Việt");
        assert!(pending.is_empty());
    }

    #[test]
    fn tool_call_survives_arbitrary_network_chunking() {
        let line = serde_json::to_vec(&json!({
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "type": "function",
                    "function": {
                        "index": 0,
                        "name": "submit_plan",
                        "arguments": {"goal": "Build workflow"}
                    }
                }]
            },
            "done": false
        }))
        .unwrap();
        let mut framed = line;
        framed.push(b'\n');

        let split = framed.len() / 2;
        let mut pending = Vec::new();
        let mut assistant = ChatMessage::assistant("");

        pending.extend_from_slice(&framed[..split]);
        consume_complete_lines(&mut pending, &mut assistant, &mut |_| {}).unwrap();
        assert!(assistant.tool_calls.is_empty());

        pending.extend_from_slice(&framed[split..]);
        consume_complete_lines(&mut pending, &mut assistant, &mut |_| {}).unwrap();

        assert_eq!(assistant.tool_calls.len(), 1);
        assert_eq!(assistant.tool_calls[0].function.name, "submit_plan");
        assert_eq!(
            assistant.tool_calls[0].function.arguments["goal"],
            "Build workflow"
        );
    }

    #[test]
    fn tool_result_serializes_with_tool_name() {
        let message = ChatMessage::tool("submit_plan", "ok");
        let value = serde_json::to_value(message).unwrap();
        assert_eq!(value["role"], "tool");
        assert_eq!(value["tool_name"], "submit_plan");
    }
}
