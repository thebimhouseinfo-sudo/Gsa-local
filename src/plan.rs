use anyhow::{bail, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fmt::Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceMode {
    Verify,
    Measure,
    Probe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceNeed {
    pub id: String,
    pub question: String,
    pub purpose: String,
    pub required: bool,
    pub consumer: String,
    pub modes: Vec<EvidenceMode>,
    pub intent: String,
}

impl EvidenceNeed {
    fn validate(&self) -> Result<()> {
        require_text("evidence_need.id", &self.id)?;
        require_text("evidence_need.question", &self.question)?;
        require_text("evidence_need.purpose", &self.purpose)?;
        require_text("evidence_need.consumer", &self.consumer)?;
        require_text("evidence_need.intent", &self.intent)?;
        if self.modes.is_empty() {
            bail!("evidence_need.modes must contain at least one mode");
        }
        let mut modes = HashSet::new();
        if self.modes.iter().any(|mode| !modes.insert(*mode)) {
            bail!("evidence_need.modes must not contain duplicates");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_needs: Vec<EvidenceNeed>,
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
        let mut evidence_ids = HashSet::new();
        for need in &self.evidence_needs {
            need.validate()?;
            let id = need.id.trim().to_owned();
            if !evidence_ids.insert(id.clone()) {
                bail!("duplicate evidence_need id {id}");
            }
        }
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
            evidence_needs: vec![],
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
    fn evidence_need_changes_plan_hash_and_validates_identity() {
        let mut first = sample();
        first.evidence_needs.push(EvidenceNeed {
            id: "runtime-session".into(),
            question: "Does session identity survive a new chat?".into(),
            purpose: "Avoid inventing a session binding variable.".into(),
            required: true,
            consumer: "session architecture".into(),
            modes: vec![EvidenceMode::Probe],
            intent: "Observe identity stability across a controlled boundary change.".into(),
        });
        let mut second = first.clone();
        second.evidence_needs[0].question = "Does identity survive runtime restart?".into();
        assert_ne!(first.hash().unwrap(), second.hash().unwrap());

        second.evidence_needs[0].id = first.evidence_needs[0].id.clone();
        second.evidence_needs.push(first.evidence_needs[0].clone());
        assert!(second.validate().is_err());
    }

    #[test]
    fn legacy_plan_json_defaults_evidence_needs_and_preserves_hash_bytes() {
        let legacy_json = r#"{"goal":"Build X","current_architecture":"Existing runtime","required_changes":["Add workflow"],"implementation_approach":["Persist state"],"dependencies":[],"sequence":["Plan","Review"],"risks":["Stale verdict"],"acceptance_direction":["Approval is revision-bound"]}"#;
        let plan: PlanArtifact = serde_json::from_str(legacy_json).unwrap();
        assert!(plan.evidence_needs.is_empty());
        assert_eq!(serde_json::to_string(&plan).unwrap(), legacy_json);

        let digest = Sha256::digest(legacy_json.as_bytes());
        let expected = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(plan.hash().unwrap(), expected);
    }

    #[test]
    fn validation_rejects_missing_required_sections() {
        let mut plan = sample();
        plan.sequence.clear();
        assert!(plan.validate().is_err());
    }
}
