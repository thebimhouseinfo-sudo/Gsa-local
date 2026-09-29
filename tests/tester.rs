use gsa_local::{
    controller::MilestoneController,
    execution_graph::{ExecutionGraph, JobPackSpec, MilestoneSpec, TodoSpec},
    plan::PlanArtifact,
    registry::{ChecklistClaim, Registry, ReviewActor, ReviewVerdict},
    tester::TesterVerdict,
    verification::{
        CommandEvidence, DiscoveryStatus, VerificationCapability, VerificationCommand,
        VerificationCommandKind, VerificationEvidence, VerificationProfile, VerificationResult,
    },
};
use serde_json::json;
use std::time::Duration;
use tempfile::tempdir;

fn plan() -> PlanArtifact {
    PlanArtifact {
        goal: "tester checkpoints".into(),
        current_architecture: "reviewed and verified change set".into(),
        required_changes: vec!["read-only Tester".into()],
        implementation_approach: vec!["immutable evidence binding".into()],
        dependencies: vec![],
        sequence: vec!["review".into(), "verify".into(), "tester".into()],
        risks: vec!["stale pass".into()],
        acceptance_direction: vec!["exact target only".into()],
    }
}

fn graph() -> ExecutionGraph {
    ExecutionGraph {
        milestones: vec![MilestoneSpec {
            id: "M1".into(),
            title: "Tester".into(),
            order: 1,
        }],
        jobpacks: vec![JobPackSpec {
            id: "JP1".into(),
            milestone_id: "M1".into(),
            title: "Tester checkpoint".into(),
            goal: "test exact reviewed work".into(),
            todo_ids: vec!["T1".into()],
            depends_on: vec![],
            required_inputs: vec!["reviewed source".into()],
            expected_outputs: vec!["tested source".into()],
            acceptance: vec!["semantic acceptance".into()],
            verification_hints: vec!["exercise regression behavior".into()],
        }],
        todos: vec![TodoSpec {
            id: "T1".into(),
            jobpack_id: "JP1".into(),
            title: "Implement".into(),
            checklist: vec!["behavior complete".into()],
        }],
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
    let active = MilestoneController::new(&registry, dir.path(), "owner-a")
        .resolve_or_activate()
        .unwrap()
        .unwrap();
    assert_eq!(active.jobpack_id, "JP1");
    (dir, registry, version)
}

fn reviewed(
    dir: &tempfile::TempDir,
    registry: &Registry,
    version: i64,
    change_set: &str,
) {
    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP1")
        .unwrap();
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            change_set,
            "implemented",
            &[ChecklistClaim {
                todo_id: "T1".into(),
                position: 1,
            }],
            &["goal met".into()],
            &json!([{
                "path":"src/lib.rs",
                "before_sha256":"before",
                "after_sha256":"after"
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
            "JP1",
            change_set,
            ReviewVerdict::Pass,
            &[],
            1,
            1,
        )
        .unwrap();
}

fn passing_evidence() -> VerificationEvidence {
    VerificationEvidence {
        profile: VerificationProfile {
            status: DiscoveryStatus::Applicable,
            capabilities: vec![
                VerificationCapability::BuildOnly,
                VerificationCapability::Integration,
            ],
            commands: vec![VerificationCommand {
                id: "cargo-test".into(),
                kind: VerificationCommandKind::Test,
                capability: VerificationCapability::Integration,
                argv: vec!["cargo".into(), "test".into()],
                source_paths: vec!["Cargo.toml".into()],
                config_hash: "config-hash".into(),
            }],
            reason: None,
        },
        commands: vec![CommandEvidence {
            command_id: "cargo-test".into(),
            config_hash: "config-hash".into(),
            argv: vec!["cargo".into(), "test".into()],
            exit_code: Some(0),
            duration_ms: 10,
            timed_out: false,
            blocked_reason: None,
            stdout: "ok".into(),
            stderr: String::new(),
        }],
        test_surface_changed: true,
    }
}

#[test]
fn tester_contract_comes_from_registered_jobpack() {
    let (_dir, registry, version) = setup();
    let (acceptance, hints) = registry
        .jobpack_test_contract(version, "JP1")
        .unwrap()
        .unwrap();
    assert_eq!(acceptance, vec!["semantic acceptance"]);
    assert_eq!(hints, vec!["exercise regression behavior"]);
}

#[test]
fn tester_is_bound_to_latest_immutable_verification_run() {
    let (dir, registry, version) = setup();
    reviewed(&dir, &registry, version, "change-1");

    let first = registry
        .record_verification_run(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            &passing_evidence(),
        )
        .unwrap();
    let second = registry
        .record_verification_run(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            &passing_evidence(),
        )
        .unwrap();
    assert!(second.id > first.id);

    assert!(registry
        .record_tester_evidence(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            first.id,
            TesterVerdict::Pass,
            &[],
            &["stale evidence".into()],
        )
        .is_err());

    let tester = registry
        .record_tester_evidence(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            second.id,
            TesterVerdict::Pass,
            &[],
            &["inspected exact acceptance behavior".into()],
        )
        .unwrap();
    assert_eq!(tester.verification_run_id, second.id);
    assert_eq!(
        registry
            .latest_tester_verdict(version, "JP1", "change-1", second.id)
            .unwrap()
            .as_deref(),
        Some("PASS")
    );
}

#[test]
fn tester_fail_reopens_exact_internal_fix_and_invalidates_claims() {
    let (dir, registry, version) = setup();
    reviewed(&dir, &registry, version, "change-1");

    assert_eq!(
        registry.checklist_checked(version, "T1", 1).unwrap(),
        Some(true)
    );
    assert_eq!(
        registry.todo_status(version, "T1").unwrap().as_deref(),
        Some("DONE")
    );

    let verification = registry
        .record_verification_run(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            &passing_evidence(),
        )
        .unwrap();
    assert_eq!(verification.result, VerificationResult::TestPass);

    registry
        .record_tester_evidence(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            verification.id,
            TesterVerdict::Fail,
            &["acceptance behavior regressed".into()],
            &["read src/lib.rs and compared acceptance".into()],
        )
        .unwrap();
    registry
        .route_post_review_failure_to_internal_fix(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            verification.id,
            "TESTER_FAIL",
            &["acceptance behavior regressed".into()],
        )
        .unwrap();

    let state = registry.code_workflow_state().unwrap().unwrap();
    assert_eq!(state.status, "INTERNAL_FIX");
    assert_eq!(state.coder_attempts, 1);
    assert_eq!(state.reviewer_attempts, 1);
    assert_eq!(
        registry.checklist_checked(version, "T1", 1).unwrap(),
        Some(false)
    );
    assert_eq!(
        registry.todo_status(version, "T1").unwrap().as_deref(),
        Some("PENDING")
    );

    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-2",
            "fixed tester finding",
            &[],
            &["tester finding addressed".into()],
            &json!([{
                "path":"src/lib.rs",
                "before_sha256":"after",
                "after_sha256":"fixed"
            }]),
            2,
            1,
        )
        .unwrap();
    assert_eq!(
        registry.code_workflow_state().unwrap().unwrap().status,
        "REVIEWER"
    );
}

#[test]
fn deterministic_verification_fail_routes_before_tester() {
    let (dir, registry, version) = setup();
    reviewed(&dir, &registry, version, "change-1");

    let mut evidence = passing_evidence();
    evidence.commands[0].exit_code = Some(1);
    let verification = registry
        .record_verification_run(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            &evidence,
        )
        .unwrap();
    assert_eq!(verification.result, VerificationResult::Fail);

    assert!(registry
        .record_tester_evidence(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            verification.id,
            TesterVerdict::Pass,
            &[],
            &["must not pass".into()],
        )
        .is_err());

    registry
        .route_post_review_failure_to_internal_fix(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            verification.id,
            "VERIFICATION_FAIL",
            &["cargo test failed".into()],
        )
        .unwrap();
    assert_eq!(
        registry.code_workflow_state().unwrap().unwrap().status,
        "INTERNAL_FIX"
    );
}
