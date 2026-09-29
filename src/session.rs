use crate::{config::AppConfig, harness::AgentId};
use std::collections::HashMap;

#[derive(Debug)]
pub struct Session {
    pub active_agent: AgentId,
    model_overrides: HashMap<AgentId, String>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            active_agent: AgentId::General,
            model_overrides: HashMap::new(),
        }
    }
}

impl Session {
    pub fn set_active_agent(&mut self, agent: AgentId) {
        self.active_agent = agent;
    }

    pub fn set_model_override(&mut self, agent: AgentId, model: String) {
        self.model_overrides.insert(agent, model);
    }

    pub fn model_override(&self, agent: AgentId) -> Option<&str> {
        self.model_overrides.get(&agent).map(String::as_str)
    }

    pub fn resolved_model<'a>(&'a self, config: &'a AppConfig, agent: AgentId) -> Option<&'a str> {
        self.model_override(agent).or_else(|| config.model_for(agent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_is_agent_scoped_and_beats_preset() {
        let mut config = AppConfig::default();
        config.set_agent_model(AgentId::Coder, "preset".into());
        config.set_agent_model(AgentId::Reviewer, "reviewer-preset".into());

        let mut session = Session::default();
        session.set_model_override(AgentId::Coder, "hot".into());

        assert_eq!(session.resolved_model(&config, AgentId::Coder), Some("hot"));
        assert_eq!(
            session.resolved_model(&config, AgentId::Reviewer),
            Some("reviewer-preset")
        );
    }
}
