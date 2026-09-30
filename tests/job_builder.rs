use gsa_local::{
    execution_graph::{
        CheckpointBoundaryKind, CheckpointPrerequisiteSpec, EvidenceOutputSpec,
        EvidenceRequirementSpec, ExecutionGraph, JobPackSpec, MilestoneSpec, PrerequisiteState,
        TestCheckpointSpec, TodoSpec,
    },
    plan::{EvidenceMode, EvidenceNeed, PlanArtifact},
    registry::{Registry, ReviewActor, ReviewVerdict},
};
use tempfile::tempdir;

fn plan(goal: &str) -> PlanArtifact {
    PlanArtifact {
        goal: goal.into(),
        current_architecture: "Existing runtime".into(),
        required_changes: vec!["Add execution graph".into()],
        implementation_approach: vec!["Register graph transactionally".into()],
        dependencies: vec![],
        sequence: vec!["Build".into(), "Register".into()],
        risks: vec!["Stale plan binding".into()],
        acceptance_direction: vec!["Graph is registry truth".into()],
        evidence_needs: vec![],
    }
}

fn valid_graph() -> ExecutionGraph {
    ExecutionGraph {
        milestones: vec![
            MilestoneSpec {
                id: "M1".into(),
                title: "Foundation".into(),
                order: 1,
            },
            MilestoneSpec {
                id: "M2".into(),
                title: "Workflow".into(),
                order: 2,
            },
        ],
        jobpacks: vec![
            JobPackSpec {
                id: "JP1".into(),
                milestone_id: "M1".into(),
                title: "Foundation pack".into(),
                goal: "Build foundation".into(),
                todo_ids: vec!["T1".into()],
                depends_on: vec![],
                required_inputs: vec!["approved inputs".into()],
                expected_outputs: vec!["foundation output".into()],
                acceptance: vec!["Foundation complete".into()],
                verification_hints: vec!["cargo test".into()],
            },
            JobPackSpec {
                id: "JP2".into(),
                milestone_id: "M2".into(),
                title: "Workflow pack".into(),
                goal: "Build workflow".into(),
                todo_ids: vec!["T2".into()],
                depends_on: vec!["JP1".into()],
                required_inputs: vec!["foundation output".into()],
                expected_outputs: vec!["workflow output".into()],
                acceptance: vec!["Workflow complete".into()],
                verification_hints: vec!["integration test".into()],
            },
        ],
        todos: vec![
            TodoSpec {
                id: "T1".into(),
                jobpack_id: "JP1".into(),
                title: "Foundation todo".into(),
                checklist: vec!["Implement".into(), "Verify".into()],
            },
            TodoSpec {
                id: "T2".into(),
                jobpack_id: "JP2".into(),
                title: "Workflow todo".into(),
                checklist: vec!["Implement".into(), "Verify".into()],
            },
        ],
        checkpoints: vec![],
        evidence_requirements: vec![],
    }
}

fn approve(registry: &Registry, artifact: PlanArtifact) -> (i64, String) {
    registry.begin_plan_workflow().unwrap();
    let revision = registry.persist_plan_revision(&artifact).unwrap();
    registry
        .record_plan_verdict(
            ReviewActor::Reviewer,
            revision.revision,
            &revision.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .unwrap();
    registry
        .record_plan_verdict(
            ReviewActor::LocalCr,
            revision.revision,
            &revision.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .unwrap();
    registry
        .approve_current_plan(revision.revision, &revision.hash)
        .unwrap();
    (revision.revision, revision.hash)
}

#[test]
fn validator_rejects_duplicate_jobpack_ids() {
    let mut graph = valid_graph();
    graph.jobpacks[1].id = "JP1".into();
    assert!(graph.validate().is_err());
}

#[test]
fn validator_rejects_missing_jobpack_input_output_contract() {
    let mut graph = valid_graph();
    graph.jobpacks[0].required_inputs.clear();
    assert!(graph.validate().is_err());

    let mut graph = valid_graph();
    graph.jobpacks[0].expected_outputs.clear();
    assert!(graph.validate().is_err());
}

#[test]
fn validator_rejects_missing_milestone() {
    let mut graph = valid_graph();
    graph.jobpacks[0].milestone_id = "M404".into();
    assert!(graph.validate().is_err());
}

#[test]
fn validator_rejects_missing_dependency() {
    let mut graph = valid_graph();
    graph.jobpacks[1].depends_on = vec!["JP404".into()];
    assert!(graph.validate().is_err());
}

#[test]
fn validator_rejects_dependency_cycle() {
    let mut graph = valid_graph();
    graph.jobpacks[0].depends_on = vec!["JP2".into()];
    graph.jobpacks[1].depends_on = vec!["JP1".into()];
    assert!(graph.validate().is_err());
}

#[test]
fn validator_rejects_orphan_todo() {
    let mut graph = valid_graph();
    graph.jobpacks[0].todo_ids.clear();
    assert!(graph.validate().is_err());
}

#[test]
fn validator_rejects_empty_milestone() {
    let mut graph = valid_graph();
    graph.jobpacks.retain(|pack| pack.milestone_id != "M2");
    graph.todos.retain(|todo| todo.jobpack_id != "JP2");
    assert!(graph.validate().is_err());
}

#[test]
fn invalid_graph_leaves_no_partial_registration() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let (revision, hash) = approve(&registry, plan("invalid graph"));

    let mut graph = valid_graph();
    graph.jobpacks[1].depends_on = vec!["JP404".into()];
    assert!(registry
        .register_execution_graph(revision, &hash, &graph)
        .is_err());
    assert_eq!(registry.current_execution_graph_version().unwrap(), None);
}

#[test]
fn stale_plan_binding_is_rejected() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let (old_revision, old_hash) = approve(&registry, plan("old"));
    let (_new_revision, _new_hash) = approve(&registry, plan("new"));

    assert!(registry
        .register_execution_graph(old_revision, &old_hash, &valid_graph())
        .is_err());
    assert_eq!(registry.current_execution_graph_version().unwrap(), None);
}

#[test]
fn registration_is_normalized_checkpointed_and_not_active() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let (revision, hash) = approve(&registry, plan("register"));

    let version = registry
        .register_execution_graph(revision, &hash, &valid_graph())
        .unwrap();

    assert_eq!(version, 1);
    assert_eq!(
        registry.execution_graph_counts(version).unwrap(),
        (2, 2, 2, 4)
    );
    assert!(!registry.has_active_jobpack(version).unwrap());

    let checkpoint = registry.latest_checkpoint().unwrap().unwrap();
    assert_eq!(checkpoint.plan_revision, Some(revision));
    assert_eq!(checkpoint.stage, "graph_registered");
    assert_eq!(checkpoint.milestone, None);
    assert_eq!(checkpoint.jobpack, None);

    let binding = registry.plan_binding().unwrap().unwrap();
    assert_eq!(binding.execution_graph_version, version);

    let (required_inputs, expected_outputs) = registry
        .execution_jobpack_contract(version, "JP2")
        .unwrap()
        .unwrap();
    assert_eq!(required_inputs, vec!["foundation output"]);
    assert_eq!(expected_outputs, vec!["workflow output"]);
}

#[test]
fn checkpoint_graph_contract_round_trips_through_registry() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();

    let mut artifact = plan("checkpoint registry");
    artifact.evidence_needs.push(EvidenceNeed {
        id: "runtime-id".into(),
        question: "Which runtime id is stable?".into(),
        purpose: "Prevent a guessed binding variable.".into(),
        required: true,
        consumer: "JP2".into(),
        modes: vec![EvidenceMode::Probe],
        intent: "Observe runtime identity after JP1 before JP2 consumes it.".into(),
    });
    let (revision, hash) = approve(&registry, artifact);

    let mut graph = valid_graph();
    graph.checkpoints.push(TestCheckpointSpec {
        id: "CP1".into(),
        milestone_id: "M1".into(),
        boundary: CheckpointBoundaryKind::AfterJobpackSet,
        prerequisites: vec![CheckpointPrerequisiteSpec {
            jobpack_id: "JP1".into(),
            state: PrerequisiteState::ReviewPass,
        }],
        before_jobpack_id: None,
        evidence_need_ids: vec!["runtime-id".into()],
        modes: vec![EvidenceMode::Probe],
        goal: "Observe runtime identity".into(),
        criteria: vec!["Capture real runtime identity behavior".into()],
        required_capabilities: vec!["RUNTIME".into()],
        experiment_dimensions: vec!["session boundary".into()],
        evidence_outputs: vec![EvidenceOutputSpec {
            id: "runtime-id-observation".into(),
            mode: EvidenceMode::Probe,
            description: "Observed runtime identity behavior".into(),
            required: true,
            evidence_need_id: Some("runtime-id".into()),
        }],
    });
    graph.evidence_requirements.push(EvidenceRequirementSpec {
        consumer_jobpack_id: "JP2".into(),
        checkpoint_id: "CP1".into(),
        output_id: "runtime-id-observation".into(),
        required: true,
    });

    let version = registry
        .register_execution_graph(revision, &hash, &graph)
        .unwrap();

    assert_eq!(
        registry.execution_checkpoint_counts(version).unwrap(),
        (1, 1, 1, 1)
    );
    assert_eq!(
        registry.execution_test_checkpoints(version).unwrap(),
        graph.checkpoints
    );
    assert_eq!(
        registry.execution_evidence_requirements(version).unwrap(),
        graph.evidence_requirements
    );
}

#[test]
fn newer_approved_plan_graph_supersedes_older_graph() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();

    let (first_revision, first_hash) = approve(&registry, plan("first"));
    let first_version = registry
        .register_execution_graph(first_revision, &first_hash, &valid_graph())
        .unwrap();

    let (second_revision, second_hash) = approve(&registry, plan("second"));
    assert_eq!(
        registry
            .execution_graph_status(first_version)
            .unwrap()
            .as_deref(),
        Some("SUPERSEDED")
    );
    assert_eq!(registry.current_execution_graph_version().unwrap(), None);

    let second_version = registry
        .register_execution_graph(second_revision, &second_hash, &valid_graph())
        .unwrap();

    assert_eq!(
        registry
            .execution_graph_status(first_version)
            .unwrap()
            .as_deref(),
        Some("SUPERSEDED")
    );
    assert_eq!(
        registry
            .execution_graph_status(second_version)
            .unwrap()
            .as_deref(),
        Some("CURRENT")
    );
    assert_eq!(
        registry.current_execution_graph_version().unwrap(),
        Some(second_version)
    );
    assert!(!registry.has_active_jobpack(second_version).unwrap());
}

#[test]
fn same_approved_plan_cannot_register_twice() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let (revision, hash) = approve(&registry, plan("same"));

    registry
        .register_execution_graph(revision, &hash, &valid_graph())
        .unwrap();
    assert!(registry
        .register_execution_graph(revision, &hash, &valid_graph())
        .is_err());
}
