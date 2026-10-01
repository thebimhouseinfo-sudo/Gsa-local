use crate::registry::Registry;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub sequence: i64,
    pub plan_revision: Option<i64>,
    pub milestone: Option<String>,
    pub jobpack: Option<String>,
    pub stage: String,
    pub jobpack_status: Option<String>,
}

impl Checkpoint {
    pub fn new(stage: impl Into<String>) -> Self {
        Self {
            sequence: 0,
            plan_revision: None,
            milestone: None,
            jobpack: None,
            stage: stage.into(),
            jobpack_status: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoveryClassification {
    DurableExact,
    SourceDiverged,
    Blocked,
    NeedsHuman,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResumeAction {
    ResumeCoder,
    ResumeReviewer,
    ResumeInternalFix,
    ResumeTesterAttempt,
    ResumeLocalCr,
    RunRequiredVerification,
    CompleteMilestone,
    ActivateInitialMilestone,
    WaitExplicitNextMilestoneStart,
    BlockedNeedsHuman,
    ExecutionComplete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeDecision {
    pub classification: RecoveryClassification,
    pub action: ResumeAction,
    pub graph_version: Option<i64>,
    pub milestone_id: Option<String>,
    pub jobpack_id: Option<String>,
    pub change_set_id: Option<String>,
    pub checkpoint_id: Option<String>,
    pub attempt_id: Option<String>,
    pub reason: String,
}

pub struct CheckpointResolver<'a> {
    registry: &'a Registry,
}

impl<'a> CheckpointResolver<'a> {
    pub fn new(registry: &'a Registry) -> Self {
        Self { registry }
    }

    pub fn resolve(&self) -> Result<Option<Checkpoint>> {
        let checkpoint = self.registry.latest_checkpoint()?;
        let Some(checkpoint) = checkpoint else {
            return Ok(None);
        };

        if let Some(revision) = checkpoint.plan_revision {
            let binding = self.registry.plan_binding()?;
            match binding {
                Some(binding) if binding.revision == revision => {}
                Some(binding) => bail!(
                    "stale checkpoint plan revision {}; current approved revision is {}",
                    revision,
                    binding.revision
                ),
                None => bail!(
                    "checkpoint references a plan revision but no approved plan binding exists"
                ),
            }
        }

        Ok(Some(checkpoint))
    }
}
