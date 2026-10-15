use gsa_local::{
    checkpoint::{Checkpoint, RecoveryClassification, ResumeAction},
    controller::{ActiveWork, MilestoneController, NextWork},
    execution_graph::{
        CheckpointBoundaryKind, CheckpointPrerequisiteSpec, EvidenceOutputSpec,
        EvidenceRequirementSpec, ExecutionGraph, JobPackSpec, MilestoneSpec, PrerequisiteState,
        TestCheckpointSpec, TodoSpec,
    },
    plan::{EvidenceMode, PlanArtifact},
    registry::{ChecklistClaim, Registry, ReviewActor, ReviewVerdict},
    tester_evidence::{TesterAttemptEvidence, TesterModeOutcome, TesterModeResult},
    verification::{
        CommandEvidence, DiscoveryStatus, VerificationCapability, VerificationCommand,
        VerificationCommandKind, VerificationEvidence, VerificationProfile,
    },
};
use rusqlite::params;
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

    assert!(controller.resolve_next(&[]).unwrap().is_none());
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
        evidence_outputs: vec![EvidenceOutputSpec {
            id: "m1-pass-observation".into(),
            mode: EvidenceMode::Verify,
            description: "Optional milestone integration observation".into(),
            required: false,
            evidence_need_id: None,
        }],
    });

    let (_revision, _hash, version) = approve_and_register_graph(&registry, execution_graph);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();

    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    assert_eq!(
        controller
            .resolve_or_activate()
            .unwrap()
            .unwrap()
            .jobpack_id,
        "JP-B"
    );
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

    let reopened = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let restart = reopened.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(restart.action, ResumeAction::WaitExplicitNextMilestoneStart);
    assert_eq!(
        registry.milestone_status(version, "M2").unwrap().as_deref(),
        Some("LOCKED")
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
    let reopened_coder = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let coder = reopened_coder
        .resolve_resume_decision(dir.path(), &[])
        .unwrap();
    assert_eq!(coder.action, ResumeAction::ResumeCoder);
    assert_eq!(coder.classification, RecoveryClassification::DurableExact);

    submit_code_checkpoint(&dir, &registry, version, "JP-B", "T-B", "resume-change");
    let reopened_reviewer = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let reviewer = reopened_reviewer
        .resolve_resume_decision(dir.path(), &[])
        .unwrap();
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
    let reopened_fix = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let fix = reopened_fix
        .resolve_resume_decision(dir.path(), &[])
        .unwrap();
    assert_eq!(fix.action, ResumeAction::ResumeInternalFix);
    assert_eq!(fix.change_set_id.as_deref(), Some("resume-change"));
}

#[test]
fn paused_code_workflow_remains_blocked_after_registry_reopen() {
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
    submit_code_checkpoint(&dir, &registry, version, "JP-B", "T-B", "paused-change");
    registry
        .pause_code_workflow(
            dir.path(),
            "owner-a",
            version,
            "JP-B",
            1,
            0,
            "wait for explicit recovery",
        )
        .unwrap();

    let reopened = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let decision = reopened.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(decision.action, ResumeAction::BlockedNeedsHuman);
    assert_eq!(decision.classification, RecoveryClassification::Blocked);
    assert_eq!(decision.change_set_id.as_deref(), Some("paused-change"));
}

#[test]
fn reviewer_resume_decision_survives_registry_reopen() {
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
    submit_code_checkpoint(&dir, &registry, version, "JP-B", "T-B", "restart-reviewer");

    let reopened = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let decision = reopened.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(decision.action, ResumeAction::ResumeReviewer);
    assert_eq!(
        decision.classification,
        RecoveryClassification::DurableExact
    );
    assert_eq!(decision.change_set_id.as_deref(), Some("restart-reviewer"));
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
    submit_code_checkpoint(
        &dir,
        &registry,
        version,
        "JP-B",
        "T-B",
        "review-pass-change",
    );
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

    let reopened = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let decision = reopened.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(decision.action, ResumeAction::RunRequiredVerification);
    assert_eq!(
        decision.classification,
        RecoveryClassification::DurableExact
    );
    assert_eq!(
        decision.change_set_id.as_deref(),
        Some("review-pass-change")
    );
}

#[test]
fn resume_decision_fails_closed_when_review_target_source_is_ahead() {
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

    let reopened = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let decision = reopened.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(decision.action, ResumeAction::BlockedNeedsHuman);
    assert_eq!(decision.classification, RecoveryClassification::SourceAhead);
    assert!(decision.reason.contains("automatic adoption is prohibited"));

    let error = controller.resolve_next(&[]).unwrap_err();
    assert!(format!("{error:#}").contains("automatic adoption is prohibited"));
}

#[test]
fn resume_decision_distinguishes_source_stale_from_source_ahead() {
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

    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let before = "before-state";
    let after = "after-state";
    let before_sha256 = Sha256::digest(before.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let after_sha256 = Sha256::digest(after.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    std::fs::write(dir.path().join("src/controller.rs"), after).unwrap();

    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP-B",
            "stale-change",
            "resume target",
            &[ChecklistClaim {
                todo_id: "T-B".into(),
                position: 1,
            }],
            &["resume exact state".into()],
            &json!([{
                "path":"src/controller.rs",
                "before_sha256":before_sha256,
                "after_sha256":after_sha256
            }]),
            1,
            0,
        )
        .unwrap();

    std::fs::write(dir.path().join("src/controller.rs"), before).unwrap();

    let reopened = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let decision = reopened.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(decision.action, ResumeAction::BlockedNeedsHuman);
    assert_eq!(decision.classification, RecoveryClassification::SourceStale);
    assert!(decision.reason.contains("matches durable before-state"));
}

#[test]
fn resume_decision_classifies_missing_reviewed_source_as_diverged() {
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
    std::fs::remove_file(dir.path().join("src/controller.rs")).unwrap();

    let reopened = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let decision = reopened.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(decision.action, ResumeAction::BlockedNeedsHuman);
    assert_eq!(
        decision.classification,
        RecoveryClassification::SourceDiverged
    );
    assert!(decision.reason.contains("source path unavailable"));
}

#[test]
fn terminal_cr_commits_j1_before_required_tester_evidence_unlocks_j2() {
    // Reproduces the real Stock-QA failure: CR PASS for J1 was followed by
    // PLAN_GAP while trying to load J2 before the after-J1 Tester ran.
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let mut execution_graph = graph();
    execution_graph.milestones.retain(|m| m.id == "M1");
    execution_graph
        .jobpacks
        .retain(|pack| pack.id == "JP-B" || pack.id == "JP-C");
    execution_graph
        .todos
        .retain(|todo| todo.jobpack_id == "JP-B" || todo.jobpack_id == "JP-C");
    let successor = execution_graph
        .jobpacks
        .iter_mut()
        .find(|pack| pack.id == "JP-C")
        .unwrap();
    successor.depends_on = vec!["JP-B".into()];
    execution_graph.checkpoints = vec![TestCheckpointSpec {
        id: "chk-j1-verify".into(),
        milestone_id: "M1".into(),
        boundary: CheckpointBoundaryKind::AfterJobpackSet,
        prerequisites: vec![CheckpointPrerequisiteSpec {
            jobpack_id: "JP-B".into(),
            state: PrerequisiteState::Done,
        }],
        before_jobpack_id: None,
        cr_review_boundary: false,
        evidence_need_ids: vec![],
        modes: vec![EvidenceMode::Verify],
        goal: "Verify J1 after CR PASS before starting J2".into(),
        criteria: vec!["Observe actual J1 verification evidence".into()],
        required_capabilities: vec![],
        experiment_dimensions: vec![],
        evidence_outputs: vec![EvidenceOutputSpec {
            id: "ev-j1-verify".into(),
            mode: EvidenceMode::Verify,
            description: "Observed J1 verification".into(),
            required: true,
            evidence_need_id: None,
        }],
    }];
    execution_graph.evidence_requirements = vec![EvidenceRequirementSpec {
        consumer_jobpack_id: "JP-C".into(),
        checkpoint_id: "chk-j1-verify".into(),
        output_id: "ev-j1-verify".into(),
        required: true,
    }];
    let (_, _, version) = approve_and_register_graph(&registry, execution_graph);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();

    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    assert_eq!(
        controller
            .resolve_or_activate()
            .unwrap()
            .unwrap()
            .jobpack_id,
        "JP-B"
    );
    let next = phase11_complete_active(
        &dir,
        &registry,
        version,
        "JP-B",
        "T-B",
        "j1-reviewed-source",
    );
    // DONE must commit rather than rolling back because J2 evidence is absent.
    assert!(
        next.is_none(),
        "J2 must not activate before observed Tester evidence"
    );
    assert_eq!(
        registry.jobpack_status(version, "JP-B").unwrap().as_deref(),
        Some("DONE")
    );
    assert_eq!(
        registry.jobpack_status(version, "JP-C").unwrap().as_deref(),
        Some("PENDING")
    );
    let resumed = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let decision = resumed.resolve_resume_decision(dir.path(), &[]).unwrap();
    assert_eq!(decision.action, ResumeAction::ResumeTesterAttempt);
    assert_eq!(decision.checkpoint_id.as_deref(), Some("chk-j1-verify"));
    match controller.resolve_next(&[]).unwrap() {
        Some(NextWork::Tester(work)) => {
            assert_eq!(work.checkpoint.id, "chk-j1-verify");
        }
        other => panic!("expected Tester before J2, found {other:?}"),
    }
}

#[test]
fn terminal_done_survives_missing_evidence_at_not_yet_reached_checkpoint() {
    // Negative control: a milestone-gate output cannot become available before
    // J2 in that same milestone. J1 DONE must still commit instead of rollback.
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let mut execution_graph = graph();
    execution_graph.milestones.retain(|m| m.id == "M1");
    execution_graph
        .jobpacks
        .retain(|pack| pack.id == "JP-B" || pack.id == "JP-C");
    execution_graph
        .todos
        .retain(|todo| todo.jobpack_id == "JP-B" || todo.jobpack_id == "JP-C");
    let successor = execution_graph
        .jobpacks
        .iter_mut()
        .find(|pack| pack.id == "JP-C")
        .unwrap();
    successor.depends_on = vec!["JP-B".into()];
    execution_graph.checkpoints = vec![TestCheckpointSpec {
        id: "chk-j1-verify".into(),
        milestone_id: "M1".into(),
        boundary: CheckpointBoundaryKind::MilestoneGate,
        prerequisites: vec![CheckpointPrerequisiteSpec {
            jobpack_id: "JP-B".into(),
            state: PrerequisiteState::Done,
        }],
        before_jobpack_id: None,
        cr_review_boundary: false,
        evidence_need_ids: vec![],
        modes: vec![EvidenceMode::Verify],
        goal: "Verify J1 after CR PASS before starting J2".into(),
        criteria: vec!["Observe actual J1 verification evidence".into()],
        required_capabilities: vec![],
        experiment_dimensions: vec![],
        evidence_outputs: vec![EvidenceOutputSpec {
            id: "ev-j1-verify".into(),
            mode: EvidenceMode::Verify,
            description: "Observed J1 verification".into(),
            required: true,
            evidence_need_id: None,
        }],
    }];
    execution_graph.evidence_requirements = vec![EvidenceRequirementSpec {
        consumer_jobpack_id: "JP-C".into(),
        checkpoint_id: "chk-j1-verify".into(),
        output_id: "ev-j1-verify".into(),
        required: true,
    }];
    // The invalid graph must now be rejected before it can mutate Registry state.
    let error = execution_graph.validate().unwrap_err().to_string();
    assert!(error.contains("GRAPH_UNSATISFIABLE_EVIDENCE_ORDER"));
}

#[test]
fn future_evidence_requirement_does_not_block_independent_eligible_jobpack() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let mut execution_graph = graph();
    execution_graph.jobpacks[0].depends_on.clear();
    execution_graph.checkpoints = vec![TestCheckpointSpec {
        id: "later-milestone-gate".into(),
        milestone_id: "M1".into(),
        boundary: CheckpointBoundaryKind::AfterJobpackSet,
        prerequisites: vec![CheckpointPrerequisiteSpec {
            jobpack_id: "JP-C".into(),
            state: PrerequisiteState::Done,
        }],
        before_jobpack_id: None,
        cr_review_boundary: false,
        evidence_need_ids: vec![],
        modes: vec![EvidenceMode::Verify],
        goal: "Unrelated future evidence".into(),
        criteria: vec!["Run milestone verification".into()],
        required_capabilities: vec![],
        experiment_dimensions: vec![],
        evidence_outputs: vec![EvidenceOutputSpec {
            id: "later-output".into(),
            mode: EvidenceMode::Verify,
            description: "Later evidence".into(),
            required: true,
            evidence_need_id: None,
        }],
    }];
    execution_graph.evidence_requirements = vec![EvidenceRequirementSpec {
        consumer_jobpack_id: "JP-A".into(),
        checkpoint_id: "later-milestone-gate".into(),
        output_id: "later-output".into(),
        required: true,
    }];
    let (_, _, version) = approve_and_register_graph(&registry, execution_graph);
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    assert_eq!(
        controller
            .resolve_or_activate()
            .unwrap()
            .unwrap()
            .jobpack_id,
        "JP-B"
    );
    let next = phase11_complete_active(
        &dir,
        &registry,
        version,
        "JP-B",
        "T-B",
        "j1-unrelated-evidence",
    )
    .unwrap();
    assert_eq!(next.jobpack_id, "JP-C");
    assert_eq!(
        registry.jobpack_status(version, "JP-B").unwrap().as_deref(),
        Some("DONE")
    );
}

#[test]
fn legacy_registered_stock_qa_graph_repair_preview_is_read_only() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("state.db");
    let registry = Registry::open_at(&db_path).unwrap();
    let mut spec = graph();
    for milestone in &mut spec.milestones {
        if milestone.id == "M1" {
            milestone.id = "m1-reservation-correctness".into();
        }
    }
    for pack in &mut spec.jobpacks {
        pack.milestone_id = pack
            .milestone_id
            .replace("M1", "m1-reservation-correctness");
        pack.id = pack
            .id
            .replace("JP-B", "jp-j1-fefo-atomicity")
            .replace("JP-C", "jp-j2-idempotency");
        for dependency in &mut pack.depends_on {
            *dependency = dependency
                .replace("JP-B", "jp-j1-fefo-atomicity")
                .replace("JP-C", "jp-j2-idempotency");
        }
    }
    for todo in &mut spec.todos {
        todo.jobpack_id = todo
            .jobpack_id
            .replace("JP-B", "jp-j1-fefo-atomicity")
            .replace("JP-C", "jp-j2-idempotency");
    }
    spec.checkpoints = vec![TestCheckpointSpec {
        id: "chk-j1-verify".into(),
        milestone_id: "m1-reservation-correctness".into(),
        boundary: CheckpointBoundaryKind::AfterJobpackSet,
        prerequisites: vec![CheckpointPrerequisiteSpec {
            jobpack_id: "jp-j1-fefo-atomicity".into(),
            state: PrerequisiteState::Done,
        }],
        before_jobpack_id: None,
        cr_review_boundary: false,
        evidence_need_ids: vec![],
        modes: vec![EvidenceMode::Verify],
        goal: "Verify J1 ordering".into(),
        criteria: vec!["Observed J1 verification".into()],
        required_capabilities: vec![],
        experiment_dimensions: vec![],
        evidence_outputs: vec![EvidenceOutputSpec {
            id: "ev-j1-verify".into(),
            mode: EvidenceMode::Verify,
            description: "Observed J1 evidence".into(),
            required: true,
            evidence_need_id: None,
        }],
    }];
    spec.evidence_requirements = vec![EvidenceRequirementSpec {
        consumer_jobpack_id: "jp-j2-idempotency".into(),
        checkpoint_id: "chk-j1-verify".into(),
        output_id: "ev-j1-verify".into(),
        required: true,
    }];
    let (_, _, version) = approve_and_register_graph(&registry, spec.clone());
    let mut legacy = spec.checkpoints[0].clone();
    legacy.boundary = CheckpointBoundaryKind::MilestoneGate;
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "UPDATE execution_test_checkpoints SET boundary='MILESTONE_GATE', definition_json=?1 WHERE graph_version=?2 AND checkpoint_id='chk-j1-verify'",
        params![serde_json::to_string(&legacy).unwrap(), version],
    ).unwrap();
    let preview = registry.preview_stock_qa_graph_repair().unwrap();
    assert_eq!(preview.graph_version, version);
    assert_ne!(preview.before_hash, preview.after_hash);
    assert!(preview.original_checkpoint_json.contains("MILESTONE_GATE"));
    assert!(preview
        .replacement_checkpoint_json
        .contains("AFTER_JOBPACK_SET"));
    let preserved: String = conn.query_row(
        "SELECT boundary FROM execution_test_checkpoints WHERE graph_version=?1 AND checkpoint_id='chk-j1-verify'",
        params![version],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(preserved, "MILESTONE_GATE");
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();
    // Approval text alone cannot substitute for independent REVIEWER/LOCAL_CR PASS.
    let exact_hash = gsa_local::graph_contract::amendment_fingerprint(&preview).unwrap();
    let unreviewed = registry.apply_stock_qa_graph_repair(
        dir.path(),
        "owner-a",
        &exact_hash,
        "unchanged-source",
        &format!("APPROVE GRAPH REPAIR {exact_hash}"),
    );
    assert!(unreviewed
        .unwrap_err()
        .to_string()
        .contains("GRAPH_REPAIR_BLOCKED"));
    let amendments: i64 = conn.query_row(
        "SELECT COUNT(*) FROM graph_contract_versions WHERE graph_version=?1 AND contract_revision>0",
        params![version],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(amendments, 0);
    let error = registry
        .apply_stock_qa_graph_repair(
            dir.path(),
            "owner-a",
            "unapproved",
            "unknown",
            "APPROVE GRAPH REPAIR unapproved",
        )
        .unwrap_err();
    assert!(error.to_string().contains("GRAPH_REPAIR_BLOCKED"));
    let still_legacy: String = conn.query_row(
        "SELECT boundary FROM execution_test_checkpoints WHERE graph_version=?1 AND checkpoint_id='chk-j1-verify'",
        params![version],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(still_legacy, "MILESTONE_GATE");

    // Emulate a genuinely pre-amendment graph v1 (no contract revision table rows).
    // The original source, review and verification records are created normally.
    conn.execute(
        "DELETE FROM graph_contract_versions WHERE graph_version=?1",
        params![version],
    )
    .unwrap();
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let work = controller.resolve_or_activate().unwrap().unwrap();
    assert_eq!(work.jobpack_id, "jp-j1-fefo-atomicity");
    let change_set = "j1-test-exact-source";
    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "jp-j1-fefo-atomicity")
        .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/controller.rs"), change_set).unwrap();
    let after_sha256 = Sha256::digest(change_set.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    registry.record_code_checkpoint(
        dir.path(), "owner-a", version, "jp-j1-fefo-atomicity", change_set,
        "J1 done", &[ChecklistClaim { todo_id: "T-B".into(), position: 1 }],
        &["done".into()],
        &json!([{"path":"src/controller.rs","before_sha256":"before","after_sha256":after_sha256}]),
        1, 0,
    ).unwrap();
    registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "jp-j1-fefo-atomicity",
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
            "jp-j1-fefo-atomicity",
            change_set,
            &phase11_verification(),
        )
        .unwrap();
    let cr_before = registry.resolve_code_cr_boundary().unwrap().unwrap();
    registry
        .record_code_cr_review(
            dir.path(),
            "owner-a",
            &cr_before.key,
            ReviewVerdict::Pass,
            &[],
        )
        .unwrap();
    let fingerprint = gsa_local::graph_contract::amendment_fingerprint(&preview).unwrap();
    registry
        .stage_stock_qa_graph_repair(dir.path(), "owner-a", &fingerprint)
        .unwrap();
    assert_eq!(
        registry.pending_graph_repair_hash().unwrap().as_deref(),
        Some(fingerprint.as_str())
    );
    registry
        .record_graph_contract_review(
            ReviewActor::Reviewer,
            ReviewVerdict::Pass,
            &fingerprint,
            &[],
            "model-reviewer",
            "review-run-1",
            "response-hash-1",
        )
        .unwrap();
    registry
        .record_graph_contract_review(
            ReviewActor::LocalCr,
            ReviewVerdict::Pass,
            &fingerprint,
            &[],
            "model-cr",
            "review-run-2",
            "response-hash-2",
        )
        .unwrap();
    let approved = format!("APPROVE GRAPH REPAIR {fingerprint}");
    let applied = registry
        .apply_stock_qa_graph_repair(dir.path(), "owner-a", &fingerprint, change_set, &approved)
        .unwrap();
    assert_eq!(applied.after_hash, preview.after_hash);
    assert_eq!(
        registry
            .code_workflow_state()
            .unwrap()
            .unwrap()
            .change_set_id
            .as_deref(),
        Some(change_set)
    );
    assert_eq!(
        registry
            .jobpack_status(version, "jp-j1-fefo-atomicity")
            .unwrap()
            .as_deref(),
        Some("ACTIVE")
    );
    assert_eq!(
        registry
            .jobpack_status(version, "jp-j2-idempotency")
            .unwrap()
            .as_deref(),
        Some("PENDING")
    );
    let after = registry.resolve_code_cr_boundary().unwrap().unwrap();
    assert_eq!(cr_before.key, after.key);
    let applied_again = registry
        .apply_stock_qa_graph_repair(dir.path(), "owner-a", &fingerprint, change_set, &approved)
        .unwrap();
    assert_eq!(applied, applied_again);
    assert!(registry.pending_graph_repair_hash().unwrap().is_none());
    let rows: i64 = conn.query_row(
        "SELECT COUNT(*) FROM graph_contract_versions WHERE graph_version=?1 AND contract_revision=1",
        params![version], |row| row.get(0),
    ).unwrap();
    assert_eq!(rows, 1);
    let event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind='GRAPH_CONTRACT_AMENDED'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(event_count, 1);
    // Lease fencing survives even an exact already-applied replay.
    let wrong_owner = registry.apply_stock_qa_graph_repair(
        dir.path(),
        "other-owner",
        &fingerprint,
        change_set,
        &approved,
    );
    assert!(wrong_owner.is_err());
    let wrong_proposal = registry.apply_stock_qa_graph_repair(
        dir.path(),
        "owner-a",
        "outdated-proposal",
        change_set,
        "APPROVE GRAPH REPAIR outdated-proposal",
    );
    assert!(wrong_proposal.is_err());
    // Reopening SQLite must not replay the mutation or lose the old J1 gate.
    let resumed = Registry::open_at(&db_path).unwrap();
    assert_eq!(
        resumed.current_execution_graph_version().unwrap(),
        Some(version)
    );
    assert_eq!(
        resumed
            .code_workflow_state()
            .unwrap()
            .unwrap()
            .change_set_id
            .as_deref(),
        Some(change_set)
    );
    let saved_boundary: String = conn.query_row(
        "SELECT boundary FROM execution_test_checkpoints WHERE graph_version=?1 AND checkpoint_id='chk-j1-verify'",
        params![version],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(saved_boundary, "AFTER_JOBPACK_SET");
    let replay_events: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind='GRAPH_CONTRACT_AMENDED'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(replay_events, 1);
    // A corrected boundary must allow J1 to terminalize without prematurely
    // activating the J2 consumer while Tester output is still missing.
    let next = controller.mark_active_jobpack_done().unwrap();
    assert!(next.is_none());
    assert_eq!(
        registry
            .jobpack_status(version, "jp-j1-fefo-atomicity")
            .unwrap()
            .as_deref(),
        Some("DONE")
    );
    assert_eq!(
        registry
            .jobpack_status(version, "jp-j2-idempotency")
            .unwrap()
            .as_deref(),
        Some("PENDING")
    );
}
