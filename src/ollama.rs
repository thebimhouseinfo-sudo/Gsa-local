use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OllamaClient {
    base_url: String,
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
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
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
        mut on_token: F,
    ) -> Result<String>
    where
        F: FnMut(&str),
    {
        let response = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&json!({
                "model": model,
                "messages": messages,
                "stream": true
            }))
            .send()
            .await
            .context("failed to connect to Ollama /api/chat")?
            .error_for_status()
            .context("Ollama /api/chat returned an error")?;

        let mut stream = response.bytes_stream();
        let mut pending = Vec::new();
        let mut full = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed while reading Ollama stream")?;
            pending.extend_from_slice(&chunk);
            consume_complete_lines(&mut pending, &mut full, &mut on_token)?;
        }

        if !pending.is_empty() {
            consume_chat_bytes(&pending, &mut full, &mut on_token)?;
        }

        Ok(full)
    }
}

fn consume_complete_lines<F>(
    pending: &mut Vec<u8>,
    full: &mut String,
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
            consume_chat_bytes(&line, full, on_token)?;
        }
    }
    Ok(())
}

fn consume_chat_bytes<F>(line: &[u8], full: &mut String, on_token: &mut F) -> Result<()>
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
            full.push_str(&message.content);
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
        let mut full = String::new();
        let mut emitted = String::new();

        pending.extend_from_slice(&bytes[..split_inside_multibyte]);
        consume_complete_lines(&mut pending, &mut full, &mut |token| {
            emitted.push_str(token)
        })
        .unwrap();
        assert!(full.is_empty());
        assert!(emitted.is_empty());

        pending.extend_from_slice(&bytes[split_inside_multibyte..]);
        consume_complete_lines(&mut pending, &mut full, &mut |token| emitted.push_str(token))
            .unwrap();

        assert_eq!(full, "Việt");
        assert_eq!(emitted, "Việt");
        assert!(pending.is_empty());
    }
}
