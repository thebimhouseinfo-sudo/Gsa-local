use gsa_local::{
    controller::MilestoneController,
    execution_graph::{ExecutionGraph, JobPackSpec, MilestoneSpec, TodoSpec},
    plan::PlanArtifact,
    registry::{Registry, ReviewActor, ReviewVerdict},
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
        goal: "verification".into(),
        current_architecture: "reviewed change set".into(),
        required_changes: vec!["deterministic local CI".into()],
        implementation_approach: vec!["runtime evidence".into()],
        dependencies: vec![],
        sequence: vec!["review".into(), "verify".into()],
        risks: vec!["fake pass".into()],
        acceptance_direction: vec!["observed evidence only".into()],
        evidence_needs: vec![],
    }
}

fn graph() -> ExecutionGraph {
    ExecutionGraph {
        milestones: vec![MilestoneSpec {
            id: "M1".into(),
            title: "Verification".into(),
            order: 1,
        }],
        jobpacks: vec![JobPackSpec {
            id: "JP1".into(),
            milestone_id: "M1".into(),
            title: "Verify".into(),
            goal: "verify exact code".into(),
            todo_ids: vec!["T1".into()],
            depends_on: vec![],
            required_inputs: vec!["reviewed source".into()],
            expected_outputs: vec!["verification evidence".into()],
            acceptance: vec!["evidence bound".into()],
            verification_hints: vec!["cargo test".into()],
        }],
        todos: vec![TodoSpec {
            id: "T1".into(),
            jobpack_id: "JP1".into(),
            title: "Implement".into(),
            checklist: vec!["done".into()],
        }],
    }
    checkpoints: vec![],
    evidence_requirements: vec![],
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
    assert_eq!(
        controller
            .resolve_or_activate()
            .unwrap()
            .unwrap()
            .jobpack_id,
        "JP1"
    );
    (dir, registry, version)
}

fn checkpoint_and_review(
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
            &[],
            &[],
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

fn profile() -> VerificationProfile {
    VerificationProfile {
        status: DiscoveryStatus::Applicable,
        capabilities: vec![VerificationCapability::Unit],
        commands: vec![VerificationCommand {
            id: "cargo-test".into(),
            kind: VerificationCommandKind::Test,
            capability: VerificationCapability::Unit,
            argv: vec!["cargo".into(), "test".into()],
            source_paths: vec!["Cargo.toml".into()],
            config_hash: "config-hash".into(),
        }],
        reason: None,
    }
}

fn passing_evidence() -> VerificationEvidence {
    VerificationEvidence {
        profile: profile(),
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
        test_surface_changed: false,
    }
}

#[test]
fn verification_requires_exact_reviewer_pass() {
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
            "implemented",
            &[],
            &[],
            &json!([{
                "path":"src/lib.rs",
                "before_sha256":"before",
                "after_sha256":"after"
            }]),
            1,
            0,
        )
        .unwrap();

    assert!(registry
        .record_verification_evidence(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            &passing_evidence(),
        )
        .is_err());
    assert_eq!(
        registry
            .latest_verification_result(version, "JP1", "change-1")
            .unwrap(),
        None
    );
}

#[test]
fn test_pass_is_derived_only_from_matching_observed_exit_zero() {
    let (dir, registry, version) = setup();
    checkpoint_and_review(&dir, &registry, version, "change-1");

    let result = registry
        .record_verification_evidence(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            &passing_evidence(),
        )
        .unwrap();
    assert_eq!(result, VerificationResult::TestPass);
    assert_eq!(
        registry
            .latest_verification_result(version, "JP1", "change-1")
            .unwrap()
            .as_deref(),
        Some("TEST_PASS")
    );

    let mut mismatched = passing_evidence();
    mismatched.commands[0].config_hash = "wrong".into();
    let blocked = registry
        .record_verification_evidence(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            &mismatched,
        )
        .unwrap();
    assert_eq!(blocked, VerificationResult::Blocked);
}

#[test]
fn nonzero_timeout_and_not_applicable_cannot_be_test_pass() {
    let (dir, registry, version) = setup();
    checkpoint_and_review(&dir, &registry, version, "change-1");

    let mut failed = passing_evidence();
    failed.commands[0].exit_code = Some(1);
    assert_eq!(
        registry
            .record_verification_evidence(
                dir.path(),
                "owner-a",
                version,
                "JP1",
                "change-1",
                &failed,
            )
            .unwrap(),
        VerificationResult::Fail
    );

    let not_applicable = VerificationEvidence {
        profile: VerificationProfile {
            status: DiscoveryStatus::NotApplicable,
            capabilities: vec![],
            commands: vec![],
            reason: Some("none".into()),
        },
        commands: vec![],
        test_surface_changed: false,
    };
    assert_eq!(
        registry
            .record_verification_evidence(
                dir.path(),
                "owner-a",
                version,
                "JP1",
                "change-1",
                &not_applicable,
            )
            .unwrap(),
        VerificationResult::NotApplicable
    );
}

#[test]
fn successful_build_only_evidence_is_not_a_test_pass() {
    let evidence = VerificationEvidence {
        profile: VerificationProfile {
            status: DiscoveryStatus::Applicable,
            capabilities: vec![VerificationCapability::BuildOnly],
            commands: vec![VerificationCommand {
                id: "cargo-check".into(),
                kind: VerificationCommandKind::Typecheck,
                capability: VerificationCapability::BuildOnly,
                argv: vec!["cargo".into(), "check".into()],
                source_paths: vec!["Cargo.toml".into()],
                config_hash: "config-hash".into(),
            }],
            reason: Some("no test surface".into()),
        },
        commands: vec![CommandEvidence {
            command_id: "cargo-check".into(),
            config_hash: "config-hash".into(),
            argv: vec!["cargo".into(), "check".into()],
            exit_code: Some(0),
            duration_ms: 10,
            timed_out: false,
            blocked_reason: None,
            stdout: String::new(),
            stderr: String::new(),
        }],
        test_surface_changed: false,
    };

    assert_eq!(evidence.derived_result(), VerificationResult::NotApplicable);
}

#[test]
fn build_only_failure_still_fails_instead_of_becoming_not_applicable() {
    let mut evidence = VerificationEvidence {
        profile: VerificationProfile {
            status: DiscoveryStatus::Applicable,
            capabilities: vec![VerificationCapability::BuildOnly],
            commands: vec![VerificationCommand {
                id: "cargo-check".into(),
                kind: VerificationCommandKind::Typecheck,
                capability: VerificationCapability::BuildOnly,
                argv: vec!["cargo".into(), "check".into()],
                source_paths: vec!["Cargo.toml".into()],
                config_hash: "config-hash".into(),
            }],
            reason: Some("no test surface".into()),
        },
        commands: vec![CommandEvidence {
            command_id: "cargo-check".into(),
            config_hash: "config-hash".into(),
            argv: vec!["cargo".into(), "check".into()],
            exit_code: Some(1),
            duration_ms: 10,
            timed_out: false,
            blocked_reason: None,
            stdout: String::new(),
            stderr: "failed".into(),
        }],
        test_surface_changed: false,
    };

    assert_eq!(evidence.derived_result(), VerificationResult::Fail);
    evidence.commands[0].exit_code = None;
    evidence.commands[0].blocked_reason = Some("sandbox unavailable".into());
    assert_eq!(evidence.derived_result(), VerificationResult::Blocked);
}
