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
    tester_execution::tester_capability_catalog,
    verification::{DiscoveryStatus, VerificationCapability, VerificationProfile},
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
        milestones: vec![MilestoneSpec {
            id: "M1".into(),
            title: "M1".into(),
            order: 1,
        }],
        jobpacks: vec![JobPackSpec {
            id: "JP1".into(),
            milestone_id: "M1".into(),
            title: "JP1".into(),
            goal: "reviewed target".into(),
            todo_ids: vec!["T1".into()],
            depends_on: vec![],
            required_inputs: vec!["plan".into()],
            expected_outputs: vec!["source".into()],
            acceptance: vec!["reviewed".into()],
            verification_hints: vec!["checkpoint".into()],
        }],
        todos: vec![TodoSpec {
            id: "T1".into(),
            jobpack_id: "JP1".into(),
            title: "T1".into(),
            checklist: vec!["implement".into()],
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
            cr_review_boundary: false,
            evidence_need_ids: vec![],
            modes: vec![EvidenceMode::Verify],
            goal: "verify reviewed target".into(),
            criteria: vec!["works".into()],
            required_capabilities,
            experiment_dimensions: vec![],
            evidence_outputs: vec![EvidenceOutputSpec {
                id: "verdict".into(),
                mode: EvidenceMode::Verify,
                description: "verdict".into(),
                required: false,
                evidence_need_id: None,
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
        .register_execution_graph(
            revision.revision,
            &revision.hash,
            &graph(required_capabilities),
        )
        .unwrap();
    registry
        .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
        .unwrap();
    MilestoneController::new(&registry, dir.path(), "owner-a")
        .resolve_or_activate()
        .unwrap()
        .unwrap();
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
            &[],
            &json!([{"path":"src/example.rs","before_sha256":"a","after_sha256":"b"}]),
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
            "change-1",
            ReviewVerdict::Pass,
            &[],
            1,
            1,
        )
        .unwrap();
    (dir, registry, version)
}

fn tester_work(
    controller: &MilestoneController<'_>,
    capabilities: &[String],
) -> gsa_local::registry::TesterCheckpointWorkRecord {
    match controller.resolve_next(capabilities).unwrap().unwrap() {
        NextWork::Tester(work) => work,
        NextWork::Coder(work) => panic!("expected Tester, got Coder {}", work.jobpack_id),
        NextWork::Repair(work) => panic!(
            "expected Tester, got Repair for checkpoint {}",
            work.checkpoint.id
        ),
        NextWork::Cr(work) => panic!("expected Tester, got CR {}", work.key.boundary_id),
    }
}

fn repair_work(controller: &MilestoneController<'_>) -> gsa_local::controller::TesterRepairWork {
    match controller.resolve_next(&[]).unwrap().unwrap() {
        NextWork::Repair(work) => work,
        NextWork::Tester(work) => {
            panic!("expected Repair, got Tester {}", work.disposition.as_str())
        }
        NextWork::Coder(work) => panic!("expected Repair, got Coder {}", work.jobpack_id),
        NextWork::Cr(work) => panic!("expected Repair, got CR {}", work.key.boundary_id),
    }
}

fn record_product_failure(
    dir: &tempfile::TempDir,
    registry: &Registry,
    version: i64,
    attempt_id: String,
    target: gsa_local::tester_evidence::TesterTargetBinding,
) {
    registry
        .record_tester_attempt_evidence(
            dir.path(),
            "owner-a",
            &TesterAttemptEvidence {
                graph_version: version,
                checkpoint_id: "CP1".into(),
                attempt_id,
                target,
                mode_results: vec![TesterModeResult {
                    mode: EvidenceMode::Verify,
                    outcome: TesterModeOutcome::Fail,
                    reason: Some("observed product behavior violates checkpoint criterion".into()),
                }],
                classifications: vec![TesterClassification::ProductFailure],
                experiment: None,
                outputs: vec![],
                limitations: vec![],
            },
        )
        .unwrap();
}

fn review_repair_target(
    dir: &tempfile::TempDir,
    registry: &Registry,
    version: i64,
    change_set_id: &str,
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
            change_set_id,
            "repair product failure",
            &[],
            &["recheck failed Tester criterion".into()],
            &json!([{"path":"src/example.rs","before_sha256":"old","after_sha256":change_set_id}]),
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
            change_set_id,
            ReviewVerdict::Pass,
            &[],
            1,
            1,
        )
        .unwrap();
}

#[test]
fn review_pass_is_due_without_marking_jobpack_done() {
    let (dir, registry, version) = setup(vec![]);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let work = tester_work(&controller, &[]);
    assert_eq!(work.disposition, TesterCheckpointDisposition::Due);
    assert_eq!(
        work.target.unwrap().prerequisites[0]
            .change_set_id
            .as_deref(),
        Some("change-1")
    );
    assert_eq!(
        registry.jobpack_status(version, "JP1").unwrap().as_deref(),
        Some("ACTIVE")
    );
}

#[test]
fn exact_target_pass_satisfies_checkpoint_without_completion() {
    let (dir, registry, version) = setup(vec![]);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let due = tester_work(&controller, &[]);
    registry
        .record_tester_attempt_evidence(
            dir.path(),
            "owner-a",
            &TesterAttemptEvidence {
                graph_version: version,
                checkpoint_id: "CP1".into(),
                attempt_id: due.next_attempt_id.unwrap(),
                target: due.target.unwrap(),
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

    match controller.resolve_next(&[]).unwrap().unwrap() {
        NextWork::Coder(work) => assert_eq!(work.jobpack_id, "JP1"),
        NextWork::Tester(work) => panic!("checkpoint remained {}", work.disposition.as_str()),
        NextWork::Repair(work) => panic!("checkpoint requested repair {}", work.checkpoint.id),
        NextWork::Cr(work) => panic!(
            "checkpoint unexpectedly requested CR {}",
            work.key.boundary_id
        ),
    }
    assert_eq!(
        registry.jobpack_status(version, "JP1").unwrap().as_deref(),
        Some("ACTIVE")
    );
}

#[test]
fn runtime_probe_capability_comes_from_tester_catalog() {
    let (dir, registry, _version) = setup(vec!["RUNTIME_PROBE".into()]);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let profile = VerificationProfile {
        status: DiscoveryStatus::Applicable,
        capabilities: vec![VerificationCapability::Integration],
        commands: vec![],
        reason: None,
    };
    let capabilities = tester_capability_catalog(&profile);
    let work = tester_work(&controller, &capabilities);
    assert_eq!(work.disposition, TesterCheckpointDisposition::Due);
    assert!(capabilities.iter().any(|item| item == "RUNTIME_PROBE"));
    assert!(capabilities.iter().any(|item| item == "INTEGRATION"));
}

#[test]
fn missing_capability_is_integration_not_ready() {
    let (dir, registry, _version) = setup(vec!["BROWSER".into()]);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let work = tester_work(&controller, &["INTEGRATION".into()]);
    assert_eq!(
        work.disposition,
        TesterCheckpointDisposition::IntegrationNotReady
    );
    assert!(work.reason.unwrap().contains("BROWSER"));
}

#[test]
fn product_failure_routes_to_active_coder_repair_without_marking_done() {
    let (dir, registry, version) = setup(vec![]);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let due = tester_work(&controller, &[]);
    record_product_failure(
        &dir,
        &registry,
        version,
        due.next_attempt_id.unwrap(),
        due.target.unwrap(),
    );

    let repair = repair_work(&controller);
    assert_eq!(repair.active_work.jobpack_id, "JP1");
    assert_eq!(repair.checkpoint.id, "CP1");
    assert_eq!(repair.retest_context.failed_attempt_id, "attempt-0001");
    assert!(repair
        .retest_context
        .failure_summary
        .contains("PRODUCT_FAILURE"));
    assert_eq!(
        registry.jobpack_status(version, "JP1").unwrap().as_deref(),
        Some("ACTIVE")
    );
}

#[test]
fn reviewed_repair_retests_same_checkpoint_on_new_exact_target() {
    let (dir, registry, version) = setup(vec![]);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let due = tester_work(&controller, &[]);
    record_product_failure(
        &dir,
        &registry,
        version,
        due.next_attempt_id.unwrap(),
        due.target.unwrap(),
    );
    let repair = repair_work(&controller);
    assert_eq!(repair.active_work.jobpack_id, "JP1");

    review_repair_target(&dir, &registry, version, "change-2");

    let retest = tester_work(&controller, &[]);
    assert_eq!(retest.disposition, TesterCheckpointDisposition::Due);
    assert_eq!(retest.checkpoint.id, "CP1");
    assert_eq!(retest.next_attempt_id.as_deref(), Some("attempt-0002"));
    assert_eq!(
        retest.target.as_ref().unwrap().prerequisites[0]
            .change_set_id
            .as_deref(),
        Some("change-2")
    );
    let context = retest.retest_context.as_ref().unwrap();
    assert_eq!(context.failed_attempt_id, "attempt-0001");
    assert_ne!(
        context.failed_target_fingerprint,
        retest.target_fingerprint.as_deref().unwrap()
    );

    registry
        .record_tester_attempt_evidence(
            dir.path(),
            "owner-a",
            &TesterAttemptEvidence {
                graph_version: version,
                checkpoint_id: "CP1".into(),
                attempt_id: retest.next_attempt_id.unwrap(),
                target: retest.target.unwrap(),
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

    match controller.resolve_next(&[]).unwrap().unwrap() {
        NextWork::Coder(work) => assert_eq!(work.jobpack_id, "JP1"),
        NextWork::Tester(work) => panic!("retest remained {}", work.disposition.as_str()),
        NextWork::Repair(work) => panic!("retest still requested repair {}", work.checkpoint.id),
        NextWork::Cr(work) => panic!("retest unexpectedly requested CR {}", work.key.boundary_id),
    }
}

#[test]
fn prior_quality_pass_cannot_satisfy_a_later_reviewed_target() {
    let (dir, registry, version) = setup(vec![]);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let due = tester_work(&controller, &[]);
    registry
        .record_tester_attempt_evidence(
            dir.path(),
            "owner-a",
            &TesterAttemptEvidence {
                graph_version: version,
                checkpoint_id: "CP1".into(),
                attempt_id: due.next_attempt_id.unwrap(),
                target: due.target.unwrap(),
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

    review_repair_target(&dir, &registry, version, "change-2");

    let new_target = tester_work(&controller, &[]);
    assert_eq!(new_target.disposition, TesterCheckpointDisposition::Due);
    assert_eq!(new_target.next_attempt_id.as_deref(), Some("attempt-0002"));
    assert_eq!(
        new_target.target.as_ref().unwrap().prerequisites[0]
            .change_set_id
            .as_deref(),
        Some("change-2")
    );
    assert!(new_target.retest_context.is_none());
}

#[test]
fn non_product_failure_and_needs_human_still_stop_coder_progression() {
    for (outcome, classification, expected) in [
        (
            TesterModeOutcome::Fail,
            TesterClassification::TestFailure,
            TesterCheckpointDisposition::Blocked,
        ),
        (
            TesterModeOutcome::NeedsHuman,
            TesterClassification::ProductFailure,
            TesterCheckpointDisposition::NeedsHuman,
        ),
    ] {
        let (dir, registry, version) = setup(vec![]);
        let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
        let due = tester_work(&controller, &[]);
        registry
            .record_tester_attempt_evidence(
                dir.path(),
                "owner-a",
                &TesterAttemptEvidence {
                    graph_version: version,
                    checkpoint_id: "CP1".into(),
                    attempt_id: due.next_attempt_id.unwrap(),
                    target: due.target.unwrap(),
                    mode_results: vec![TesterModeResult {
                        mode: EvidenceMode::Verify,
                        outcome,
                        reason: Some("not satisfied".into()),
                    }],
                    classifications: vec![classification],
                    experiment: None,
                    outputs: vec![],
                    limitations: vec![],
                },
            )
            .unwrap();
        assert_eq!(tester_work(&controller, &[]).disposition, expected);
        assert_eq!(
            registry.jobpack_status(version, "JP1").unwrap().as_deref(),
            Some("ACTIVE")
        );
    }
}

#[test]
fn spec_gap_is_preserved_as_a_distinct_replanning_signal() {
    let (dir, registry, version) = setup(vec![]);
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let due = tester_work(&controller, &[]);
    registry
        .record_tester_attempt_evidence(
            dir.path(),
            "owner-a",
            &TesterAttemptEvidence {
                graph_version: version,
                checkpoint_id: "CP1".into(),
                attempt_id: due.next_attempt_id.unwrap(),
                target: due.target.unwrap(),
                mode_results: vec![TesterModeResult {
                    mode: EvidenceMode::Verify,
                    outcome: TesterModeOutcome::Blocked,
                    reason: Some("approved behavior is underspecified".into()),
                }],
                classifications: vec![TesterClassification::SpecGap],
                experiment: None,
                outputs: vec![],
                limitations: vec![],
            },
        )
        .unwrap();

    let work = tester_work(&controller, &[]);
    assert_eq!(work.disposition, TesterCheckpointDisposition::SpecGap);
    assert!(work.reason.unwrap().contains("SPEC_GAP"));
}
