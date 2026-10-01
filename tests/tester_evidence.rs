use gsa_local::{
    controller::MilestoneController,
    execution_graph::{
        CheckpointBoundaryKind, CheckpointPrerequisiteSpec, EvidenceOutputSpec,
        EvidenceRequirementSpec, ExecutionGraph, JobPackSpec, MilestoneSpec, PrerequisiteState,
        TestCheckpointSpec, TodoSpec,
    },
    harness::AgentId,
    plan::{EvidenceMode, EvidenceNeed, PlanArtifact},
    registry::{ChecklistClaim, Registry, ReviewActor, ReviewVerdict},
    tester_evidence::{
        AdapterObservationField, ApplicabilityContext, ApplicabilityDecision, ApplicabilityMatcher,
        EvidenceApplicability, EvidenceProvenance, ExperimentContext, ExperimentObservation,
        ExperimentSample, ObservedValue, ReplaySafety, RevalidationPolicy, TesterAttemptEvidence,
        TesterClassification, TesterEvidenceOutputRecord, TesterEvidenceRef, TesterModeOutcome,
        TesterModeResult, TesterPrerequisiteTarget, TesterTargetBinding,
        VerificationObservationField,
    },
    tester_execution::{
        TesterAdapterRequest, TesterExecutionObservation, TesterExecutionRuntime,
        TesterExecutionStatus, TesterExecutionStepRequest,
    },
    tester_workspace::{TesterArtifactRef, TesterWorkspaceRuntime},
    verification::{
        CommandEvidence, DiscoveryStatus, VerificationCapability, VerificationCommand,
        VerificationCommandKind, VerificationEvidence, VerificationProfile, VerificationResult,
    },
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Duration};
use tempfile::tempdir;

fn plan() -> PlanArtifact {
    PlanArtifact {
        goal: "Use observed runtime identity".into(),
        current_architecture: "runtime identity lifetime is unknown".into(),
        required_changes: vec!["measure identity before consumer".into()],
        implementation_approach: vec!["probe reviewed product state".into()],
        dependencies: vec![],
        sequence: vec!["implement".into(), "probe".into()],
        risks: vec!["guessed binding id".into()],
        acceptance_direction: vec!["observed evidence only".into()],
        evidence_needs: vec![EvidenceNeed {
            id: "runtime-id".into(),
            question: "Which runtime id is stable?".into(),
            purpose: "Prevent guessed session binding.".into(),
            required: true,
            consumer: "later session work".into(),
            modes: vec![EvidenceMode::Probe],
            intent: "Observe runtime identity across a session boundary.".into(),
        }],
    }
}

fn graph() -> ExecutionGraph {
    ExecutionGraph {
        milestones: vec![MilestoneSpec {
            id: "M1".into(),
            title: "Evidence".into(),
            order: 1,
        }],
        jobpacks: vec![
            JobPackSpec {
                id: "JP1".into(),
                milestone_id: "M1".into(),
                title: "Product slice".into(),
                goal: "Create reviewed target".into(),
                todo_ids: vec!["T1".into()],
                depends_on: vec![],
                required_inputs: vec!["approved plan".into()],
                expected_outputs: vec!["reviewed source".into()],
                acceptance: vec!["source reviewed".into()],
                verification_hints: vec!["deterministic self-check".into()],
            },
            JobPackSpec {
                id: "JP2".into(),
                milestone_id: "M1".into(),
                title: "Evidence consumer".into(),
                goal: "Use observed runtime identity".into(),
                todo_ids: vec!["T2".into()],
                depends_on: vec!["JP1".into()],
                required_inputs: vec!["observed runtime identity".into()],
                expected_outputs: vec!["consumer configured from evidence".into()],
                acceptance: vec!["no guessed identity".into()],
                verification_hints: vec!["inspect injected evidence".into()],
            },
        ],
        todos: vec![
            TodoSpec {
                id: "T1".into(),
                jobpack_id: "JP1".into(),
                title: "Implement slice".into(),
                checklist: vec!["implemented".into()],
            },
            TodoSpec {
                id: "T2".into(),
                jobpack_id: "JP2".into(),
                title: "Consume evidence".into(),
                checklist: vec!["uses observed value".into()],
            },
        ],
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
            evidence_need_ids: vec!["runtime-id".into()],
            modes: vec![EvidenceMode::Probe],
            goal: "Observe runtime identity".into(),
            criteria: vec!["Capture identity from the reviewed target".into()],
            required_capabilities: vec!["RUNTIME_PROBE".into()],
            experiment_dimensions: vec!["session".into()],
            evidence_outputs: vec![EvidenceOutputSpec {
                id: "runtime-id-observation".into(),
                mode: EvidenceMode::Probe,
                description: "Observed runtime identity".into(),
                required: true,
                evidence_need_id: Some("runtime-id".into()),
            }],
        }],
        evidence_requirements: vec![EvidenceRequirementSpec {
            consumer_jobpack_id: "JP2".into(),
            checkpoint_id: "CP1".into(),
            output_id: "runtime-id-observation".into(),
            required: true,
        }],
    }
}

fn setup() -> (tempfile::TempDir, Registry, i64, i64) {
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

    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP1")
        .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/session.rs"), "after").unwrap();
    let session_sha = Sha256::digest(b"after")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            "reviewed evidence target",
            &[ChecklistClaim {
                todo_id: "T1".into(),
                position: 1,
            }],
            &["target ready".into()],
            &json!([{
                "path":"src/session.rs",
                "before_sha256":"before",
                "after_sha256":session_sha
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
            "change-1",
            ReviewVerdict::Pass,
            &[],
            1,
            1,
        )
        .unwrap();

    let command = VerificationCommand {
        id: "runtime-probe".into(),
        kind: VerificationCommandKind::Test,
        capability: VerificationCapability::Integration,
        argv: vec!["probe-runtime".into()],
        source_paths: vec!["tests/runtime_probe.rs".into()],
        config_hash: "probe-config".into(),
    };
    let verification = VerificationEvidence {
        profile: VerificationProfile {
            status: DiscoveryStatus::Applicable,
            capabilities: vec![VerificationCapability::Integration],
            commands: vec![command.clone()],
            reason: None,
        },
        commands: vec![CommandEvidence {
            command_id: command.id,
            config_hash: command.config_hash,
            argv: command.argv,
            exit_code: Some(0),
            duration_ms: 5,
            timed_out: false,
            blocked_reason: None,
            stdout: "runtime-1".into(),
            stderr: String::new(),
        }],
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
                &verification,
            )
            .unwrap(),
        VerificationResult::TestPass
    );
    let verification_run_id = registry
        .latest_verification_run_id(version, "JP1", "change-1")
        .unwrap()
        .unwrap();

    (dir, registry, version, verification_run_id)
}

fn workspace_artifact(
    dir: &tempfile::TempDir,
    version: i64,
    attempt_id: &str,
) -> TesterArtifactRef {
    let workspace = TesterWorkspaceRuntime::new(dir.path(), version, "CP1", attempt_id).unwrap();
    let result = workspace
        .execute(
            AgentId::Tester,
            "tester_workspace_write",
            &json!({
                "path":"artifacts/runtime.txt",
                "content":"runtime-1",
                "create_only":true
            }),
        )
        .unwrap();
    serde_json::from_value(result["artifact"].clone()).unwrap()
}

fn attempt(
    version: i64,
    attempt_id: &str,
    change_set_id: &str,
    artifact: TesterArtifactRef,
    verification_run_id: i64,
) -> TesterAttemptEvidence {
    let artifact_ref = TesterEvidenceRef::WorkspaceArtifact { artifact };
    let runtime_ref = TesterEvidenceRef::VerificationObservation {
        run_id: verification_run_id,
        command_id: "runtime-probe".into(),
        field: VerificationObservationField::Stdout,
        observed: ObservedValue::Text("runtime-1".into()),
    };
    TesterAttemptEvidence {
        graph_version: version,
        checkpoint_id: "CP1".into(),
        attempt_id: attempt_id.into(),
        target: TesterTargetBinding {
            prerequisites: vec![TesterPrerequisiteTarget {
                jobpack_id: "JP1".into(),
                state: PrerequisiteState::ReviewPass,
                change_set_id: Some(change_set_id.into()),
                target_revision: Some("revision-1".into()),
            }],
        },
        mode_results: vec![TesterModeResult {
            mode: EvidenceMode::Probe,
            outcome: TesterModeOutcome::Complete,
            reason: None,
        }],
        classifications: vec![],
        experiment: Some(ExperimentContext {
            dimensions: vec!["session".into()],
            samples: vec![ExperimentSample {
                sample_index: 1,
                variables: BTreeMap::from([("session".into(), "same-chat".into())]),
                boundary_event: Some("initial observation".into()),
                target_revision: Some("revision-1".into()),
                change_set_id: Some(change_set_id.into()),
                runtime_identity: Some("runtime-1".into()),
                capability_fingerprint: Some("capability-set-1".into()),
                observations: vec![ExperimentObservation {
                    name: "runtime_id".into(),
                    value: ObservedValue::Text("runtime-1".into()),
                    unit: None,
                    evidence_refs: vec![runtime_ref.clone(), artifact_ref.clone()],
                    limitations: vec![],
                }],
            }],
        }),
        outputs: vec![TesterEvidenceOutputRecord {
            output_id: "runtime-id-observation".into(),
            mode: EvidenceMode::Probe,
            provenance: EvidenceProvenance::Observed,
            value: Some(ObservedValue::Text("runtime-1".into())),
            unit: None,
            evidence_refs: vec![runtime_ref, artifact_ref],
            limitations: vec![],
            applicability: EvidenceApplicability {
                policy: RevalidationPolicy::ReuseIfMatches,
                matchers: vec![ApplicabilityMatcher::RuntimeIdentity {
                    value: "runtime-1".into(),
                }],
            },
        }],
        limitations: vec![],
    }
}

#[test]
fn tester_plan_context_stays_bound_to_graph_when_newer_draft_plan_exists() {
    let (_dir, registry, version, _verification_run_id) = setup();
    let bound = registry.plan_for_current_execution_graph(version).unwrap();
    assert_eq!(bound.revision, 1);
    assert_eq!(bound.artifact.goal, "Use observed runtime identity");

    let mut newer = plan();
    newer.goal = "Unapproved successor plan".into();
    let newer_revision = registry.persist_plan_revision(&newer).unwrap();
    assert_eq!(newer_revision.revision, 2);

    let still_bound = registry.plan_for_current_execution_graph(version).unwrap();
    assert_eq!(still_bound.revision, 1);
    assert_eq!(still_bound.hash, bound.hash);
    assert_eq!(still_bound.artifact.goal, "Use observed runtime identity");
}

#[test]
fn exact_target_probe_evidence_round_trips_from_registry() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-1");
    let attempt = attempt(version, "ATT-1", "change-1", artifact, verification_run_id);

    let fingerprint = registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &attempt)
        .unwrap();
    assert_eq!(
        registry
            .tester_target_fingerprint(version, "CP1", "ATT-1")
            .unwrap()
            .as_deref(),
        Some(fingerprint.as_str())
    );
    assert_eq!(
        registry
            .tester_attempt_evidence(version, "CP1", "ATT-1")
            .unwrap(),
        Some(attempt.clone())
    );
    assert_eq!(
        registry
            .tester_evidence_output(version, "CP1", "ATT-1", "runtime-id-observation")
            .unwrap(),
        Some(attempt.outputs[0].clone())
    );

    let (latest_attempt, latest_output) = registry
        .latest_tester_evidence_output(version, "CP1", "runtime-id-observation")
        .unwrap()
        .unwrap();
    assert_eq!(latest_attempt, "ATT-1");
    assert_eq!(latest_output, attempt.outputs[0]);
}

#[test]
fn restarted_code_workflow_invalidates_prior_review_pass_target() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-OLD-PASS");
    let evidence = attempt(
        version,
        "ATT-OLD-PASS",
        "change-1",
        artifact,
        verification_run_id,
    );

    registry
        .begin_code_workflow(dir.path(), "owner-a", version, "JP1")
        .unwrap();

    assert!(registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &evidence)
        .is_err());
}

#[test]
fn stale_review_target_cannot_record_tester_evidence() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-STALE");
    let stale = attempt(
        version,
        "ATT-STALE",
        "change-old",
        artifact,
        verification_run_id,
    );

    assert!(registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &stale)
        .is_err());
    assert!(registry
        .tester_attempt_evidence(version, "CP1", "ATT-STALE")
        .unwrap()
        .is_none());
}

#[test]
fn stale_workspace_artifact_ref_cannot_be_persisted_as_observed() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-STALE-ARTIFACT");
    let stale = attempt(
        version,
        "ATT-STALE-ARTIFACT",
        "change-1",
        artifact.clone(),
        verification_run_id,
    );
    std::fs::write(
        dir.path()
            .join(".gsa/tester")
            .join(version.to_string())
            .join("CP1/ATT-STALE-ARTIFACT/artifacts/runtime.txt"),
        "mutated-after-ref",
    )
    .unwrap();

    assert!(registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &stale)
        .is_err());
}

#[test]
fn completed_adapter_observation_can_back_observed_evidence() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-EXEC");
    let mut evidence = attempt(
        version,
        "ATT-EXEC",
        "change-1",
        artifact,
        verification_run_id,
    );
    let target = evidence.target.clone();
    let request = TesterExecutionStepRequest {
        step_id: "probe-runtime".into(),
        replay_safety: ReplaySafety::ObserveOnly,
        adapter: TesterAdapterRequest::WorkspacePython {
            script_path: "tests/probe.py".into(),
            args: vec![],
        },
    };
    let target_fingerprint = target.fingerprint(version, "CP1").unwrap();
    let fence = request
        .fence_key(version, "CP1", "ATT-EXEC", &target_fingerprint)
        .unwrap();

    registry
        .prepare_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "ATT-EXEC",
            &target,
            &fence,
            &request.step_id,
            request.adapter.adapter_id(),
            request.replay_safety,
            &fence,
            &serde_json::to_value(&request).unwrap(),
        )
        .unwrap();
    assert!(registry
        .prepare_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "ATT-EXEC",
            &target,
            &fence,
            &request.step_id,
            request.adapter.adapter_id(),
            request.replay_safety,
            &fence,
            &serde_json::to_value(&request).unwrap(),
        )
        .is_err());

    let observation = TesterExecutionObservation {
        execution_id: fence.clone(),
        step_id: request.step_id.clone(),
        adapter_id: request.adapter.adapter_id().into(),
        replay_safety: request.replay_safety,
        fence_key: fence.clone(),
        status: TesterExecutionStatus::Completed,
        exit_code: Some(0),
        duration_ms: 1,
        timed_out: false,
        blocked_reason: None,
        stdout: "runtime-1".into(),
        stderr: String::new(),
        evidence_refs: vec![],
    };
    registry
        .complete_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "ATT-EXEC",
            &observation,
        )
        .unwrap();

    let adapter_ref = TesterEvidenceRef::AdapterObservation {
        adapter_id: request.adapter.adapter_id().into(),
        execution_id: fence,
        field: AdapterObservationField::Stdout,
        observed: ObservedValue::Text("runtime-1".into()),
    };
    evidence.experiment.as_mut().unwrap().samples[0].observations[0].evidence_refs =
        vec![adapter_ref.clone()];
    evidence.outputs[0].evidence_refs = vec![adapter_ref];

    registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &evidence)
        .unwrap();
}

#[test]
fn prepared_execution_is_unresolved_until_closed() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-PREPARED");
    let evidence = attempt(
        version,
        "ATT-PREPARED",
        "change-1",
        artifact,
        verification_run_id,
    );
    let target = evidence.target.clone();
    let request = TesterExecutionStepRequest {
        step_id: "probe-runtime".into(),
        replay_safety: ReplaySafety::ObserveOnly,
        adapter: TesterAdapterRequest::WorkspacePython {
            script_path: "tests/probe.py".into(),
            args: vec![],
        },
    };
    let target_fingerprint = target.fingerprint(version, "CP1").unwrap();
    let fence = request
        .fence_key(version, "CP1", "ATT-PREPARED", &target_fingerprint)
        .unwrap();

    registry
        .prepare_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "ATT-PREPARED",
            &target,
            &fence,
            &request.step_id,
            request.adapter.adapter_id(),
            request.replay_safety,
            &fence,
            &serde_json::to_value(&request).unwrap(),
        )
        .unwrap();

    assert!(registry
        .has_unresolved_tester_execution_steps(version, "CP1", "ATT-PREPARED")
        .unwrap());

    registry
        .complete_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "ATT-PREPARED",
            &TesterExecutionObservation {
                execution_id: fence.clone(),
                step_id: request.step_id.clone(),
                adapter_id: request.adapter.adapter_id().into(),
                replay_safety: request.replay_safety,
                fence_key: fence,
                status: TesterExecutionStatus::Blocked,
                exit_code: None,
                duration_ms: 0,
                timed_out: false,
                blocked_reason: Some("adapter failed".into()),
                stdout: String::new(),
                stderr: String::new(),
                evidence_refs: vec![],
            },
        )
        .unwrap();

    assert!(!registry
        .has_unresolved_tester_execution_steps(version, "CP1", "ATT-PREPARED")
        .unwrap());
}

#[test]
fn fabricated_adapter_value_is_rejected_against_execution_record() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-EXEC-MISMATCH");
    let mut evidence = attempt(
        version,
        "ATT-EXEC-MISMATCH",
        "change-1",
        artifact,
        verification_run_id,
    );
    let target = evidence.target.clone();
    let request = TesterExecutionStepRequest {
        step_id: "probe-runtime".into(),
        replay_safety: ReplaySafety::ObserveOnly,
        adapter: TesterAdapterRequest::WorkspacePython {
            script_path: "tests/probe.py".into(),
            args: vec![],
        },
    };
    let target_fingerprint = target.fingerprint(version, "CP1").unwrap();
    let fence = request
        .fence_key(version, "CP1", "ATT-EXEC-MISMATCH", &target_fingerprint)
        .unwrap();

    registry
        .prepare_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "ATT-EXEC-MISMATCH",
            &target,
            &fence,
            &request.step_id,
            request.adapter.adapter_id(),
            request.replay_safety,
            &fence,
            &serde_json::to_value(&request).unwrap(),
        )
        .unwrap();

    registry
        .complete_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "ATT-EXEC-MISMATCH",
            &TesterExecutionObservation {
                execution_id: fence.clone(),
                step_id: request.step_id.clone(),
                adapter_id: request.adapter.adapter_id().into(),
                replay_safety: request.replay_safety,
                fence_key: fence.clone(),
                status: TesterExecutionStatus::Completed,
                exit_code: Some(0),
                duration_ms: 1,
                timed_out: false,
                blocked_reason: None,
                stdout: "runtime-1".into(),
                stderr: String::new(),
                evidence_refs: vec![],
            },
        )
        .unwrap();

    let forged = TesterEvidenceRef::AdapterObservation {
        adapter_id: request.adapter.adapter_id().into(),
        execution_id: fence,
        field: AdapterObservationField::Stdout,
        observed: ObservedValue::Text("invented".into()),
    };
    evidence.outputs[0].value = Some(ObservedValue::Text("invented".into()));
    evidence.experiment.as_mut().unwrap().samples[0].observations[0].value =
        ObservedValue::Text("invented".into());
    evidence.experiment.as_mut().unwrap().samples[0].observations[0].evidence_refs =
        vec![forged.clone()];
    evidence.outputs[0].evidence_refs = vec![forged];

    assert!(registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &evidence)
        .is_err());
}

#[test]
fn adapter_observation_ref_is_rejected_until_execution_records_exist() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-ADAPTER");
    let mut evidence = attempt(
        version,
        "ATT-ADAPTER",
        "change-1",
        artifact,
        verification_run_id,
    );
    let adapter_ref = TesterEvidenceRef::AdapterObservation {
        adapter_id: "future-adapter".into(),
        execution_id: "exec-1".into(),
        field: AdapterObservationField::Stdout,
        observed: ObservedValue::Text("runtime-1".into()),
    };
    evidence.experiment.as_mut().unwrap().samples[0].observations[0].evidence_refs =
        vec![adapter_ref.clone()];
    evidence.outputs[0].evidence_refs = vec![adapter_ref];

    assert!(registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &evidence)
        .is_err());
}

#[test]
fn observed_value_must_match_persisted_verification_output() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-MISMATCH");
    let mut evidence = attempt(
        version,
        "ATT-MISMATCH",
        "change-1",
        artifact,
        verification_run_id,
    );
    evidence.outputs[0].value = Some(ObservedValue::Text("invented-runtime".into()));
    evidence.outputs[0].evidence_refs = vec![TesterEvidenceRef::VerificationObservation {
        run_id: verification_run_id,
        command_id: "runtime-probe".into(),
        field: VerificationObservationField::Stdout,
        observed: ObservedValue::Text("invented-runtime".into()),
    }];

    assert!(registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &evidence)
        .is_err());
}

#[test]
fn successful_required_probe_output_must_be_observed() {
    let (dir, _registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-IMPLIED");
    let mut implied = attempt(
        version,
        "ATT-IMPLIED",
        "change-1",
        artifact,
        verification_run_id,
    );
    implied.outputs[0].provenance = EvidenceProvenance::Implication;

    assert!(implied
        .validate_against_checkpoint(&graph().checkpoints[0])
        .is_err());
}

#[test]
fn applicability_runtime_decision_cannot_be_overridden_by_prose() {
    let applicability = EvidenceApplicability {
        policy: RevalidationPolicy::ReuseIfMatches,
        matchers: vec![ApplicabilityMatcher::RuntimeIdentity {
            value: "runtime-1".into(),
        }],
    };
    assert_eq!(
        applicability
            .evaluate(&ApplicabilityContext {
                runtime_identity: Some("runtime-1".into()),
                ..Default::default()
            })
            .unwrap(),
        ApplicabilityDecision::Compatible
    );
    assert_eq!(
        applicability
            .evaluate(&ApplicabilityContext {
                runtime_identity: Some("runtime-2".into()),
                ..Default::default()
            })
            .unwrap(),
        ApplicabilityDecision::Invalidated
    );
}

#[test]
fn latest_checkpoint_tracks_exact_tester_execution_stage() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-RESUME-STATE");
    let evidence = attempt(
        version,
        "ATT-RESUME-STATE",
        "change-1",
        artifact,
        verification_run_id,
    );
    let target = evidence.target;
    let request = TesterExecutionStepRequest {
        step_id: "resume-probe".into(),
        replay_safety: ReplaySafety::ObserveOnly,
        adapter: TesterAdapterRequest::WorkspacePython {
            script_path: "tests/resume_probe.py".into(),
            args: vec![],
        },
    };
    let target_fingerprint = target.fingerprint(version, "CP1").unwrap();
    let fence = request
        .fence_key(version, "CP1", "ATT-RESUME-STATE", &target_fingerprint)
        .unwrap();

    registry
        .prepare_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "ATT-RESUME-STATE",
            &target,
            &fence,
            &request.step_id,
            request.adapter.adapter_id(),
            request.replay_safety,
            &fence,
            &serde_json::to_value(&request).unwrap(),
        )
        .unwrap();

    let prepared = registry.latest_tester_resume_state().unwrap().unwrap();
    assert_eq!(prepared.graph_version, version);
    assert_eq!(prepared.checkpoint_id, "CP1");
    assert_eq!(prepared.attempt_id, "ATT-RESUME-STATE");
    assert_eq!(prepared.execution_id.as_deref(), Some(fence.as_str()));
    assert_eq!(prepared.target_fingerprint, target_fingerprint);
    assert_eq!(prepared.stage, "EXECUTION_PREPARED");

    registry
        .complete_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "ATT-RESUME-STATE",
            &TesterExecutionObservation {
                execution_id: fence.clone(),
                step_id: request.step_id.clone(),
                adapter_id: request.adapter.adapter_id().into(),
                replay_safety: request.replay_safety,
                fence_key: fence.clone(),
                status: TesterExecutionStatus::Blocked,
                exit_code: None,
                duration_ms: 0,
                timed_out: false,
                blocked_reason: Some("simulated restart boundary".into()),
                stdout: String::new(),
                stderr: String::new(),
                evidence_refs: vec![],
            },
        )
        .unwrap();

    let completed = registry.latest_tester_resume_state().unwrap().unwrap();
    assert_eq!(completed.execution_id.as_deref(), Some(fence.as_str()));
    assert_eq!(completed.stage, "EXECUTION_COMPLETED");
}

#[test]
fn safe_prepared_execution_replays_same_fence_after_restart() {
    for replay_safety in [ReplaySafety::ObserveOnly, ReplaySafety::Idempotent] {
        let (dir, registry, version, verification_run_id) = setup();
        let attempt_id = format!("ATT-SAFE-{:?}", replay_safety);
        let artifact = workspace_artifact(&dir, version, &attempt_id);
        let evidence = attempt(
            version,
            &attempt_id,
            "change-1",
            artifact,
            verification_run_id,
        );
        let target = evidence.target;
        let request = TesterExecutionStepRequest {
            step_id: "resume-safe".into(),
            replay_safety,
            adapter: TesterAdapterRequest::WorkspacePython {
                script_path: "tests/resume_safe.py".into(),
                args: vec![],
            },
        };
        let target_fingerprint = target.fingerprint(version, "CP1").unwrap();
        let fence = request
            .fence_key(version, "CP1", &attempt_id, &target_fingerprint)
            .unwrap();

        registry
            .prepare_tester_execution_step(
                dir.path(),
                "owner-a",
                version,
                "CP1",
                &attempt_id,
                &target,
                &fence,
                &request.step_id,
                request.adapter.adapter_id(),
                request.replay_safety,
                &fence,
                &serde_json::to_value(&request).unwrap(),
            )
            .unwrap();

        let mut resumed = TesterExecutionRuntime::new(
            &registry,
            dir.path(),
            "owner-a",
            version,
            "CP1",
            &attempt_id,
            target.clone(),
        )
        .unwrap();
        let observations = resumed.resume_prepared().unwrap();
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].execution_id, fence);
        assert!(!registry
            .has_unresolved_tester_execution_steps(version, "CP1", &attempt_id)
            .unwrap());

        let rows = registry
            .tester_execution_steps(version, "CP1", &attempt_id)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_ne!(rows[0].status, "PREPARED");

        let mut restarted = TesterExecutionRuntime::new(
            &registry,
            dir.path(),
            "owner-a",
            version,
            "CP1",
            &attempt_id,
            target,
        )
        .unwrap();
        assert!(restarted.resume_prepared().unwrap().is_empty());
        let persisted = restarted.persisted_steps();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].execution_id, fence);
        assert!(persisted[0].observation.is_some());
        assert_eq!(
            registry
                .tester_execution_steps(version, "CP1", &attempt_id)
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn rereresolve_preserves_prepared_resume_stage_and_attempt_identity() {
    let (dir, registry, version, _verification_run_id) = setup();
    let due = registry
        .resolve_tester_checkpoint(&["RUNTIME_PROBE".into()])
        .unwrap()
        .unwrap();
    assert_eq!(due.next_attempt_id.as_deref(), Some("attempt-0001"));
    let due_state = registry.latest_tester_resume_state().unwrap().unwrap();
    assert_eq!(due_state.stage, "CHECKPOINT_DUE");
    assert_eq!(due_state.attempt_id, "attempt-0001");

    let target = due.target.unwrap();
    let request = TesterExecutionStepRequest {
        step_id: "resume-boundary".into(),
        replay_safety: ReplaySafety::ObserveOnly,
        adapter: TesterAdapterRequest::WorkspacePython {
            script_path: "tests/resume_boundary.py".into(),
            args: vec![],
        },
    };
    let target_fingerprint = target.fingerprint(version, "CP1").unwrap();
    let fence = request
        .fence_key(version, "CP1", "attempt-0001", &target_fingerprint)
        .unwrap();
    registry
        .prepare_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "attempt-0001",
            &target,
            &fence,
            &request.step_id,
            request.adapter.adapter_id(),
            request.replay_safety,
            &fence,
            &serde_json::to_value(&request).unwrap(),
        )
        .unwrap();

    let before = registry.latest_tester_resume_state().unwrap().unwrap();
    assert_eq!(before.stage, "EXECUTION_PREPARED");
    assert_eq!(before.execution_id.as_deref(), Some(fence.as_str()));

    let resolved_again = registry
        .resolve_tester_checkpoint(&["RUNTIME_PROBE".into()])
        .unwrap()
        .unwrap();
    assert_eq!(
        resolved_again.next_attempt_id.as_deref(),
        Some("attempt-0001")
    );
    let after = registry.latest_tester_resume_state().unwrap().unwrap();
    assert_eq!(after, before);
}

#[test]
fn uncertain_non_idempotent_execution_requires_human_after_restart() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-NON-IDEMPOTENT");
    let evidence = attempt(
        version,
        "ATT-NON-IDEMPOTENT",
        "change-1",
        artifact,
        verification_run_id,
    );
    let target = evidence.target;
    let request = TesterExecutionStepRequest {
        step_id: "external-mutation".into(),
        replay_safety: ReplaySafety::NonIdempotent,
        adapter: TesterAdapterRequest::WorkspacePython {
            script_path: "tests/non_idempotent.py".into(),
            args: vec![],
        },
    };
    let target_fingerprint = target.fingerprint(version, "CP1").unwrap();
    let fence = request
        .fence_key(version, "CP1", "ATT-NON-IDEMPOTENT", &target_fingerprint)
        .unwrap();

    registry
        .prepare_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "ATT-NON-IDEMPOTENT",
            &target,
            &fence,
            &request.step_id,
            request.adapter.adapter_id(),
            request.replay_safety,
            &fence,
            &serde_json::to_value(&request).unwrap(),
        )
        .unwrap();

    let mut resumed = TesterExecutionRuntime::new(
        &registry,
        dir.path(),
        "owner-a",
        version,
        "CP1",
        "ATT-NON-IDEMPOTENT",
        target,
    )
    .unwrap();
    let error = resumed.resume_prepared().unwrap_err();
    assert!(format!("{error:#}").contains("NEEDS_HUMAN"));
    assert!(registry
        .has_unresolved_tester_execution_steps(version, "CP1", "ATT-NON-IDEMPOTENT")
        .unwrap());

    let recovery = registry.latest_tester_resume_state().unwrap().unwrap();
    assert_eq!(recovery.execution_id.as_deref(), Some(fence.as_str()));
    assert_eq!(recovery.stage, "RECOVERY_REQUIRED");
}

#[test]
fn lease_owner_is_required_before_tester_attempt_can_prepare_execution() {
    let (dir, registry, version, verification_run_id) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-LEASE");
    let evidence = attempt(
        version,
        "ATT-LEASE",
        "change-1",
        artifact,
        verification_run_id,
    );
    let target = evidence.target;
    let request = TesterExecutionStepRequest {
        step_id: "lease-guard".into(),
        replay_safety: ReplaySafety::ObserveOnly,
        adapter: TesterAdapterRequest::WorkspacePython {
            script_path: "tests/lease_guard.py".into(),
            args: vec![],
        },
    };
    let target_fingerprint = target.fingerprint(version, "CP1").unwrap();
    let fence = request
        .fence_key(version, "CP1", "ATT-LEASE", &target_fingerprint)
        .unwrap();

    assert!(registry
        .prepare_tester_execution_step(
            dir.path(),
            "owner-b",
            version,
            "CP1",
            "ATT-LEASE",
            &target,
            &fence,
            &request.step_id,
            request.adapter.adapter_id(),
            request.replay_safety,
            &fence,
            &serde_json::to_value(&request).unwrap(),
        )
        .is_err());

    registry
        .prepare_tester_execution_step(
            dir.path(),
            "owner-a",
            version,
            "CP1",
            "ATT-LEASE",
            &target,
            &fence,
            &request.step_id,
            request.adapter.adapter_id(),
            request.replay_safety,
            &fence,
            &serde_json::to_value(&request).unwrap(),
        )
        .unwrap();
}

#[test]
fn downstream_active_work_receives_only_resolved_observed_tester_evidence() {
    let (dir, registry, version, verification_run_id) = setup();
    let due = registry
        .resolve_tester_checkpoint(&["RUNTIME_PROBE".into()])
        .unwrap()
        .unwrap();
    let artifact = workspace_artifact(&dir, version, due.next_attempt_id.as_deref().unwrap());
    let attempt = attempt(
        version,
        due.next_attempt_id.as_deref().unwrap(),
        "change-1",
        artifact,
        verification_run_id,
    );
    registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &attempt)
        .unwrap();

    let catalog = registry.current_tester_evidence_catalog().unwrap();
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].output_id, "runtime-id-observation");
    assert_eq!(catalog[0].value, ObservedValue::Text("runtime-1".into()));

    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let cr = registry.resolve_code_cr_boundary().unwrap().unwrap();
    assert!(cr.terminal);
    registry
        .record_code_cr_review(dir.path(), "owner-a", &cr.key, ReviewVerdict::Pass, &[])
        .unwrap();
    let next = controller.mark_active_jobpack_done().unwrap().unwrap();
    assert_eq!(next.jobpack_id, "JP2");
    assert_eq!(next.tester_evidence.len(), 1);
    assert_eq!(next.tester_evidence[0].checkpoint_id, "CP1");
    assert_eq!(next.tester_evidence[0].output_id, "runtime-id-observation");
    assert_eq!(
        next.tester_evidence[0].value,
        ObservedValue::Text("runtime-1".into())
    );
}

#[test]
fn required_tester_evidence_fails_closed_when_missing() {
    let (dir, registry, _version, _verification_run_id) = setup();
    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let error = controller.mark_active_jobpack_done().unwrap_err();
    assert!(format!("{error:#}").contains("not mature for terminal Local CR"));
}

#[test]
fn observed_output_from_unsuccessful_mode_cannot_satisfy_required_consumer_evidence() {
    let (dir, registry, version, verification_run_id) = setup();
    let due = registry
        .resolve_tester_checkpoint(&["RUNTIME_PROBE".into()])
        .unwrap()
        .unwrap();
    let artifact = workspace_artifact(&dir, version, due.next_attempt_id.as_deref().unwrap());
    let mut attempt = attempt(
        version,
        due.next_attempt_id.as_deref().unwrap(),
        "change-1",
        artifact,
        verification_run_id,
    );
    attempt.mode_results[0].outcome = TesterModeOutcome::Blocked;
    attempt.mode_results[0].reason = Some("probe did not complete".into());
    attempt.classifications = vec![TesterClassification::TestFailure];

    registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &attempt)
        .unwrap();

    assert!(registry
        .current_tester_evidence_catalog()
        .unwrap()
        .is_empty());

    let controller = MilestoneController::new(&registry, dir.path(), "owner-a");
    let error = controller.mark_active_jobpack_done().unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("not mature for terminal Local CR"));
}
