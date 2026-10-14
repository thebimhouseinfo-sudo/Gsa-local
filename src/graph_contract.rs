use crate::{
    execution_graph::{CheckpointBoundaryKind, ExecutionGraph, PrerequisiteState},
    plan::PlanArtifact,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Immutable intent: no code, requirements, evidence identifiers or Job Pack
/// definitions may be modified as part of a topology-only amendment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphContractAmendment {
    pub graph_version: i64,
    pub plan_revision: i64,
    pub plan_hash: String,
    pub checkpoint_id: String,
    pub before_hash: String,
    pub after_hash: String,
    pub original_checkpoint_json: String,
    pub replacement_checkpoint_json: String,
}

pub fn effective_graph_hash(graph: &ExecutionGraph) -> Result<String> {
    // Normalized order is independent of database row iteration order.
    let mut canonical = graph.clone();
    canonical
        .milestones
        .sort_by(|a, b| a.order.cmp(&b.order).then(a.id.cmp(&b.id)));
    canonical.jobpacks.sort_by(|a, b| a.id.cmp(&b.id));
    canonical.todos.sort_by(|a, b| a.id.cmp(&b.id));
    canonical.checkpoints.sort_by(|a, b| a.id.cmp(&b.id));
    canonical.evidence_requirements.sort_by(|a, b| {
        (&a.consumer_jobpack_id, &a.checkpoint_id, &a.output_id).cmp(&(
            &b.consumer_jobpack_id,
            &b.checkpoint_id,
            &b.output_id,
        ))
    });
    for pack in &mut canonical.jobpacks {
        pack.depends_on.sort();
        pack.todo_ids.sort();
    }
    for checkpoint in &mut canonical.checkpoints {
        checkpoint
            .prerequisites
            .sort_by(|a, b| a.jobpack_id.cmp(&b.jobpack_id));
        checkpoint.evidence_outputs.sort_by(|a, b| a.id.cmp(&b.id));
        checkpoint.evidence_need_ids.sort();
    }
    let bytes = serde_json::to_vec(&canonical)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn amendment_fingerprint(amendment: &GraphContractAmendment) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(amendment)?)))
}

pub fn stock_qa_timing_amendment(
    graph: &ExecutionGraph,
    plan: &PlanArtifact,
    graph_version: i64,
    plan_revision: i64,
    plan_hash: &str,
) -> Result<(GraphContractAmendment, ExecutionGraph)> {
    if graph_version < 1 || plan_revision < 1 || plan_hash.trim().is_empty() {
        bail!("GRAPH_REPAIR_BLOCKED: invalid original graph/plan binding");
    }
    let checkpoint = graph
        .checkpoints
        .iter()
        .find(|item| item.id == "chk-j1-verify")
        .context("GRAPH_REPAIR_BLOCKED: checkpoint chk-j1-verify not found")?;
    if checkpoint.boundary != CheckpointBoundaryKind::MilestoneGate
        || checkpoint.milestone_id != "m1-reservation-correctness"
        || checkpoint.prerequisites.len() != 1
        || checkpoint.prerequisites[0].jobpack_id != "jp-j1-fefo-atomicity"
        || checkpoint.prerequisites[0].state != PrerequisiteState::Done
    {
        bail!("GRAPH_REPAIR_BLOCKED: original checkpoint contract differs from allowlist");
    }
    if !checkpoint
        .evidence_outputs
        .iter()
        .any(|output| output.id == "ev-j1-verify" && output.required)
        || !graph.evidence_requirements.iter().any(|requirement| {
            requirement.consumer_jobpack_id == "jp-j2-idempotency"
                && requirement.checkpoint_id == checkpoint.id
                && requirement.output_id == "ev-j1-verify"
                && requirement.required
        })
    {
        bail!("GRAPH_REPAIR_BLOCKED: J1 Tester output/J2 requirement differs from allowlist");
    }
    let mut candidate = graph.clone();
    let changed = candidate
        .checkpoints
        .iter_mut()
        .find(|item| item.id == checkpoint.id)
        .context("checkpoint disappeared")?;
    changed.boundary = CheckpointBoundaryKind::AfterJobpackSet;
    candidate.validate_against_plan(plan)?;
    let amendment = GraphContractAmendment {
        graph_version,
        plan_revision,
        plan_hash: plan_hash.to_string(),
        checkpoint_id: checkpoint.id.clone(),
        before_hash: effective_graph_hash(graph)?,
        after_hash: effective_graph_hash(&candidate)?,
        original_checkpoint_json: serde_json::to_string(checkpoint)?,
        replacement_checkpoint_json: serde_json::to_string(changed)?,
    };
    if amendment.before_hash == amendment.after_hash {
        bail!("GRAPH_REPAIR_BLOCKED: no effective contract change");
    }
    Ok((amendment, candidate))
}
