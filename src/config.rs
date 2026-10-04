use crate::harness::AgentId;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_ollama_url")]
    pub ollama_base_url: String,
    #[serde(default = "default_ollama_num_ctx")]
    pub ollama_num_ctx: usize,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub agent_models: HashMap<String, String>,
}

fn default_ollama_url() -> String {
    "http://127.0.0.1:11434".to_owned()
}

fn default_ollama_num_ctx() -> usize {
    32 * 1024
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ollama_base_url: default_ollama_url(),
            ollama_num_ctx: default_ollama_num_ctx(),
            default_model: None,
            agent_models: HashMap::new(),
        }
    }
}

impl AppConfig {
    pub fn config_path() -> Result<PathBuf> {
        let base = dirs::config_dir().context("unable to resolve user config directory")?;
        Ok(base.join("gsa").join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        Self::load_from(&Self::config_path()?)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("failed to parse config {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path()?)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }
        let text = toml::to_string_pretty(self).context("failed to serialize config")?;
        fs::write(path, text).with_context(|| format!("failed to write config {}", path.display()))
    }

    pub fn agent_model(&self, agent: AgentId) -> Option<&str> {
        self.agent_models.get(agent.key()).map(String::as_str)
    }

    pub fn model_for(&self, agent: AgentId) -> Option<&str> {
        self.agent_model(agent).or(self.default_model.as_deref())
    }

    pub fn set_default_model(&mut self, model: Option<String>) {
        self.default_model = model;
    }

    pub fn set_agent_model(&mut self, agent: AgentId, model: String) {
        self.agent_models.insert(agent.key().to_owned(), model);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn config_round_trip_preserves_agent_mapping() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = AppConfig::default();
        config.set_agent_model(AgentId::Coder, "qwen-local".into());
        config.save_to(&path).unwrap();

        let loaded = AppConfig::load_from(&path).unwrap();
        assert_eq!(loaded.agent_model(AgentId::Coder), Some("qwen-local"));
        assert_eq!(loaded.model_for(AgentId::Coder), Some("qwen-local"));
    }

    #[test]
    fn exact_agent_model_is_distinct_from_default_model() {
        let mut config = AppConfig::default();
        config.set_default_model(Some("default".into()));
        config.set_agent_model(AgentId::Reviewer, "reviewer".into());

        assert_eq!(config.agent_model(AgentId::Coder), None);
        assert_eq!(config.model_for(AgentId::Coder), Some("default"));
        assert_eq!(config.agent_model(AgentId::Reviewer), Some("reviewer"));
        assert_eq!(config.model_for(AgentId::Reviewer), Some("reviewer"));
    }
}
