use gsa_local::{
    checkpoint::{Checkpoint, RecoveryClassification, ResumeAction},
    controller::{ActiveWork, MilestoneController},
    execution_graph::{
        CheckpointBoundaryKind, CheckpointPrerequisiteSpec, EvidenceOutputSpec, ExecutionGraph,
        JobPackSpec, MilestoneSpec, PrerequisiteState, TestCheckpointSpec, TodoSpec,
    },
    plan::{EvidenceMode, PlanArtifact},
    registry::{ChecklistClaim, Registry, ReviewActor, ReviewVerdict},
    tester_evidence::{TesterAttemptEvidence, TesterModeOutcome, TesterModeResult},
    verification::{
        CommandEvidence, DiscoveryStatus, VerificationCapability, VerificationCommand,
        VerificationCommandKind, VerificationEvidence, VerificationProfile,
    },
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tempfile::tempdir;

fn plan(goal: &str) -> PlanArtifact {
    PlanArtifact {
        goal: goal.into(),
        current_architecture: "Controller test runtime".into(),
        required_changes: vec!["Add controller".into()],
        implementation_approach: vec!["Use registry state".into()],
        dependencies: vec![],
        sequence: vec!["Activate".into(), "Advance".into()],
        risks: vec!["Milestone jump".into()],
        acceptance_direction: vec!["Exactly one active Job Pack".into()],
        evidence_needs: vec![],
    }
}

fn graph() -> ExecutionGraph {
    ExecutionGraph {
        milestones: vec![
            MilestoneSpec {
                id: "M1".into(),
                title: "Foundation".into(),
                order: 1,
            },
            MilestoneSpec {
                id: "M2".into(),
                title: "Next".into(),
                order: 2,
            },
        ],
        jobpacks: vec![
            JobPackSpec {
                id: "JP-A".into(),
                milestone_id: "M1".into(),
                title: "Dependent A".into(),
                goal: "Runs after C".into(),
                todo_ids: vec!["T-A".into()],
                depends_on: vec!["JP-C".into()],
                required_inputs: vec!["C output".into()],
                expected_outputs: vec!["A output".into()],
                acceptance: vec!["A done".into()],
                verification_hints: vec!["test A".into()],
            },
            JobPackSpec {
                id: "JP-B".into(),
                milestone_id: "M1".into(),
                title: "Independent B".into(),
                goal: "First eligible".into(),
                todo_ids: vec!["T-B".into()],
                depends_on: vec![],
                required_inputs: vec!["plan".into()],
                expected_outputs: vec!["B output".into()],
                acceptance: vec!["B done".into()],
                verification_hints: vec!["test B".into()],
            },
            JobPackSpec {
                id: "JP-C".into(),
                milestone_id: "M1".into(),
                title: "Independent C".into(),
                goal: "Second eligible".into(),
                todo_ids: vec!["T-C".into()],
                depends_on: vec![],
                required_inputs: vec!["plan".into()],
                expected_outputs: vec!["C output".into()],
                acceptance: vec!["C done".into()],
                verification_hints: vec!["test C".into()],
            },
            JobPackSpec {
                id: "JP-D".into(),
                milestone_id: "M2".into(),
                title: "Future D".into(),
                goal: "Only after M1".into(),
                todo_ids: vec!["T-D".into()],
                depends_on: vec!["JP-A".into()],
                required_inputs: vec!["A output".into()],
                expected_outputs: vec!["D output".into()],
                acceptance: vec!["D done".into()],
                verification_hints: vec!["test D".into()],
            },
        ],
        todos: vec![
            TodoSpec {
                id: "T-A".into(),
                jobpack_id: "JP-A".into(),
                title: "A".into(),
                checklist: vec!["implement".into()],
            },
            TodoSpec {
                id: "T-B".into(),
                jobpack_id: "JP-B".into(),
                title: "B".into(),
                checklist: vec!["implement".into()],
            },
            TodoSpec {
                id: "T-C".into(),
                jobpack_id: "JP-C".into(),
                title: "C".into(),
                checklist: vec!["implement".into()],
            },
            TodoSpec {
                id: "T-D".into(),
                jobpack_id: "JP-D".into(),
                title: "D".into(),
                checklist: vec!["implement".into()],
            },
        ],
        checkpoints: vec![],
        evidence_requirements: vec![],
    }
}

fn approve_and_register(registry: &Registry) -> (i64, String, i64) {
    approve_and_register_graph(registry, graph())
}

fn approve_and_register_graph(
    registry: &Registry,
    execution_graph: ExecutionGraph,
) -> (i64, String, i64) {
    registry.begin_plan_workflow().unwrap();
    let revision = registry.persist_plan_revision(&plan("controller")).unwrap();
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
        .register_execution_graph(revision.revision, &revision.hash, &execution_graph)
        .unwrap();
    (revision.revision, revision.hash, version)
}

fn phase11_verification() -> VerificationEvidence {
    VerificationEvidence {
        profile: VerificationProfile {
            status: DiscoveryStatus::Applicable,
            capabilities: vec![VerificationCapability::Unit],
            commands: vec![VerificationCommand {
                id: "controller-test".into(),
                kind: VerificationCommandKind::Test,
                capability: VerificationCapability::Unit,
                argv: vec!["controller-test".into()],
                source_paths: vec!["tests/controller.rs".into()],
                config_hash: "controller-config".into(),
            }],
            reason: None,
        },
        commands: vec![CommandEvidence {
            command_id: "controller-test".into(),
            config_hash: "controller-config".into(),
            argv: vec!["controller-test".into()],
            exit_code: Some(0),
            duration_ms: 1,
            timed_out: false,
            blocked_reason: None,
            stdout: "ok".into(),
            stderr: String::new(),
        }],
        test_surface_changed: false,
    }
}

fn submit_code_checkpoint(
    dir: &tempfile::TempDir,
    registry: &Registry,
    version: i64,
    jobpack_id: &str,
    todo_id: &str,
    change_set: &str,
) {
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/controller.rs"), change_set).unwrap();
    let after_sha256 = Sha256::digest(change_set.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            jobpack_id,
            change_set,
            "resume target",
            &[ChecklistClaim {
                todo_id: todo_id.into(),
                position: 1,
            }],
            &["resume exact state".into()],
            &json!([{
                "path":"src/controller.rs",
                "before_sha256":"before",
                "after_sha256":after_sha256
            }]),
            1,
            0,
        )
        .unwrap();
}

fn phase11_complete_active(
    dir: &tempfile::TempDir,
    registry: &Registry,
    version: i64,
    jobpack_id: &str,
    todo_id: &str,
    change_set: &str,
) -> Option<ActiveWork> {
    registry
        .begin_code_workflow(dir.path(), "owner-a", version, jobpack_id)
        .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/controller.rs"), change_set).unwrap();
    let after_sha256 = Sha256::digest(change_set.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            jobpack_id,
            change_set,
            "done",
            &[ChecklistClaim {
                todo_id: todo_id.into(),
                position: 1,
            }],
            &["done".into()],
            &json!([{
                "path":"src/controller.rs",
                "before_sha256":"before",
                "after_sha256":after_sha256
            }]),
            1,
            0,
        )
        .unwrap();
    registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            jobpack_id,
            change_set,
            ReviewVerdict::Pass,
            &[],
            1,
            1,
        )
        .unwrap();
    registry
        .record_verification_evidence(
            dir.path(),
            "owner-a",
            version,
            jobpack_id,
            change_set,
            &phase11_verification(),
        )
        .unwrap();
    let cr = registry.resolve_code_cr_boundary().unwrap().unwrap();
    assert!(cr.terminal);
    registry
        .record_code_cr_review(dir.path(), "owner-a", &cr.key, ReviewVerdict::Pass, &[])
        .unwrap();
    MilestoneController::new(registry, dir.path(), "owner-a")
        .mark_active_jobpack_done()
        .unwrap()
}

#[test]
fn controller_activates_deterministically_and_never_jumps_milestones() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let (_revision, _hash, version) = approve_and_register(&registry);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();

    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let first = controller.resolve_or_activate().unwrap().unwrap();

    // JP-A sorts first, but is dependency-blocked by JP-C. JP-B must win.
    assert_eq!(first.milestone_id, "M1");
    assert_eq!(first.jobpack_id, "JP-B");
    assert_eq!(registry.active_jobpack_count(version).unwrap(), 1);
    assert_eq!(
        registry.milestone_status(version, "M1").unwrap().as_deref(),
        Some("ACTIVE")
    );
    assert_eq!(
        registry.milestone_status(version, "M2").unwrap().as_deref(),
        Some("LOCKED")
    );
    assert_eq!(
        registry.jobpack_status(version, "JP-A").unwrap().as_deref(),
        Some("PENDING")
    );

    // Restart/idempotent resolve must not activate a second Job Pack.
    let again = controller.resolve_or_activate().unwrap().unwrap();
    assert_eq!(again.jobpack_id, "JP-B");
    assert_eq!(registry.active_jobpack_count(version).unwrap(), 1);

    let second =
        phase11_complete_active(&dir, &registry, version, "JP-B", "T-B", "change-b").unwrap();
    assert_eq!(second.jobpack_id, "JP-C");
    assert_eq!(
        registry.jobpack_status(version, "JP-B").unwrap().as_deref(),
        Some("DONE")
    );

    let third =
        phase11_complete_active(&dir, &registry, version, "JP-C", "T-C", "change-c").unwrap();
    assert_eq!(third.jobpack_id, "JP-A");

    let none = phase11_complete_active(&dir, &registry, version, "JP-A", "T-A", "change-a");
    assert!(none.is_none());
    assert_eq!(
        registry.milestone_status(version, "M1").unwrap().as_deref(),
        Some("VERIFY")
    );
    assert_eq!(
        registry.milestone_status(version, "M2").unwrap().as_deref(),
        Some("LOCKED")
    );
    assert_eq!(registry.active_jobpack_count(version).unwrap(), 0);

    assert!(controller
        .mark_verified_milestone_complete("M1")
        .unwrap()
        .is_none());
    assert_eq!(
        registry.milestone_status(version, "M1").unwrap().as_deref(),
        Some("COMPLETE")
    );
    assert_eq!(
        registry.milestone_status(version, "M2").unwrap().as_deref(),
        Some("LOCKED")
    );
    assert!(controller.resolve_or_activate().unwrap().is_none());
    assert_eq!(
        registry.milestone_status(version, "M2").unwrap().as_deref(),
        Some("LOCKED")
    );

    let next = controller.start_next_milestone().unwrap().unwrap();
    assert_eq!(next.milestone_id, "M2");
    assert_eq!(next.jobpack_id, "JP-D");
    assert_eq!(
        registry.milestone_status(version, "M2").unwrap().as_deref(),
        Some("ACTIVE")
    );

    assert!(phase11_complete_active(&dir, &registry, version, "JP-D", "T-D", "change-d").is_none());
    assert_eq!(
        registry.milestone_status(version, "M2").unwrap().as_deref(),
        Some("VERIFY")
    );
    assert!(controller
        .mark_verified_milestone_complete("M2")
        .unwrap()
        .is_none());
    assert_eq!(
        registry.milestone_status(version, "M2").unwrap().as_deref(),
        Some("COMPLETE")
    );
}

#[test]
fn controller_requires_exact_lease_owner_for_mutations() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    approve_and_register(&registry);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();

    let wrong = MilestoneController::new(&registry, dir.path(), "owner-b");
    assert!(wrong.resolve_or_activate().is_err());
}

#[test]
fn stale_checkpoint_is_repaired_to_current_active_work() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let (revision, _hash, _version) = approve_and_register(&registry);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let work = controller.resolve_or_activate().unwrap().unwrap();

    let current = registry.latest_checkpoint().unwrap().unwrap();
    let mut stale = Checkpoint::new("jobpack_active");
    stale.plan_revision = Some(revision);
    stale.milestone = Some("WRONG".into());
    stale.jobpack = Some("WRONG".into());
    stale.jobpack_status = Some("ACTIVE".into());
    registry
        .transition(current.sequence, "TEST_STALE_CHECKPOINT", "{}", stale)
        .unwrap();

    let repaired = controller.resolve_or_activate().unwrap().unwrap();
    assert_eq!(repaired.jobpack_id, work.jobpack_id);
    let checkpoint = registry.latest_checkpoint().unwrap().unwrap();
    assert_eq!(
        checkpoint.milestone.as_deref(),
        Some(work.milestone_id.as_str())
    );
    assert_eq!(
        checkpoint.jobpack.as_deref(),
        Some(work.jobpack_id.as_str())
    );
    assert_eq!(checkpoint.stage, "jobpack_active");
    assert_eq!(checkpoint.jobpack_status.as_deref(), Some("ACTIVE"));
}

#[test]
fn stale_plan_binding_fails_closed_instead_of_activating() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let (_revision, _hash, version) = approve_and_register(&registry);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();

    // A newer unapproved plan revision invalidates the registered graph binding.
    registry.persist_plan_revision(&plan("newer")).unwrap();

    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    assert!(controller.resolve_or_activate().is_err());
    assert_eq!(registry.active_jobpack_count(version).unwrap(), 0);
}

#[test]
fn verified_completion_cannot_skip_verify_state() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    approve_and_register(&registry);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    controller.resolve_or_activate().unwrap();

    assert!(controller.mark_verified_milestone_complete("M1").is_err());
}

#[test]
fn superseding_active_graph_retires_old_active_jobpack_before_new_activation() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let (_revision, _hash, v1) = approve_and_register(&registry);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();

    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let first = controller.resolve_or_activate().unwrap().unwrap();
    assert_eq!(first.jobpack_id, "JP-B");
    assert_eq!(registry.project_active_jobpack_count().unwrap(), 1);

    registry.begin_plan_workflow().unwrap();
    let next_plan = registry
        .persist_plan_revision(&plan("controller-v2"))
        .unwrap();
    for actor in [ReviewActor::Reviewer, ReviewActor::LocalCr] {
        registry
            .record_plan_verdict(
                actor,
                next_plan.revision,
                &next_plan.hash,
                ReviewVerdict::Pass,
                &[],
            )
            .unwrap();
    }
    registry
        .approve_current_plan(next_plan.revision, &next_plan.hash)
        .unwrap();

    assert_eq!(
        registry.jobpack_status(v1, "JP-B").unwrap().as_deref(),
        Some("BLOCKED")
    );
    assert_eq!(registry.project_active_jobpack_count().unwrap(), 0);

    let v2 = registry
        .register_execution_graph(next_plan.revision, &next_plan.hash, &graph())
        .unwrap();
    let next = controller.resolve_or_activate().unwrap().unwrap();

    assert_eq!(next.graph_version, v2);
    assert_eq!(next.jobpack_id, "JP-B");
    assert_eq!(registry.project_active_jobpack_count().unwrap(), 1);
    assert_eq!(registry.active_jobpack_count(v1).unwrap(), 0);
    assert_eq!(registry.active_jobpack_count(v2).unwrap(), 1);
}

#[test]
fn milestone_completion_requires_satisfied_milestone_gate_checkpoint() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let mut execution_graph = graph();
    execution_graph.checkpoints.push(TestCheckpointSpec {
        id: "M1-GATE".into(),
        milestone_id: "M1".into(),
        boundary: CheckpointBoundaryKind::MilestoneGate,
        prerequisites: vec![
            CheckpointPrerequisiteSpec {
                jobpack_id: "JP-A".into(),
                state: PrerequisiteState::Done,
            },
            CheckpointPrerequisiteSpec {
                jobpack_id: "JP-B".into(),
                state: PrerequisiteState::Done,
            },
            CheckpointPrerequisiteSpec {
                jobpack_id: "JP-C".into(),
                state: PrerequisiteState::Done,
            },
        ],
        before_jobpack_id: None,
        cr_review_boundary: false,
        evidence_need_ids: vec![],
        modes: vec![EvidenceMode::Verify],
        goal: "Verify M1 integration".into(),
        criteria: vec!["All M1 work integrates".into()],
        required_capabilities: vec![],
        experiment_dimensions: vec![],
        evidence_outputs: vec![EvidenceOutputSpec {
            id: "m1-integration".into(),
            mode: EvidenceMode::Verify,
            description: "Milestone integration verification".into(),
            required: true,
            evidence_need_id: None,
        }],
    });

    let (_revision, _hash, version) = approve_and_register_graph(&registry, execution_graph);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();

    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let first = controller.resolve_or_activate().unwrap().unwrap();
    assert_eq!(first.jobpack_id, "JP-B");
    assert_eq!(
        phase11_complete_active(&dir, &registry, version, "JP-B", "T-B", "change-b")
            .unwrap()
            .jobpack_id,
        "JP-C"
    );
    assert_eq!(
        phase11_complete_active(&dir, &registry, version, "JP-C", "T-C", "change-c")
            .unwrap()
            .jobpack_id,
        "JP-A"
    );
    assert!(phase11_complete_active(&dir, &registry, version, "JP-A", "T-A", "change-a").is_none());
    assert_eq!(
        registry.milestone_status(version, "M1").unwrap().as_deref(),
        Some("VERIFY")
    );

    let error = controller
        .mark_verified_milestone_complete("M1")
        .unwrap_err();
    assert!(format!("{error:#}")
        .contains("milestone checkpoint M1-GATE requires a satisfied Tester attempt"));
    assert_eq!(
        registry.milestone_status(version, "M1").unwrap().as_deref(),
        Some("VERIFY")
    );
    assert_eq!(
        registry.milestone_status(version, "M2").unwrap().as_deref(),
        Some("LOCKED")
    );
}


#[test]
fn satisfied_milestone_gate_allows_completion_but_not_implicit_next_activation() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let mut execution_graph = graph();
    execution_graph.checkpoints.push(TestCheckpointSpec {
        id: "M1-GATE-PASS".into(),
        milestone_id: "M1".into(),
        boundary: CheckpointBoundaryKind::MilestoneGate,
        prerequisites: vec![
            CheckpointPrerequisiteSpec {
                jobpack_id: "JP-A".into(),
                state: PrerequisiteState::Done,
            },
            CheckpointPrerequisiteSpec {
                jobpack_id: "JP-B".into(),
                state: PrerequisiteState::Done,
            },
            CheckpointPrerequisiteSpec {
                jobpack_id: "JP-C".into(),
                state: PrerequisiteState::Done,
            },
        ],
        before_jobpack_id: None,
        cr_review_boundary: false,
        evidence_need_ids: vec![],
        modes: vec![EvidenceMode::Verify],
        goal: "Verify M1 integration".into(),
        criteria: vec!["All M1 work integrates".into()],
        required_capabilities: vec![],
        experiment_dimensions: vec![],
        evidence_outputs: vec![],
    });

    let (_revision, _hash, version) = approve_and_register_graph(&registry, execution_graph);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();

    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    assert_eq!(controller.resolve_or_activate().unwrap().unwrap().jobpack_id, "JP-B");
    assert_eq!(
        phase11_complete_active(&dir, &registry, version, "JP-B", "T-B", "change-b")
            .unwrap()
            .jobpack_id,
        "JP-C"
    );
    assert_eq!(
        phase11_complete_active(&dir, &registry, version, "JP-C", "T-C", "change-c")
            .unwrap()
            .jobpack_id,
        "JP-A"
    );
    assert!(phase11_complete_active(&dir, &registry, version, "JP-A", "T-A", "change-a").is_none());

    let due = registry.resolve_tester_checkpoint(&[]).unwrap().unwrap();
    let attempt_id = due.next_attempt_id.clone().unwrap();
    let target = due.target.clone().unwrap();
    registry
        .record_tester_attempt_evidence(
            dir.path(),
            "owner-a",
            &TesterAttemptEvidence {
                graph_version: version,
                checkpoint_id: "M1-GATE-PASS".into(),
                attempt_id,
                target,
                mode_results: vec![TesterModeResult {
                    mode: EvidenceMode::Verify,
                    outcome: TesterModeOutcome::Pass,
                    reason: None,
                }],
                classifications: vec![],
                experiment: None,
                outputs: vec![],
                limitations: vec![],
            },
        )
        .unwrap();

    assert!(controller
        .mark_verified_milestone_complete("M1")
        .unwrap()
        .is_none());
    assert_eq!(
        registry.milestone_status(version, "M1").unwrap().as_deref(),
        Some("COMPLETE")
    );
    assert_eq!(
        registry.milestone_status(version, "M2").unwrap().as_deref(),
        Some("LOCKED")
    );
    assert!(controller.resolve_or_activate().unwrap().is_none());
    assert_eq!(
        registry.latest_checkpoint().unwrap().unwrap().stage,
        "milestone_ready_explicit_start"
    );

    let next = controller.start_next_milestone().unwrap().unwrap();
    assert_eq!(next.milestone_id, "M2");
    assert_eq!(next.jobpack_id, "JP-D");
}


#[test]
fn resume_decision_tracks_code_stages_without_reinitializing() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let (_revision, _hash, version) = approve_and_register(&registry);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let active = controller.resolve_or_activate().unwrap().unwrap();
    assert_eq!(active.jobpack_id, "JP-B");

    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP-B")
        .unwrap();
    let coder = registry.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(coder.action, ResumeAction::ResumeCoder);
    assert_eq!(coder.classification, RecoveryClassification::DurableExact);

    submit_code_checkpoint(&dir, &registry, version, "JP-B", "T-B", "resume-change");
    let reviewer = registry.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(reviewer.action, ResumeAction::ResumeReviewer);
    assert_eq!(reviewer.change_set_id.as_deref(), Some("resume-change"));

    registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP-B",
            "resume-change",
            ReviewVerdict::Revise,
            &["fix it".into()],
            1,
            1,
        )
        .unwrap();
    let fix = registry.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(fix.action, ResumeAction::ResumeInternalFix);
    assert_eq!(fix.change_set_id.as_deref(), Some("resume-change"));
}

#[test]
fn review_pass_resume_requests_verification_before_other_gates() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let (_revision, _hash, version) = approve_and_register(&registry);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    controller.resolve_or_activate().unwrap();

    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP-B")
        .unwrap();
    submit_code_checkpoint(&dir, &registry, version, "JP-B", "T-B", "review-pass-change");
    registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP-B",
            "review-pass-change",
            ReviewVerdict::Pass,
            &[],
            1,
            1,
        )
        .unwrap();

    let decision = registry.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(decision.action, ResumeAction::RunRequiredVerification);
    assert_eq!(decision.change_set_id.as_deref(), Some("review-pass-change"));
}

#[test]
fn resume_decision_fails_closed_when_review_target_source_diverged() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let (_revision, _hash, version) = approve_and_register(&registry);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    controller.resolve_or_activate().unwrap();

    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP-B")
        .unwrap();
    submit_code_checkpoint(&dir, &registry, version, "JP-B", "T-B", "stable-source");
    std::fs::write(dir.path().join("src/controller.rs"), "source-ahead").unwrap();

    let decision = registry.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(decision.action, ResumeAction::BlockedNeedsHuman);
    assert_eq!(
        decision.classification,
        RecoveryClassification::SourceDiverged
    );
    assert!(decision.reason.contains("source diverged"));
}
