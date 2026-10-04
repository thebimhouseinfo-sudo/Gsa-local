use crate::harness::AgentId;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_is_agent_scoped() {
        let mut session = Session::default();
        session.set_model_override(AgentId::Coder, "hot".into());
        session.set_model_override(AgentId::Reviewer, "reviewer-hot".into());

        assert_eq!(session.model_override(AgentId::Coder), Some("hot"));
        assert_eq!(session.model_override(AgentId::Reviewer), Some("reviewer-hot"));
        assert_eq!(session.model_override(AgentId::Planner), None);
    }
}
