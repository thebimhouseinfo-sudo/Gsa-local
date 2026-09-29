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
        Self { role: "system".into(), content: content.into() }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into() }
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
        let mut pending = String::new();
        let mut full = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed while reading Ollama stream")?;
            pending.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(newline) = pending.find('\n') {
                let line = pending[..newline].trim().to_owned();
                pending.drain(..=newline);
                if !line.is_empty() {
                    consume_chat_line(&line, &mut full, &mut on_token)?;
                }
            }
        }

        let tail = pending.trim();
        if !tail.is_empty() {
            consume_chat_line(tail, &mut full, &mut on_token)?;
        }

        Ok(full)
    }
}

fn consume_chat_line<F>(line: &str, full: &mut String, on_token: &mut F) -> Result<()>
where
    F: FnMut(&str),
{
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
