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
        ApplicabilityContext, ApplicabilityDecision, ApplicabilityMatcher, EvidenceApplicability,
        EvidenceProvenance, ExperimentContext, ExperimentObservation, ExperimentSample,
        ObservedValue, RevalidationPolicy, TesterAttemptEvidence, TesterEvidenceOutputRecord,
        TesterEvidenceRef, TesterModeOutcome, TesterModeResult, TesterPrerequisiteTarget,
        TesterTargetBinding,
    },
    tester_workspace::{TesterArtifactRef, TesterWorkspaceRuntime},
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

fn setup() -> (tempfile::TempDir, Registry, i64) {
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

    (dir, registry, version)
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
) -> TesterAttemptEvidence {
    let evidence_ref = TesterEvidenceRef::WorkspaceArtifact { artifact };
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
                    evidence_refs: vec![evidence_ref.clone()],
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
            evidence_refs: vec![evidence_ref],
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
fn exact_target_probe_evidence_round_trips_from_registry() {
    let (dir, registry, version) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-1");
    let attempt = attempt(version, "ATT-1", "change-1", artifact);

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
fn stale_review_target_cannot_record_tester_evidence() {
    let (dir, registry, version) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-STALE");
    let stale = attempt(version, "ATT-STALE", "change-old", artifact);

    assert!(registry
        .record_tester_attempt_evidence(dir.path(), "owner-a", &stale)
        .is_err());
    assert!(registry
        .tester_attempt_evidence(version, "CP1", "ATT-STALE")
        .unwrap()
        .is_none());
}

#[test]
fn successful_required_probe_output_must_be_observed() {
    let (dir, _registry, version) = setup();
    let artifact = workspace_artifact(&dir, version, "ATT-IMPLIED");
    let mut implied = attempt(version, "ATT-IMPLIED", "change-1", artifact);
    implied.outputs[0].provenance = EvidenceProvenance::Implication;

    assert!(implied.validate_against_checkpoint(&graph().checkpoints[0]).is_err());
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
