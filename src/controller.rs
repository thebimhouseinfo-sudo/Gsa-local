use crate::{
    execution_graph::{PrerequisiteState, TestCheckpointSpec},
    registry::{
        ActiveWorkRecord, Registry, TesterCheckpointDisposition, TesterCheckpointWorkRecord,
        TesterRetestContext,
    },
    tester_evidence::TesterTargetBinding,
};
use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWork {
    pub graph_version: i64,
    pub plan_revision: i64,
    pub plan_hash: String,
    pub milestone_id: String,
    pub milestone_title: String,
    pub milestone_status: String,
    pub jobpack_id: String,
    pub jobpack_title: String,
    pub jobpack_status: String,
    pub goal: String,
    pub required_inputs: Vec<String>,
    pub expected_outputs: Vec<String>,
    pub acceptance: Vec<String>,
    pub verification_hints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextWork {
    Coder(ActiveWork),
    Tester(TesterCheckpointWorkRecord),
    Repair(TesterRepairWork),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TesterRepairWork {
    pub active_work: ActiveWork,
    pub checkpoint: TestCheckpointSpec,
    pub failed_target: TesterTargetBinding,
    pub retest_context: TesterRetestContext,
}

impl From<ActiveWorkRecord> for ActiveWork {
    fn from(value: ActiveWorkRecord) -> Self {
        Self {
            graph_version: value.graph_version,
            plan_revision: value.plan_revision,
            plan_hash: value.plan_hash,
            milestone_id: value.milestone_id,
            milestone_title: value.milestone_title,
            milestone_status: value.milestone_status,
            jobpack_id: value.jobpack_id,
            jobpack_title: value.jobpack_title,
            jobpack_status: value.jobpack_status,
            goal: value.goal,
            required_inputs: value.required_inputs,
            expected_outputs: value.expected_outputs,
            acceptance: value.acceptance,
            verification_hints: value.verification_hints,
        }
    }
}

pub struct MilestoneController<'a> {
    registry: &'a Registry,
    project_root: &'a Path,
    lease_owner: &'a str,
}

impl<'a> MilestoneController<'a> {
    pub fn new(registry: &'a Registry, project_root: &'a Path, lease_owner: &'a str) -> Self {
        Self {
            registry,
            project_root,
            lease_owner,
        }
    }

    pub fn resolve_next(&self, available_capabilities: &[String]) -> Result<Option<NextWork>> {
        if let Some(mut checkpoint) = self
            .registry
            .resolve_tester_checkpoint(available_capabilities)?
        {
            if checkpoint.disposition == TesterCheckpointDisposition::ProductFailure {
                let Some(target) = checkpoint.target.clone() else {
                    checkpoint.disposition = TesterCheckpointDisposition::Blocked;
                    checkpoint.reason =
                        Some("PRODUCT_FAILURE checkpoint is missing exact target binding".into());
                    return Ok(Some(NextWork::Tester(checkpoint)));
                };
                let Some(retest_context) = checkpoint.retest_context.clone() else {
                    checkpoint.disposition = TesterCheckpointDisposition::Blocked;
                    checkpoint.reason =
                        Some("PRODUCT_FAILURE checkpoint is missing failed-attempt context".into());
                    return Ok(Some(NextWork::Tester(checkpoint)));
                };
                let repair_targets = target
                    .prerequisites
                    .iter()
                    .filter(|item| item.state == PrerequisiteState::ReviewPass)
                    .collect::<Vec<_>>();
                if repair_targets.len() != 1 {
                    checkpoint.disposition = TesterCheckpointDisposition::Blocked;
                    checkpoint.reason = Some(format!(
                        "PRODUCT_FAILURE repair requires exactly one REVIEW_PASS prerequisite; found {}",
                        repair_targets.len()
                    ));
                    return Ok(Some(NextWork::Tester(checkpoint)));
                }

                let Some(active_work) = self.registry.current_active_work()? else {
                    checkpoint.disposition = TesterCheckpointDisposition::Blocked;
                    checkpoint.reason =
                        Some("PRODUCT_FAILURE repair requires an ACTIVE Job Pack".into());
                    return Ok(Some(NextWork::Tester(checkpoint)));
                };
                if active_work.jobpack_id != repair_targets[0].jobpack_id {
                    checkpoint.disposition = TesterCheckpointDisposition::Blocked;
                    checkpoint.reason = Some(format!(
                        "PRODUCT_FAILURE repair target {} is not the ACTIVE Job Pack {}",
                        repair_targets[0].jobpack_id, active_work.jobpack_id
                    ));
                    return Ok(Some(NextWork::Tester(checkpoint)));
                }

                return Ok(Some(NextWork::Repair(TesterRepairWork {
                    active_work: active_work.into(),
                    checkpoint: checkpoint.checkpoint,
                    failed_target: target,
                    retest_context,
                })));
            }
            return Ok(Some(NextWork::Tester(checkpoint)));
        }
        self.resolve_or_activate()
            .map(|work| work.map(NextWork::Coder))
    }

    pub fn resolve_or_activate(&self) -> Result<Option<ActiveWork>> {
        self.registry
            .resolve_or_activate_work(self.project_root, self.lease_owner)
            .map(|work| work.map(Into::into))
    }

    pub fn mark_active_jobpack_done(&self) -> Result<Option<ActiveWork>> {
        self.registry
            .complete_active_jobpack(self.project_root, self.lease_owner)
            .map(|work| work.map(Into::into))
    }

    pub fn mark_verified_milestone_complete(
        &self,
        milestone_id: &str,
    ) -> Result<Option<ActiveWork>> {
        self.registry
            .complete_verified_milestone(self.project_root, self.lease_owner, milestone_id)
            .map(|work| work.map(Into::into))
    }
}
