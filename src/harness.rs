use anyhow::{bail, Result};
use std::{collections::HashMap, fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentId {
    General,
    Planner,
    JobBuilder,
    Reviewer,
    Coder,
    Tester,
    LocalCr,
    InternalFix,
}

impl AgentId {
    pub fn key(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Planner => "planner",
            Self::JobBuilder => "job_builder",
            Self::Reviewer => "reviewer",
            Self::Coder => "coder",
            Self::Tester => "tester",
            Self::LocalCr => "local_cr",
            Self::InternalFix => "internal_fix",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Planner => "Planner",
            Self::JobBuilder => "Job Builder",
            Self::Reviewer => "Reviewer",
            Self::Coder => "Coder",
            Self::Tester => "Tester",
            Self::LocalCr => "CR",
            Self::InternalFix => "Internal Fix",
        }
    }

    pub fn user_selectable() -> &'static [AgentId] {
        const AGENTS: &[AgentId] = &[
            AgentId::General,
            AgentId::Planner,
            AgentId::JobBuilder,
            AgentId::Reviewer,
            AgentId::Coder,
            AgentId::Tester,
            AgentId::LocalCr,
        ];
        AGENTS
    }

    pub fn is_user_selectable(self) -> bool {
        Self::user_selectable().contains(&self)
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

impl FromStr for AgentId {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let normalized = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        match normalized.as_str() {
            "general" => Ok(Self::General),
            "planner" | "plan" => Ok(Self::Planner),
            "job_builder" | "jobbuilder" | "jb" => Ok(Self::JobBuilder),
            "reviewer" | "review" => Ok(Self::Reviewer),
            "coder" | "code" => Ok(Self::Coder),
            "tester" | "test" => Ok(Self::Tester),
            "cr" | "local_cr" | "critic_reviewer" => Ok(Self::LocalCr),
            "internal_fix" => Ok(Self::InternalFix),
            _ => bail!("unknown agent: {value}"),
        }
    }
}

pub struct HarnessRegistry {
    shared: &'static str,
    roles: HashMap<AgentId, &'static str>,
}

impl Default for HarnessRegistry {
    fn default() -> Self {
        let roles = HashMap::from([
            (AgentId::General, include_str!("../harnesses/general.md")),
            (AgentId::Planner, include_str!("../harnesses/planner.md")),
            (AgentId::JobBuilder, include_str!("../harnesses/job_builder.md")),
            (AgentId::Reviewer, include_str!("../harnesses/reviewer.md")),
            (AgentId::Coder, include_str!("../harnesses/coder.md")),
            (AgentId::Tester, include_str!("../harnesses/tester.md")),
            (AgentId::LocalCr, include_str!("../harnesses/local_cr.md")),
            (AgentId::InternalFix, include_str!("../harnesses/internal_fix.md")),
        ]);
        Self {
            shared: include_str!("../harnesses/shared.md"),
            roles,
        }
    }
}

impl HarnessRegistry {
    pub fn compose(&self, agent: AgentId) -> Result<String> {
        let role = self
            .roles
            .get(&agent)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("missing harness for {}", agent.display_name()))?;
        Ok(format!("{}\n\n{}", self.shared.trim(), role.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_cr_is_selectable_but_internal_fix_is_not() {
        assert!(AgentId::LocalCr.is_user_selectable());
        assert!(!AgentId::InternalFix.is_user_selectable());
    }

    #[test]
    fn composed_harness_has_shared_and_role_contract() {
        let registry = HarnessRegistry::default();
        let text = registry.compose(AgentId::Planner).unwrap();
        assert!(text.contains("SHARED GSA LOCAL RULES"));
        assert!(text.contains("ROLE: PLANNER"));
    }
}
