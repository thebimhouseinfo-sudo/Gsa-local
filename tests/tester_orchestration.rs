use gsa_local::{
    controller::{MilestoneController, NextWork},
    execution_graph::{
        CheckpointBoundaryKind, CheckpointPrerequisiteSpec, EvidenceOutputSpec, ExecutionGraph,
        JobPackSpec, MilestoneSpec, PrerequisiteState, TestCheckpointSpec, TodoSpec,
    },
    plan::{EvidenceMode, PlanArtifact},
    registry::{Registry, ReviewActor, ReviewVerdict, TesterCheckpointDisposition},
    tester_evidence::{
        TesterAttemptEvidence, TesterClassification, TesterModeOutcome, TesterModeResult,
    },
};
use serde_json::json;
use std::time::Duration;
use tempfile::tempdir;

fn plan() -> PlanArtifact {
    PlanArtifact {
        goal: "tester orchestration".into(),
        current_architecture: "declared checkpoints".into(),
        required_changes: vec!["resolve checkpoints".into()],
        implementation_approach: vec!["derive from registry".into()],
        dependencies: vec![],
        sequence: vec!["review".into(), "checkpoint".into()],
        risks: vec!["implicit Tester".into()],
        acceptance_direction: vec!["declared only".into()],
        evidence_needs: vec![],
    }
}

fn graph(required_capabilities: Vec<String>) -> ExecutionGraph {
    ExecutionGraph {
        milestones: vec![MilestoneSpec { id: "M1".into(), title: "M1".into(), order: 1 }],
        jobpacks: vec![JobPackSpec {
            id: "JP1".into(), milestone_id: "M1".into(), title: "JP1".into(),
            goal: "reviewed target".into(), todo_ids: vec!["T1".into()],
            depends_on: vec![], required_inputs: vec!["plan".into()],
            expected_outputs: vec!["source".into()], acceptance: vec!["reviewed".into()],
            verification_hints: vec!["checkpoint".into()],
        }],
        todos: vec![TodoSpec {
            id: "T1".into(), jobpack_id: "JP1".into(), title: "T1".into(),
            checklist: vec!["implement".into()],
        }],
        checkpoints: vec![TestCheckpointSpec {
            id: "CP1".into(), milestone_id: "M1".into(),
            boundary: CheckpointBoundaryKind::AfterJobpackSet,
            prerequisites: vec![CheckpointPrerequisiteSpec {
                jobpack_id: "JP1".into(), state: PrerequisiteState::ReviewPass,
            }],
            before_jobpack_id: None, evidence_need_ids: vec![],
            modes: vec![EvidenceMode::Verify],
            goal: "verify reviewed target".into(), criteria: vec!["works".into()],
            required_capabilities, experiment_dimensions: vec![],
            evidence_outputs: vec![EvidenceOutputSpec {
                id: "verdict".into(), mode: EvidenceMode::Verify,
                description: "verdict".into(), required: false, evidence_need_id: None,
            }],
        }],
        evidence_requirements: vec![],
    }
}

fn setup(required_capabilities: Vec<String>) -> (tempfile::TempDir, Registry, i64) {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    registry.begin_plan_workflow().unwrap();
    let revision = registry.persist_plan_revision(&plan()).unwrap();
    for actor in [ReviewActor::Reviewer, ReviewActor::LocalCr] {
        registry.record_plan_verdict(
            actor, revision.revision, &revision.hash, ReviewVerdict::Pass, &[]
        ).unwrap();
    }
    registry.approve_current_plan(revision.revision, &revision.hash).unwrap();
    let version = registry.register_execution_graph(
        revision.revision, &revision.hash, &graph(required_capabilities)
    ).unwrap();
    registry.acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600)).unwrap();
    MilestoneController::new(&registry, dir.path(), "owner-a")
        .resolve_or_activate().unwrap().unwrap();
    registry.begin_code_workflow(dir.path(), "owner-a", version, "JP1").unwrap();
    registry.record_code_checkpoint(
        dir.path(), "owner-a", version, "JP1", "change-1", "review target",
        &[], &[], &json!([{"path":"src/example.rs","before_sha256":"a","after_sha256":"b"}]),
        1, 0
    ).unwrap();
    registry.record_code_review(
        dir.path(), "owner-a", version, "JP1", "change-1",
        ReviewVerdict::Pass, &[], 1, 1
    ).unwrap();
    (dir, registry, version)
}

fn tester_work(
    controller: &MilestoneController<'_>,
    capabilities: &[String],
) -> gsa_local::registry::TesterCheckpointWorkRecord {
    match controller.resolve_next(capabilities).unwrap().unwrap() {
        NextWork::Tester(work) => work,
        NextWork::Coder(work) => panic!("expected Tester, got Coder {}", work.jobpack_id),
    }
}

#[test]
fn review_pass_is_due_without_marking_jobpack_done() {
    let (dir, registry, version) = setup(vec![]);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let work = tester_work(&controller, &[]);
    assert_eq!(work.disposition, TesterCheckpointDisposition::Due);
    assert_eq!(work.target.unwrap().prerequisites[0].change_set_id.as_deref(), Some("change-1"));
    assert_eq!(registry.jobpack_status(version, "JP1").unwrap().as_deref(), Some("ACTIVE"));
}

#[test]
fn exact_target_pass_satisfies_checkpoint_without_completion() {
    let (dir, registry, version) = setup(vec![]);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let due = tester_work(&controller, &[]);
    registry.record_tester_attempt_evidence(
        dir.path(), "owner-a",
        &TesterAttemptEvidence {
            graph_version: version, checkpoint_id: "CP1".into(),
            attempt_id: due.next_attempt_id.unwrap(), target: due.target.unwrap(),
            mode_results: vec![TesterModeResult {
                mode: EvidenceMode::Verify, outcome: TesterModeOutcome::Pass, reason: None,
            }],
            classifications: vec![], experiment: None, outputs: vec![], limitations: vec![],
        }
    ).unwrap();

    match controller.resolve_next(&[]).unwrap().unwrap() {
        NextWork::Coder(work) => assert_eq!(work.jobpack_id, "JP1"),
        NextWork::Tester(work) => panic!("checkpoint remained {}", work.disposition.as_str()),
    }
    assert_eq!(registry.jobpack_status(version, "JP1").unwrap().as_deref(), Some("ACTIVE"));
}

#[test]
fn missing_capability_is_integration_not_ready() {
    let (dir, registry, _version) = setup(vec!["BROWSER".into()]);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let work = tester_work(&controller, &["INTEGRATION".into()]);
    assert_eq!(work.disposition, TesterCheckpointDisposition::IntegrationNotReady);
    assert!(work.reason.unwrap().contains("BROWSER"));
}

#[test]
fn fail_and_needs_human_stop_coder_progression() {
    for (outcome, expected) in [
        (TesterModeOutcome::Fail, TesterCheckpointDisposition::Blocked),
        (TesterModeOutcome::NeedsHuman, TesterCheckpointDisposition::NeedsHuman),
    ] {
        let (dir, registry, version) = setup(vec![]);
        let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
        let due = tester_work(&controller, &[]);
        registry.record_tester_attempt_evidence(
            dir.path(), "owner-a",
            &TesterAttemptEvidence {
                graph_version: version, checkpoint_id: "CP1".into(),
                attempt_id: due.next_attempt_id.unwrap(), target: due.target.unwrap(),
                mode_results: vec![TesterModeResult {
                    mode: EvidenceMode::Verify, outcome,
                    reason: Some("not satisfied".into()),
                }],
                classifications: vec![TesterClassification::ProductFailure],
                experiment: None, outputs: vec![], limitations: vec![],
            }
        ).unwrap();
        assert_eq!(tester_work(&controller, &[]).disposition, expected);
        assert_eq!(registry.jobpack_status(version, "JP1").unwrap().as_deref(), Some("ACTIVE"));
    }
}
