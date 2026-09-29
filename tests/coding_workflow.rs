use gsa_local::{
    controller::MilestoneController,
    execution_graph::{ExecutionGraph, JobPackSpec, MilestoneSpec, TodoSpec},
    plan::PlanArtifact,
    registry::{ChecklistClaim, Registry, ReviewActor, ReviewVerdict},
};
use serde_json::json;
use std::time::Duration;
use tempfile::tempdir;

fn plan() -> PlanArtifact {
    PlanArtifact {
        goal: "coding workflow".into(),
        current_architecture: "active jobpack runtime".into(),
        required_changes: vec!["structured coder review".into()],
        implementation_approach: vec!["registry-bound workflow".into()],
        dependencies: vec![],
        sequence: vec!["code".into(), "review".into()],
        risks: vec!["stale target".into()],
        acceptance_direction: vec!["review gate only".into()],
    }
}

fn graph() -> ExecutionGraph {
    ExecutionGraph {
        milestones: vec![MilestoneSpec {
            id: "M1".into(),
            title: "Coding".into(),
            order: 1,
        }],
        jobpacks: vec![
            JobPackSpec {
                id: "JP1".into(),
                milestone_id: "M1".into(),
                title: "Primary".into(),
                goal: "Implement primary work".into(),
                todo_ids: vec!["T1".into()],
                depends_on: vec![],
                required_inputs: vec!["plan".into()],
                expected_outputs: vec!["source".into()],
                acceptance: vec!["works".into()],
                verification_hints: vec!["later CI".into()],
            },
            JobPackSpec {
                id: "JP2".into(),
                milestone_id: "M1".into(),
                title: "Later".into(),
                goal: "Later work".into(),
                todo_ids: vec!["T2".into()],
                depends_on: vec!["JP1".into()],
                required_inputs: vec!["JP1 output".into()],
                expected_outputs: vec!["later source".into()],
                acceptance: vec!["later works".into()],
                verification_hints: vec!["later CI".into()],
            },
        ],
        todos: vec![
            TodoSpec {
                id: "T1".into(),
                jobpack_id: "JP1".into(),
                title: "Primary todo".into(),
                checklist: vec!["edit source".into(), "cover acceptance".into()],
            },
            TodoSpec {
                id: "T2".into(),
                jobpack_id: "JP2".into(),
                title: "Foreign todo".into(),
                checklist: vec!["later item".into()],
            },
        ],
    }
}

fn setup() -> (tempfile::TempDir, Registry, i64) {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
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
        .register_execution_graph(revision.revision, &revision.hash, &graph())
        .unwrap();
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let active = controller.resolve_or_activate().unwrap().unwrap();
    assert_eq!(active.jobpack_id, "JP1");
    (dir, registry, version)
}

fn journal() -> serde_json::Value {
    json!([{
        "path": "src/example.rs",
        "before_sha256": "before",
        "after_sha256": "after"
    }])
}
#[test]
fn active_jobpack_tasks_are_scoped_and_ordered() {
    let (_dir, registry, version) = setup();
    let tasks = registry.jobpack_code_tasks(version, "JP1").unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].todo_id, "T1");
    assert_eq!(tasks[0].status, "PENDING");
    assert_eq!(tasks[0].checklist.len(), 2);
    assert_eq!(tasks[0].checklist[0].position, 1);
    assert!(!tasks[0].checklist[0].checked);
}
#[test]
fn checklist_claims_remain_deferred_until_exact_reviewer_pass() {
    let (dir, registry, version) = setup();
    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP1")
        .unwrap();

    let claims = vec![
        ChecklistClaim {
            todo_id: "T1".into(),
            position: 1,
        },
        ChecklistClaim {
            todo_id: "T1".into(),
            position: 2,
        },
    ];
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            "implemented",
            &claims,
            &["criteria checked".into()],
            &journal(),
            1,
            0,
        )
        .unwrap();

    assert_eq!(
        registry.checklist_checked(version, "T1", 1).unwrap(),
        Some(false)
    );
    assert_eq!(
        registry.todo_status(version, "T1").unwrap().as_deref(),
        Some("PENDING")
    );

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

    assert_eq!(
        registry.checklist_checked(version, "T1", 1).unwrap(),
        Some(true)
    );
    assert_eq!(
        registry.checklist_checked(version, "T1", 2).unwrap(),
        Some(true)
    );
    assert_eq!(
        registry.todo_status(version, "T1").unwrap().as_deref(),
        Some("DONE")
    );
    assert_eq!(
        registry.jobpack_status(version, "JP1").unwrap().as_deref(),
        Some("ACTIVE")
    );
    assert_eq!(
        registry.code_workflow_state().unwrap().unwrap().status,
        "REVIEW_PASS"
    );
}
#[test]
fn reviewer_revise_does_not_commit_completion_claims() {
    let (dir, registry, version) = setup();
    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP1")
        .unwrap();

    let claims = vec![ChecklistClaim {
        todo_id: "T1".into(),
        position: 1,
    }];
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-r",
            "needs review",
            &claims,
            &[],
            &journal(),
            1,
            0,
        )
        .unwrap();
    registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-r",
            ReviewVerdict::Revise,
            &["fix issue".into()],
            1,
            1,
        )
        .unwrap();

    assert_eq!(
        registry.checklist_checked(version, "T1", 1).unwrap(),
        Some(false)
    );
    assert_eq!(
        registry
            .latest_code_review_verdict(version, "JP1", "change-r")
            .unwrap()
            .as_deref(),
        Some("REVISE")
    );
    assert_eq!(
        registry.code_workflow_state().unwrap().unwrap().status,
        "INTERNAL_FIX"
    );
}
#[test]
fn foreign_checklist_claim_and_stale_jobpack_are_rejected() {
    let (dir, registry, version) = setup();
    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP1")
        .unwrap();

    let foreign = vec![ChecklistClaim {
        todo_id: "T2".into(),
        position: 1,
    }];
    assert!(registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-bad",
            "invalid claim",
            &foreign,
            &[],
            &journal(),
            1,
            0,
        )
        .is_err());

    assert!(registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP2")
        .is_err());
}
#[test]
fn review_verdict_is_bound_to_exact_change_set() {
    let (dir, registry, version) = setup();
    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP1")
        .unwrap();
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-current",
            "implemented",
            &[],
            &[],
            &journal(),
            1,
            0,
        )
        .unwrap();

    assert!(registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-stale",
            ReviewVerdict::Pass,
            &[],
            1,
            1,
        )
        .is_err());
    assert_eq!(
        registry
            .latest_code_review_verdict(version, "JP1", "change-stale")
            .unwrap(),
        None
    );
}
#[test]
fn later_revise_is_latest_for_the_same_exact_target() {
    let (dir, registry, version) = setup();
    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP1")
        .unwrap();
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-same",
            "first checkpoint",
            &[],
            &[],
            &journal(),
            1,
            0,
        )
        .unwrap();
    registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-same",
            ReviewVerdict::Revise,
            &["first finding".into()],
            1,
            1,
        )
        .unwrap();

    // Internal Fix resubmits the same runtime target identity in this synthetic
    // Registry test; real source changes produce a different change_set_id.
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-same",
            "repair checkpoint",
            &[],
            &[],
            &journal(),
            2,
            1,
        )
        .unwrap();
    registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-same",
            ReviewVerdict::Pass,
            &[],
            2,
            2,
        )
        .unwrap();

    assert_eq!(
        registry
            .latest_code_review_verdict(version, "JP1", "change-same")
            .unwrap()
            .as_deref(),
        Some("PASS")
    );
}
#[test]
fn code_state_rejects_out_of_order_checkpoint_and_review_attempts() {
    let (dir, registry, version) = setup();
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
            "first checkpoint",
            &[],
            &[],
            &journal(),
            1,
            0,
        )
        .unwrap();

    assert!(registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-duplicate",
            "duplicate checkpoint",
            &[],
            &[],
            &journal(),
            2,
            0,
        )
        .is_err());

    assert!(registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            ReviewVerdict::Pass,
            &[],
            1,
            2,
        )
        .is_err());

    registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            ReviewVerdict::Revise,
            &["fix it".into()],
            1,
            1,
        )
        .unwrap();

    assert!(registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            ReviewVerdict::Pass,
            &[],
            1,
            2,
        )
        .is_err());

    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-2",
            "fixed checkpoint",
            &[],
            &[],
            &journal(),
            2,
            1,
        )
        .unwrap();
    registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-2",
            ReviewVerdict::Pass,
            &[],
            2,
            2,
        )
        .unwrap();

    assert!(registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-after-pass",
            "must be rejected",
            &[],
            &[],
            &journal(),
            3,
            2,
        )
        .is_err());
}

#[test]
fn new_code_workflow_invalidates_prior_checklist_completion() {
    let (dir, registry, version) = setup();
    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP1")
        .unwrap();
    let claims = vec![
        ChecklistClaim {
            todo_id: "T1".into(),
            position: 1,
        },
        ChecklistClaim {
            todo_id: "T1".into(),
            position: 2,
        },
    ];
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-pass",
            "first reviewed implementation",
            &claims,
            &[],
            &journal(),
            1,
            0,
        )
        .unwrap();
    registry
        .record_code_review(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-pass",
            ReviewVerdict::Pass,
            &[],
            1,
            1,
        )
        .unwrap();
    assert_eq!(
        registry.todo_status(version, "T1").unwrap().as_deref(),
        Some("DONE")
    );

    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP1")
        .unwrap();

    assert_eq!(
        registry.checklist_checked(version, "T1", 1).unwrap(),
        Some(false)
    );
    assert_eq!(
        registry.checklist_checked(version, "T1", 2).unwrap(),
        Some(false)
    );
    assert_eq!(
        registry.todo_status(version, "T1").unwrap().as_deref(),
        Some("PENDING")
    );
    assert_eq!(
        registry.jobpack_status(version, "JP1").unwrap().as_deref(),
        Some("ACTIVE")
    );
}
