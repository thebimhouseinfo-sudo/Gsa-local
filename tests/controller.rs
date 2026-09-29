use gsa_local::{
    checkpoint::Checkpoint,
    controller::MilestoneController,
    execution_graph::{ExecutionGraph, JobPackSpec, MilestoneSpec, TodoSpec},
    plan::PlanArtifact,
    registry::{Registry, ReviewActor, ReviewVerdict},
};
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
    }
    checkpoints: vec![],
    evidence_requirements: vec![],
}

fn approve_and_register(registry: &Registry) -> (i64, String, i64) {
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
        .register_execution_graph(revision.revision, &revision.hash, &graph())
        .unwrap();
    (revision.revision, revision.hash, version)
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

    let second = controller.mark_active_jobpack_done().unwrap().unwrap();
    assert_eq!(second.jobpack_id, "JP-C");
    assert_eq!(
        registry.jobpack_status(version, "JP-B").unwrap().as_deref(),
        Some("DONE")
    );

    let third = controller.mark_active_jobpack_done().unwrap().unwrap();
    assert_eq!(third.jobpack_id, "JP-A");

    let none = controller.mark_active_jobpack_done().unwrap();
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

    let next = controller
        .mark_verified_milestone_complete("M1")
        .unwrap()
        .unwrap();
    assert_eq!(next.milestone_id, "M2");
    assert_eq!(next.jobpack_id, "JP-D");
    assert_eq!(
        registry.milestone_status(version, "M1").unwrap().as_deref(),
        Some("COMPLETE")
    );
    assert_eq!(
        registry.milestone_status(version, "M2").unwrap().as_deref(),
        Some("ACTIVE")
    );

    assert!(controller.mark_active_jobpack_done().unwrap().is_none());
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
