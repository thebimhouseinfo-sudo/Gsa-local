use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanArtifact {
    pub goal: String,
    pub current_architecture: String,
    pub required_changes: Vec<String>,
    pub implementation_approach: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub sequence: Vec<String>,
    pub risks: Vec<String>,
    pub acceptance_direction: Vec<String>,
}

impl PlanArtifact {
    pub fn validate(&self) -> Result<()> {
        require_text("goal", &self.goal)?;
        require_text("current_architecture", &self.current_architecture)?;
        require_items("required_changes", &self.required_changes)?;
        require_items("implementation_approach", &self.implementation_approach)?;
        require_items("sequence", &self.sequence)?;
        require_items("risks", &self.risks)?;
        require_items("acceptance_direction", &self.acceptance_direction)?;
        Ok(())
    }

    pub fn hash(&self) -> Result<String> {
        self.validate()?;
        let canonical = serde_json::to_vec(self)?;
        let digest = Sha256::digest(canonical);
        let mut hash = String::with_capacity(digest.len() * 2);
        for byte in digest {
            write!(&mut hash, "{byte:02x}")?;
        }
        Ok(hash)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanRevision {
    pub revision: i64,
    pub hash: String,
    pub artifact: PlanArtifact,
}

fn require_text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("plan section {name} must not be empty");
    }
    Ok(())
}

fn require_items(name: &str, values: &[String]) -> Result<()> {
    if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
        bail!("plan section {name} must contain non-empty items");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PlanArtifact {
        PlanArtifact {
            goal: "Build X".into(),
            current_architecture: "Existing runtime".into(),
            required_changes: vec!["Add workflow".into()],
            implementation_approach: vec!["Persist state".into()],
            dependencies: vec![],
            sequence: vec!["Plan".into(), "Review".into()],
            risks: vec!["Stale verdict".into()],
            acceptance_direction: vec!["Approval is revision-bound".into()],
        }
    }

    #[test]
    fn hash_is_deterministic_and_content_sensitive() {
        let first = sample();
        let mut second = first.clone();
        assert_eq!(first.hash().unwrap(), second.hash().unwrap());
        second.goal = "Build Y".into();
        assert_ne!(first.hash().unwrap(), second.hash().unwrap());
    }

    #[test]
    fn validation_rejects_missing_required_sections() {
        let mut plan = sample();
        plan.sequence.clear();
        assert!(plan.validate().is_err());
    }
}
