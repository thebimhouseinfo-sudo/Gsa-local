use gsa_local::{
    plan::{EvidenceMode, EvidenceNeed, PlanArtifact},
    registry::{Registry, ReviewActor, ReviewVerdict},
    workflow::{PlanningRoute, PlanningStage},
};
use tempfile::tempdir;

fn sample(goal: &str) -> PlanArtifact {
    PlanArtifact {
        goal: goal.into(),
        current_architecture: "Existing GSA Local runtime".into(),
        required_changes: vec!["Add revision-bound workflow".into()],
        implementation_approach: vec!["Persist plan and verdicts in SQLite".into()],
        dependencies: vec![],
        sequence: vec!["Planner".into(), "Reviewer".into(), "Local CR".into()],
        risks: vec!["Stale verdict".into()],
        acceptance_direction: vec!["Only current Reviewer+CR PASS can approve".into()],
        evidence_needs: vec![],
    }
}

#[test]
fn evidence_need_round_trips_through_plan_revision() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    registry.begin_plan_workflow().unwrap();

    let mut artifact = sample("evidence need");
    artifact.evidence_needs.push(EvidenceNeed {
        id: "session-binding".into(),
        question: "Which runtime identity is stable within one session?".into(),
        purpose: "Prevent later code from guessing a binding key.".into(),
        required: true,
        consumer: "session controller".into(),
        modes: vec![EvidenceMode::Probe, EvidenceMode::Measure],
        intent: "Compare observed identity across controlled session boundaries.".into(),
    });

    let revision = registry.persist_plan_revision(&artifact).unwrap();
    let current = registry.current_plan_revision().unwrap().unwrap();
    assert_eq!(current.revision, revision.revision);
    assert_eq!(current.hash, artifact.hash().unwrap());
    assert_eq!(current.artifact.evidence_needs, artifact.evidence_needs);
}

#[test]
fn stale_reviewer_and_cr_verdicts_are_rejected() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    registry.begin_plan_workflow().unwrap();

    let first = registry.persist_plan_revision(&sample("first")).unwrap();
    let second = registry.persist_plan_revision(&sample("second")).unwrap();

    assert!(registry
        .record_plan_verdict(
            ReviewActor::Reviewer,
            first.revision,
            &first.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .is_err());
    assert!(registry
        .record_plan_verdict(
            ReviewActor::LocalCr,
            first.revision,
            &first.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .is_err());

    registry
        .record_plan_verdict(
            ReviewActor::Reviewer,
            second.revision,
            &second.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .unwrap();
}

#[test]
fn approval_requires_current_reviewer_and_cr_pass() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    registry.begin_plan_workflow().unwrap();
    let plan = registry.persist_plan_revision(&sample("approval")).unwrap();

    registry
        .record_plan_verdict(
            ReviewActor::Reviewer,
            plan.revision,
            &plan.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .unwrap();
    assert!(registry
        .approve_current_plan(plan.revision, &plan.hash)
        .is_err());

    registry
        .record_plan_verdict(
            ReviewActor::LocalCr,
            plan.revision,
            &plan.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .unwrap();
    let approved = registry
        .approve_current_plan(plan.revision, &plan.hash)
        .unwrap();
    assert_eq!(approved.revision, plan.revision);
    assert_eq!(approved.hash, plan.hash);
}

#[test]
fn new_plan_revision_invalidates_prior_approval_binding() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    registry.begin_plan_workflow().unwrap();
    let first = registry.persist_plan_revision(&sample("first")).unwrap();

    for actor in [ReviewActor::Reviewer, ReviewActor::LocalCr] {
        registry
            .record_plan_verdict(actor, first.revision, &first.hash, ReviewVerdict::Pass, &[])
            .unwrap();
    }
    registry
        .approve_current_plan(first.revision, &first.hash)
        .unwrap();
    assert!(registry.plan_binding().unwrap().is_some());

    let second = registry.persist_plan_revision(&sample("second")).unwrap();
    assert_ne!(second.hash, first.hash);
    assert!(registry.plan_binding().unwrap().is_none());
    assert!(registry
        .approve_current_plan(first.revision, &first.hash)
        .is_err());
}

#[test]
fn identical_plan_content_can_form_a_new_revision_without_crashing() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    registry.begin_plan_workflow().unwrap();

    let artifact = sample("same");
    let first = registry.persist_plan_revision(&artifact).unwrap();
    let second = registry.persist_plan_revision(&artifact).unwrap();

    assert_eq!(first.hash, second.hash);
    assert_eq!(second.revision, first.revision + 1);
}

#[test]
fn latest_reviewer_revise_invalidates_older_reviewer_pass() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    registry.begin_plan_workflow().unwrap();
    let plan = registry
        .persist_plan_revision(&sample("latest reviewer"))
        .unwrap();

    registry
        .record_plan_verdict(
            ReviewActor::Reviewer,
            plan.revision,
            &plan.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .unwrap();
    registry
        .record_plan_verdict(
            ReviewActor::Reviewer,
            plan.revision,
            &plan.hash,
            ReviewVerdict::Revise,
            &["new issue".into()],
        )
        .unwrap();
    registry
        .record_plan_verdict(
            ReviewActor::LocalCr,
            plan.revision,
            &plan.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .unwrap();

    assert!(registry
        .approve_current_plan(plan.revision, &plan.hash)
        .is_err());
}

#[test]
fn latest_cr_revise_invalidates_older_cr_pass() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    registry.begin_plan_workflow().unwrap();
    let plan = registry
        .persist_plan_revision(&sample("latest cr"))
        .unwrap();

    registry
        .record_plan_verdict(
            ReviewActor::Reviewer,
            plan.revision,
            &plan.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .unwrap();
    registry
        .record_plan_verdict(
            ReviewActor::LocalCr,
            plan.revision,
            &plan.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .unwrap();
    registry
        .record_plan_verdict(
            ReviewActor::LocalCr,
            plan.revision,
            &plan.hash,
            ReviewVerdict::Revise,
            &["late CR issue".into()],
        )
        .unwrap();

    assert!(registry
        .approve_current_plan(plan.revision, &plan.hash)
        .is_err());
}

#[test]
fn reviewer_revision_and_cr_fix_routes_are_enforced() {
    let mut route = PlanningRoute::new(3);
    assert!(route.enter_reviewer());
    route.reviewer_result(ReviewVerdict::Revise);
    assert_eq!(route.stage, PlanningStage::Planner);

    assert!(route.enter_reviewer());
    route.reviewer_result(ReviewVerdict::Pass);
    assert_eq!(route.stage, PlanningStage::LocalCr);

    assert!(route.enter_cr());
    route.cr_result(ReviewVerdict::Revise);
    assert_eq!(route.stage, PlanningStage::Planner);

    route.after_plan_revision();
    assert_eq!(route.stage, PlanningStage::Reviewer);
}

#[test]
fn unchanged_revision_fails_closed_to_paused() {
    let mut route = PlanningRoute::new(3);
    assert!(route.enter_reviewer());
    route.reviewer_result(ReviewVerdict::Revise);
    route.unchanged_revision();
    assert_eq!(route.stage, PlanningStage::Paused);
}

#[test]
fn repeated_plan_hash_can_exist_in_distinct_revisions_without_db_failure() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    registry.begin_plan_workflow().unwrap();

    let first = registry.persist_plan_revision(&sample("same")).unwrap();
    let second = registry.persist_plan_revision(&sample("same")).unwrap();

    assert_eq!(first.hash, second.hash);
    assert_ne!(first.revision, second.revision);
}

#[test]
fn loop_exhaustion_pauses_instead_of_approving() {
    let mut route = PlanningRoute::new(1);
    assert!(route.enter_reviewer());
    route.reviewer_result(ReviewVerdict::Revise);
    assert!(!route.enter_reviewer());
    assert_eq!(route.stage, PlanningStage::Paused);
}

#[test]
fn planning_run_start_is_idempotent_and_fails_closed_on_requirement_change() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    let first = registry
        .begin_or_resume_planning_run("Build typed planner")
        .unwrap();
    assert_eq!(first.stage, "PLANNER");
    assert_eq!(first.requirement.as_deref(), Some("Build typed planner"));
    assert_eq!(first.current_revision, None);
    assert_eq!(
        registry
            .begin_or_resume_planning_run("Build typed planner")
            .unwrap(),
        first
    );
    assert!(registry
        .begin_or_resume_planning_run("Other requirement")
        .is_err());
    assert!(registry.begin_or_resume_planning_run("  ").is_err());
    assert_eq!(registry.planning_run_state().unwrap().unwrap(), first);
}

#[test]
fn planning_run_start_never_clears_legacy_recovery_block() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    {
        let registry = Registry::open_at(&path).unwrap();
        registry.begin_plan_workflow().unwrap();
    }
    let registry = Registry::open_at(&path).unwrap();
    let blocked = registry.planning_run_state().unwrap().unwrap();
    assert_eq!(blocked.stage, "LEGACY_RECOVERY_REQUIRED");
    assert!(registry
        .begin_or_resume_planning_run("Start again")
        .is_err());
    assert_eq!(registry.planning_run_state().unwrap().unwrap(), blocked);
}

#[test]
fn durable_planning_verdict_transition_is_revision_bound() {
    let dir = tempdir().unwrap();
    let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
    registry
        .begin_or_resume_planning_run("Typed planning run")
        .unwrap();
    let first = registry.persist_plan_revision(&sample("first")).unwrap();
    let state = registry.planning_run_state().unwrap().unwrap();
    assert_eq!(state.stage, "REVIEWER");
    assert_eq!(state.current_revision, Some(first.revision));

    registry
        .record_plan_verdict(
            ReviewActor::Reviewer,
            first.revision,
            &first.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .unwrap();
    assert_eq!(
        registry.planning_run_state().unwrap().unwrap().stage,
        "LOCAL_CR"
    );
    assert!(registry
        .record_plan_verdict(
            ReviewActor::Reviewer,
            first.revision,
            &first.hash,
            ReviewVerdict::Pass,
            &[],
        )
        .is_err());
    registry
        .record_plan_verdict(
            ReviewActor::LocalCr,
            first.revision,
            &first.hash,
            ReviewVerdict::Revise,
            &[],
        )
        .unwrap();
    assert_eq!(
        registry.planning_run_state().unwrap().unwrap().stage,
        "PLANNER"
    );
    let second = registry.persist_plan_revision(&sample("second")).unwrap();
    assert_eq!(
        registry
            .planning_run_state()
            .unwrap()
            .unwrap()
            .current_revision,
        Some(second.revision)
    );
    assert_eq!(
        registry.planning_run_state().unwrap().unwrap().stage,
        "REVIEWER"
    );
}

#[test]
fn human_recovery_restarts_both_cursors_without_erasing_plan_history() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.db");
    {
        let registry = Registry::open_at(&path).unwrap();
        registry.begin_plan_workflow().unwrap();
        registry
            .persist_plan_revision(&sample("unfinished legacy plan"))
            .unwrap();
    }
    let registry = Registry::open_at(&path).unwrap();
    assert_eq!(
        registry.planning_run_state().unwrap().unwrap().stage,
        "LEGACY_RECOVERY_REQUIRED"
    );
    assert!(registry
        .confirm_legacy_planning_recovery("new exact intent", false)
        .is_err());
    assert_eq!(
        registry.planning_run_state().unwrap().unwrap().stage,
        "LEGACY_RECOVERY_REQUIRED"
    );
    let resumed = registry
        .confirm_legacy_planning_recovery("new exact intent", true)
        .unwrap();
    assert_eq!(resumed.stage, "PLANNER");
    assert_eq!(resumed.current_revision, None);
    assert_eq!(resumed.requirement.as_deref(), Some("new exact intent"));
    assert!(resumed.legacy_snapshot_json.is_some());
    let legacy = registry.workflow_state().unwrap();
    assert_eq!(legacy.current_revision, None);
    assert_eq!(legacy.reviewer_attempts, 0);
    assert_eq!(legacy.cr_attempts, 0);
    assert_eq!(legacy.status, "PLANNING");
    assert_eq!(
        registry
            .current_plan_revision()
            .unwrap()
            .unwrap()
            .artifact
            .goal,
        "unfinished legacy plan"
    );
    assert_eq!(
        registry
            .begin_or_resume_planning_run("new exact intent")
            .unwrap(),
        resumed
    );
}

#[test]
fn planner_restart_recovers_only_exact_persisted_revise_findings() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.db");
    {
        let registry = Registry::open_at(&path).unwrap();
        registry
            .begin_or_resume_planning_run("durable planner")
            .unwrap();
        let first = registry.persist_plan_revision(&sample("first")).unwrap();
        assert!(registry
            .pending_planner_revision_findings(first.revision, &first.hash)
            .is_err());
        registry
            .record_plan_verdict(
                ReviewActor::Reviewer,
                first.revision,
                &first.hash,
                ReviewVerdict::Revise,
                &["fix evidence binding".to_string()],
            )
            .unwrap();
    }
    let registry = Registry::open_at(&path).unwrap();
    let state = registry
        .begin_or_resume_planning_run("durable planner")
        .unwrap();
    assert_eq!(state.stage, "PLANNER");
    let first = registry.current_plan_revision().unwrap().unwrap();
    assert_eq!(state.current_revision, Some(first.revision));
    assert_eq!(
        registry
            .pending_planner_revision_findings(first.revision, &first.hash)
            .unwrap(),
        vec!["fix evidence binding".to_string()]
    );
    assert!(registry
        .pending_planner_revision_findings(first.revision, "stale-hash")
        .is_err());
}

#[test]
fn planning_source_evidence_checkpoint_is_immutable_and_survives_restart() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.db");
    let revision;
    {
        let registry = Registry::open_at(&path).unwrap();
        registry
            .begin_or_resume_planning_run("durable evidence")
            .unwrap();
        let plan = registry.persist_plan_revision(&sample("evidence")).unwrap();
        revision = (plan.revision, plan.hash);
        assert!(registry
            .planning_source_evidence(revision.0, &revision.1)
            .is_err());
        registry
            .checkpoint_planning_source_evidence(revision.0, &revision.1, "[]")
            .unwrap();
        registry
            .checkpoint_planning_source_evidence(revision.0, &revision.1, "[]")
            .unwrap();
        assert!(registry
            .checkpoint_planning_source_evidence(revision.0, &revision.1, "[{}]")
            .is_err());
    }
    let registry = Registry::open_at(&path).unwrap();
    assert_eq!(
        registry
            .planning_source_evidence(revision.0, &revision.1)
            .unwrap(),
        "[]"
    );
    assert!(registry
        .planning_source_evidence(revision.0, "wrong-hash")
        .is_err());
}

#[test]
fn job_builder_plan_gap_returns_to_planner_with_exact_persisted_findings() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.db");
    {
        let registry = Registry::open_at(&path).unwrap();
        registry
            .begin_or_resume_planning_run("job builder gap")
            .unwrap();
        let plan = registry.persist_plan_revision(&sample("gap")).unwrap();
        registry
            .record_plan_verdict(
                ReviewActor::Reviewer,
                plan.revision,
                &plan.hash,
                ReviewVerdict::Pass,
                &[],
            )
            .unwrap();
        registry
            .record_plan_verdict(
                ReviewActor::LocalCr,
                plan.revision,
                &plan.hash,
                ReviewVerdict::Pass,
                &[],
            )
            .unwrap();
        registry
            .set_workflow_state(Some(plan.revision), 3, 2, "JOB_BUILDER")
            .unwrap();
        assert!(registry
            .pending_planner_revision_findings(plan.revision, &plan.hash)
            .is_err());
        registry
            .record_job_builder_plan_gap(
                plan.revision,
                &plan.hash,
                &["missing decomposition".to_string()],
            )
            .unwrap();
    }
    let registry = Registry::open_at(&path).unwrap();
    let state = registry
        .begin_or_resume_planning_run("job builder gap")
        .unwrap();
    assert_eq!(state.stage, "PLANNER");
    let current = registry.current_plan_revision().unwrap().unwrap();
    let findings = registry
        .pending_planner_revision_findings(state.current_revision.unwrap(), &current.hash)
        .unwrap();
    assert_eq!(findings, vec!["missing decomposition".to_string()]);
    let revised = registry
        .persist_plan_revision(&sample("resolved gap"))
        .unwrap();
    let resumed = registry.planning_run_state().unwrap().unwrap();
    assert_eq!(resumed.stage, "REVIEWER");
    assert_eq!(resumed.current_revision, Some(revised.revision));
    assert_eq!(resumed.reviewer_attempts, 0);
    assert_eq!(resumed.cr_attempts, 0);
}
