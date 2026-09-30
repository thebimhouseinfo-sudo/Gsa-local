use gsa_local::{
    controller::MilestoneController,
    execution_graph::{
        CheckpointBoundaryKind, CheckpointPrerequisiteSpec, EvidenceOutputSpec, ExecutionGraph,
        JobPackSpec, MilestoneSpec, PrerequisiteState, TestCheckpointSpec, TodoSpec,
    },
    harness::AgentId,
    plan::{EvidenceMode, EvidenceNeed, PlanArtifact},
    registry::{Registry, ReviewActor, ReviewVerdict},
    tester_evidence::{
        AdapterObservationField, ApplicabilityContext, ApplicabilityDecision, ApplicabilityMatcher,
        EvidenceApplicability, EvidenceProvenance, ExperimentContext, ExperimentObservation,
        ExperimentSample, ObservedValue, ReplaySafety, RevalidationPolicy, TesterAttemptEvidence,
        TesterEvidenceOutputRecord, TesterEvidenceRef, TesterModeOutcome, TesterModeResult,
        TesterPrerequisiteTarget, TesterTargetBinding, VerificationObservationField,
    },
    tester_execution::{
        TesterAdapterRequest, TesterExecutionObservation, TesterExecutionStatus,
        TesterExecutionStepRequest,
    },
    tester_workspace::{TesterArtifactRef, TesterWorkspaceRuntime},
    verification::{
        CommandEvidence, DiscoveryStatus, VerificationCapability, VerificationCommand,
        VerificationCommandKind, VerificationEvidence, VerificationProfile, VerificationResult,
    },
};
use serde_json::json;
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
        jobpacks: vec![JobPackSpec {
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
        }],
        todos: vec![TodoSpec {
            id: "T1".into(),
            jobpack_id: "JP1".into(),
            title: "Implement slice".into(),
            checklist: vec!["implemented".into()],
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
        evidence_requirements: vec![],
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
    registry
        .record_code_checkpoint(
            dir.path(),
            "owner-a",
            version,
            "JP1",
            "change-1",
            "reviewed evidence target",
            &[],
            &["target ready".into()],
            &json!([{
                "path":"src/session.rs",
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
