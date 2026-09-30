use gsa_local::{
    controller::MilestoneController,
    execution_graph::{
        CheckpointBoundaryKind, CheckpointPrerequisiteSpec, EvidenceOutputSpec, ExecutionGraph,
        JobPackSpec, MilestoneSpec, PrerequisiteState, TestCheckpointSpec, TodoSpec,
    },
    plan::{EvidenceMode, PlanArtifact},
    registry::{Registry, ReviewActor, ReviewVerdict, TesterCheckpointStatus},
    tester_evidence::{
        TesterAttemptEvidence, TesterClassification, TesterModeOutcome, TesterModeResult,
    },
};
use serde_json::json;
use std::time::Duration;
use tempfile::tempdir;

fn plan() -> PlanArtifact {
    PlanArtifact {
        goal: "Run only declared Tester checkpoints".into(),
        current_architecture: "Tester schema and execution substrate already exist".into(),
        required_changes: vec!["Add checkpoint orchestration".into()],
        implementation_approach: vec!["Resolve declared boundaries from registry state".into()],
        dependencies: vec![],
        sequence: vec!["review".into(), "checkpoint".into()],
        risks: vec!["implicit Tester invocation".into()],
        acceptance_direction: vec!["Tester runs only when declared and due".into()],
        evidence_needs: vec![],
    }
}

fn graph(required_capabilities: Vec<String>) -> ExecutionGraph {
    ExecutionGraph {
        milestones: vec![MilestoneSpec {
            id: "M1".into(),
            title: "Milestone".into(),
            order: 1,
        }],
        jobpacks: vec![JobPackSpec {
            id: "JP1".into(),
            milestone_id: "M1".into(),
            title: "Product slice".into(),
            goal: "Create reviewed target".into(),
            todo_ids: vec!["T1".into()],
            depends_on: vec![],
            required_inputs: vec!["approved plan".into()],
            expected_outputs: vec!["reviewed source".into()],
            acceptance: vec!["source reviewed".into()],
            verification_hints: vec!["deterministic check".into()],
        }],
        todos: vec![TodoSpec {
            id: "T1".into(),
            jobpack_id: "JP1".into(),
            title: "Implement".into(),
            checklist: vec!["implemented".into()],
        }],
        checkpoints: vec![TestCheckpointSpec {
            id: "CP1".into(),
            milestone_id: "M1".into(),
            boundary: CheckpointBoundaryKind::AfterJobpackSet,
            prerequisites: vec![CheckpointPrerequisiteSpec {
                jobpack_id: "JP1".into(),
                state: PrerequisiteState::ReviewPass,
            }],
            before_jobpack_id: None,
            evidence_need_ids: vec![],
            modes: vec![EvidenceMode::Verify],
            goal: "Verify reviewed slice".into(),
            criteria: vec!["Reviewed slice behaves correctly".into()],
            required_capabilities,
            experiment_dimensions: vec![],
            evidence_outputs: vec![EvidenceOutputSpec {
                id: "quality".into(),
                mode: EvidenceMode::Verify,
                description: "Checkpoint quality verdict".into(),
                required: false,
                evidence_need_id: None,
            }],
        }],
        evidence_requirements: vec![],
    }
}

fn before_jobpack_graph() -> ExecutionGraph {
    let mut graph = graph(vec!["VERIFY".into()]);
    graph.checkpoints[0].boundary = CheckpointBoundaryKind::BeforeJobpack;
    graph.checkpoints[0].prerequisites.clear();
    graph.checkpoints[0].before_jobpack_id = Some("JP1".into());
    graph
}

fn setup_with_graph(graph: ExecutionGraph) -> (tempfile::TempDir, Registry, i64) {
    let dir = tempdir().unwrap();
    let registry = Registry::open(dir.path()).unwrap();
    registry.begin_plan_workflow().unwrap();
    let revision = registry.persist_plan_revision(&plan()).unwrap();
    for actor in [ReviewActor::Reviewer, ReviewActor::LocalCr] {
        registry
            .record_plan_verdict(
                actor,
                revision.revision,
                &revision.hash,
                ReviewVerdict::Pass,
                &[],
            )
            .unwrap();
    }
    registry
        .approve_current_plan(revision.revision, &revision.hash)
        .unwrap();
    let version = registry
        .register_execution_graph(revision.revision, &revision.hash, &graph)
        .unwrap();
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();
    (dir, registry, version)
}

fn prepare_review_target(
    dir: &tempfile::TempDir,
    registry: &Registry,
    version: i64,
) {
    let controller = MilestoneController::new(registry, dir.path(), "owner-a");
    let active = controller.resolve_or_activate().unwrap().unwrap();
    assert_eq!(active.jobpack_id, "JP1");

    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP1")
        .unwrap();
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            "review target",
            &[],
            &["target ready".into()],
            &json!([{
                "path":"src/example.rs",
                "before_sha256":"before",
                "after_sha256":"after"
            }]),
            1,
            0,
        )
        .unwrap();
}

fn pass_review(
    dir: &tempfile::TempDir,
    registry: &Registry,
    version: i64,
) {
    registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            ReviewVerdict::Pass,
            &[],
            1,
            1,
        )
        .unwrap();
}

#[test]
fn checkpoint_is_due_only_after_exact_review_pass_and_never_marks_jobpack_done() {
    let (dir, registry, version) = setup_with_graph(graph(vec!["VERIFY".into()]));
    prepare_review_target(&dir, &registry, version);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");

    assert!(controller
        .resolve_due_tester_checkpoint(&["VERIFY".into()])
        .unwrap()
        .is_none());

    pass_review(&dir, &registry, version);
    let due = controller
        .resolve_due_tester_checkpoint(&["VERIFY".into()])
        .unwrap()
        .unwrap();
    assert_eq!(due.status, TesterCheckpointStatus::Due);
    assert_eq!(
        due.target.as_ref().unwrap().prerequisites[0]
            .change_set_id
            .as_deref(),
        Some("change-1")
    );
    assert_eq!(
        registry.jobpack_status(version, "JP1").unwrap().as_deref(),
        Some("ACTIVE")
    );

    let running = controller
        .begin_due_tester_attempt(version, "CP1")
        .unwrap();
    let attempt = TesterAttemptEvidence {
        graph_version: version,
        checkpoint_id: "CP1".into(),
        attempt_id: running.attempt_id.clone().unwrap(),
        target: running.target.clone().unwrap(),
        mode_results: vec![TesterModeResult {
            mode: EvidenceMode::Verify,
            outcome: TesterModeOutcome::Pass,
            reason: None,
        }],
        classifications: vec![],
        experiment: None,
        outputs: vec![],
        limitations: vec![],
    };
    registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &attempt)
        .unwrap();

    let state = registry
        .tester_checkpoint_state(version, "CP1")
        .unwrap()
        .unwrap();
    assert_eq!(state.status, TesterCheckpointStatus::Satisfied);
    assert_eq!(
        registry.jobpack_status(version, "JP1").unwrap().as_deref(),
        Some("ACTIVE")
    );
    assert_eq!(
        registry.milestone_status(version, "M1").unwrap().as_deref(),
        Some("ACTIVE")
    );
}

#[test]
fn missing_required_capability_blocks_checkpoint_without_progression() {
    let (dir, registry, version) = setup_with_graph(graph(vec!["RUNTIME_PROBE".into()]));
    prepare_review_target(&dir, &registry, version);
    pass_review(&dir, &registry, version);

    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let blocked = controller
        .resolve_due_tester_checkpoint(&[])
        .unwrap()
        .unwrap();
    assert_eq!(blocked.status, TesterCheckpointStatus::Blocked);
    assert!(blocked
        .reason
        .as_deref()
        .unwrap()
        .contains("RUNTIME_PROBE"));
    assert_eq!(
        registry.jobpack_status(version, "JP1").unwrap().as_deref(),
        Some("ACTIVE")
    );
}

#[test]
fn integration_not_ready_stops_checkpoint_even_when_mode_claims_success() {
    let (dir, registry, version) = setup_with_graph(graph(vec!["VERIFY".into()]));
    prepare_review_target(&dir, &registry, version);
    pass_review(&dir, &registry, version);

    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let due = controller
        .resolve_due_tester_checkpoint(&["VERIFY".into()])
        .unwrap()
        .unwrap();
    assert_eq!(due.status, TesterCheckpointStatus::Due);
    let running = controller
        .begin_due_tester_attempt(version, "CP1")
        .unwrap();

    let attempt = TesterAttemptEvidence {
        graph_version: version,
        checkpoint_id: "CP1".into(),
        attempt_id: running.attempt_id.clone().unwrap(),
        target: running.target.clone().unwrap(),
        mode_results: vec![TesterModeResult {
            mode: EvidenceMode::Verify,
            outcome: TesterModeOutcome::Pass,
            reason: None,
        }],
        classifications: vec![TesterClassification::IntegrationNotReady],
        experiment: None,
        outputs: vec![],
        limitations: vec![],
    };
    registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &attempt)
        .unwrap();

    let state = registry
        .tester_checkpoint_state(version, "CP1")
        .unwrap()
        .unwrap();
    assert_eq!(state.status, TesterCheckpointStatus::Blocked);
    assert_eq!(
        registry.jobpack_status(version, "JP1").unwrap().as_deref(),
        Some("ACTIVE")
    );
}

#[test]
fn before_jobpack_checkpoint_prevents_activation_until_declared_gate_runs() {
    let (dir, registry, version) = setup_with_graph(before_jobpack_graph());
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");

    assert!(controller.resolve_or_activate().unwrap().is_none());
    assert_eq!(registry.active_jobpack_count(version).unwrap(), 0);

    let due = controller
        .resolve_due_tester_checkpoint(&["VERIFY".into()])
        .unwrap()
        .unwrap();
    assert_eq!(due.status, TesterCheckpointStatus::Due);
    assert!(due.target.as_ref().unwrap().prerequisites.is_empty());
    assert_eq!(registry.active_jobpack_count(version).unwrap(), 0);
}
