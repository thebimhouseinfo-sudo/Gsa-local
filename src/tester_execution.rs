use crate::{
    config::AppConfig,
    execution_graph::TestCheckpointSpec,
    harness::{AgentId, HarnessRegistry},
    ollama::{ChatMessage, OllamaClient, ToolCall, ToolDefinition},
    plan::EvidenceNeed,
    process_runner::{ProcessObservation, TesterSandboxRunner},
    registry::{Registry, TesterExecutionStepRecord},
    session::Session,
    tester_evidence::{
        AdapterObservationField, ExperimentContext, ReplaySafety, TesterAttemptEvidence,
        TesterClassification, TesterEvidenceOutputRecord, TesterEvidenceRef, TesterModeResult,
        TesterTargetBinding, VerificationObservationField,
    },
    tester_workspace::TesterWorkspaceRuntime,
    tools::ProjectToolRuntime,
    verification::{VerificationController, VerificationProfile, VerificationResult},
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, path::Path, time::Duration};

const MAX_TESTER_TOOL_ROUNDS: usize = 16;
const MAX_TESTER_EXECUTIONS: usize = 8;
const DEFAULT_TESTER_TIMEOUT: Duration = Duration::from_secs(120);

pub fn tester_capability_catalog(profile: &VerificationProfile) -> Vec<String> {
    let mut capabilities = profile
        .capabilities
        .iter()
        .map(|capability| capability.as_str().to_owned())
        .collect::<BTreeSet<_>>();

    for capability in [
        "PROJECT_VERIFICATION",
        "WORKSPACE_PYTHON",
        "WORKSPACE_NODE",
        "RUNTIME_PROBE",
    ] {
        capabilities.insert(capability.to_owned());
    }

    capabilities.into_iter().collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "adapter", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TesterAdapterRequest {
    WorkspacePython {
        script_path: String,
        #[serde(default)]
        args: Vec<String>,
    },
    WorkspaceNode {
        script_path: String,
        #[serde(default)]
        args: Vec<String>,
    },
    ProjectVerification {
        jobpack_id: String,
        change_set_id: String,
    },
}

impl TesterAdapterRequest {
    pub fn adapter_id(&self) -> &'static str {
        match self {
            Self::WorkspacePython { .. } => "WORKSPACE_PYTHON",
            Self::WorkspaceNode { .. } => "WORKSPACE_NODE",
            Self::ProjectVerification { .. } => "PROJECT_VERIFICATION",
        }
    }

    fn validate(&self, replay_safety: ReplaySafety) -> Result<()> {
        match self {
            Self::WorkspacePython { script_path, args }
            | Self::WorkspaceNode { script_path, args } => {
                validate_test_script_path(script_path)?;
                validate_args(args)
            }
            Self::ProjectVerification {
                jobpack_id,
                change_set_id,
            } => {
                require_text("project verification jobpack_id", jobpack_id)?;
                require_text("project verification change_set_id", change_set_id)?;
                if replay_safety != ReplaySafety::ObserveOnly {
                    bail!("PROJECT_VERIFICATION must declare OBSERVE_ONLY replay safety");
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterExecutionStepRequest {
    pub step_id: String,
    pub replay_safety: ReplaySafety,
    #[serde(flatten)]
    pub adapter: TesterAdapterRequest,
}

impl TesterExecutionStepRequest {
    fn validate(&self) -> Result<()> {
        require_text("Tester execution step_id", &self.step_id)?;
        if self.step_id.contains('/') || self.step_id.contains('\\') || self.step_id.contains(':') {
            bail!("Tester execution step_id must be a logical id, not a path");
        }
        self.adapter.validate(self.replay_safety)
    }

    pub fn fence_key(
        &self,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
        target_fingerprint: &str,
    ) -> Result<String> {
        self.validate()?;
        let canonical = serde_json::to_vec(&(
            graph_version,
            checkpoint_id,
            attempt_id,
            target_fingerprint,
            self,
        ))?;
        Ok(hex_digest(Sha256::digest(canonical)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TesterExecutionStatus {
    Completed,
    Failed,
    Blocked,
}

impl TesterExecutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Blocked => "BLOCKED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterExecutionObservation {
    pub execution_id: String,
    pub step_id: String,
    pub adapter_id: String,
    pub replay_safety: ReplaySafety,
    pub fence_key: String,
    pub status: TesterExecutionStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub blocked_reason: Option<String>,
    pub stdout: String,
    pub stderr: String,
    #[serde(default)]
    pub evidence_refs: Vec<TesterEvidenceRef>,
}

impl TesterExecutionObservation {
    fn blocked(
        execution_id: String,
        request: &TesterExecutionStepRequest,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            fence_key: execution_id.clone(),
            execution_id,
            step_id: request.step_id.clone(),
            adapter_id: request.adapter.adapter_id().into(),
            replay_safety: request.replay_safety,
            status: TesterExecutionStatus::Blocked,
            exit_code: None,
            duration_ms: 0,
            timed_out: false,
            blocked_reason: Some(reason.into()),
            stdout: String::new(),
            stderr: String::new(),
            evidence_refs: vec![],
        }
    }

    fn from_process(
        execution_id: String,
        request: &TesterExecutionStepRequest,
        process: ProcessObservation,
    ) -> Self {
        let status = if process.blocked_reason.is_some() {
            TesterExecutionStatus::Blocked
        } else if process.timed_out || process.exit_code != Some(0) {
            TesterExecutionStatus::Failed
        } else {
            TesterExecutionStatus::Completed
        };
        let mut evidence_refs = Vec::new();
        if !process.stdout.is_empty() {
            evidence_refs.push(TesterEvidenceRef::AdapterObservation {
                adapter_id: request.adapter.adapter_id().into(),
                execution_id: execution_id.clone(),
                field: AdapterObservationField::Stdout,
                observed: crate::tester_evidence::ObservedValue::Text(process.stdout.clone()),
            });
        }
        if !process.stderr.is_empty() {
            evidence_refs.push(TesterEvidenceRef::AdapterObservation {
                adapter_id: request.adapter.adapter_id().into(),
                execution_id: execution_id.clone(),
                field: AdapterObservationField::Stderr,
                observed: crate::tester_evidence::ObservedValue::Text(process.stderr.clone()),
            });
        }
        if let Some(code) = process.exit_code {
            evidence_refs.push(TesterEvidenceRef::AdapterObservation {
                adapter_id: request.adapter.adapter_id().into(),
                execution_id: execution_id.clone(),
                field: AdapterObservationField::ExitCode,
                observed: crate::tester_evidence::ObservedValue::Integer(i64::from(code)),
            });
        }

        Self {
            fence_key: execution_id.clone(),
            execution_id,
            step_id: request.step_id.clone(),
            adapter_id: request.adapter.adapter_id().into(),
            replay_safety: request.replay_safety,
            status,
            exit_code: process.exit_code,
            duration_ms: process.duration_ms,
            timed_out: process.timed_out,
            blocked_reason: process.blocked_reason,
            stdout: process.stdout,
            stderr: process.stderr,
            evidence_refs,
        }
    }
}

pub struct TesterExecutionRuntime<'a> {
    registry: &'a Registry,
    project_root: &'a Path,
    lease_owner: &'a str,
    graph_version: i64,
    checkpoint_id: String,
    attempt_id: String,
    target: TesterTargetBinding,
    workspace: TesterWorkspaceRuntime,
    runner: TesterSandboxRunner,
    execution_count: usize,
    observations: Vec<TesterExecutionObservation>,
    steps: BTreeMap<String, TesterExecutionStepRecord>,
}

impl<'a> TesterExecutionRuntime<'a> {
    pub fn new(
        registry: &'a Registry,
        project_root: &'a Path,
        lease_owner: &'a str,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
        target: TesterTargetBinding,
    ) -> Result<Self> {
        Self::with_runner(
            registry,
            project_root,
            lease_owner,
            graph_version,
            checkpoint_id,
            attempt_id,
            target,
            TesterSandboxRunner::production(),
        )
    }

    fn with_runner(
        registry: &'a Registry,
        project_root: &'a Path,
        lease_owner: &'a str,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
        target: TesterTargetBinding,
        runner: TesterSandboxRunner,
    ) -> Result<Self> {
        let workspace =
            TesterWorkspaceRuntime::new(project_root, graph_version, checkpoint_id, attempt_id)?;
        let target_fingerprint = target.fingerprint(graph_version, checkpoint_id)?;
        let persisted = registry.tester_execution_steps(graph_version, checkpoint_id, attempt_id)?;
        let mut observations = Vec::new();
        let mut steps = BTreeMap::new();
        for record in persisted {
            if record.target_fingerprint != target_fingerprint {
                bail!("persisted Tester execution target does not match current exact target");
            }
            if let Some(observation) = &record.observation {
                observations.push(observation.clone());
            }
            steps.insert(record.execution_id.clone(), record);
        }
        let execution_count = steps.len();
        Ok(Self {
            registry,
            project_root,
            lease_owner,
            graph_version,
            checkpoint_id: checkpoint_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            target,
            workspace,
            runner,
            execution_count,
            observations,
            steps,
        })
    }

    pub fn workspace(&self) -> &TesterWorkspaceRuntime {
        &self.workspace
    }

    pub fn resume_prepared(&mut self) -> Result<Vec<TesterExecutionObservation>> {
        let pending = self
            .steps
            .values()
            .filter(|record| record.status == "PREPARED")
            .cloned()
            .collect::<Vec<_>>();
        let mut resumed = Vec::new();
        for record in pending {
            resumed.push(self.resume_existing_step(record)?);
        }
        Ok(resumed)
    }

    pub fn execute(&mut self, arguments: &Value) -> Result<Value> {
        let request: TesterExecutionStepRequest = serde_json::from_value(arguments.clone())
            .context("invalid tester_execute arguments")?;
        request.validate()?;

        let target_fingerprint = self
            .target
            .fingerprint(self.graph_version, &self.checkpoint_id)?;
        let fence_key = request.fence_key(
            self.graph_version,
            &self.checkpoint_id,
            &self.attempt_id,
            &target_fingerprint,
        )?;
        let execution_id = fence_key.clone();

        if let Some(existing) = self.steps.get(&execution_id).cloned() {
            if existing.target_fingerprint != target_fingerprint || existing.request != request {
                bail!("persisted Tester execution fence conflicts with the requested step");
            }
            if let Some(observation) = existing.observation {
                return Ok(serde_json::to_value(observation)?);
            }
            if existing.status == "PREPARED" {
                let observation = self.resume_existing_step(existing)?;
                return Ok(serde_json::to_value(observation)?);
            }
            bail!("persisted Tester execution has unresolved status {}", existing.status);
        }

        if self.execution_count >= MAX_TESTER_EXECUTIONS {
            bail!("Tester exceeded bounded execution-step limit");
        }

        self.registry.prepare_tester_execution_step(
            self.project_root,
            self.lease_owner,
            self.graph_version,
            &self.checkpoint_id,
            &self.attempt_id,
            &self.target,
            &execution_id,
            &request.step_id,
            request.adapter.adapter_id(),
            request.replay_safety,
            &fence_key,
            &serde_json::to_value(&request)?,
        )?;
        self.execution_count += 1;

        let observation = self.observe_request(execution_id.clone(), &request);
        self.registry.complete_tester_execution_step(
            self.project_root,
            self.lease_owner,
            self.graph_version,
            &self.checkpoint_id,
            &self.attempt_id,
            &observation,
        )?;
        self.observations.push(observation.clone());
        self.steps.insert(
            execution_id.clone(),
            TesterExecutionStepRecord {
                execution_id,
                target_fingerprint,
                request,
                status: observation.status.as_str().to_owned(),
                observation: Some(observation.clone()),
            },
        );
        Ok(serde_json::to_value(observation)?)
    }

    fn resume_existing_step(
        &mut self,
        record: TesterExecutionStepRecord,
    ) -> Result<TesterExecutionObservation> {
        if record.status != "PREPARED" || record.observation.is_some() {
            bail!("Tester resume requires an unresolved PREPARED execution step");
        }

        if record.request.replay_safety == ReplaySafety::NonIdempotent {
            let reason = format!(
                "uncertain NON_IDEMPOTENT Tester execution {} cannot be auto-replayed",
                record.execution_id
            );
            self.registry.mark_tester_recovery_required(
                self.project_root,
                self.lease_owner,
                self.graph_version,
                &self.checkpoint_id,
                &self.attempt_id,
                &record.execution_id,
                &reason,
            )?;
            bail!("NEEDS_HUMAN: {reason}");
        }

        let observation =
            self.observe_request(record.execution_id.clone(), &record.request);
        self.registry.complete_tester_execution_step(
            self.project_root,
            self.lease_owner,
            self.graph_version,
            &self.checkpoint_id,
            &self.attempt_id,
            &observation,
        )?;
        self.observations.push(observation.clone());
        self.steps.insert(
            record.execution_id.clone(),
            TesterExecutionStepRecord {
                status: observation.status.as_str().to_owned(),
                observation: Some(observation.clone()),
                ..record
            },
        );
        Ok(observation)
    }

    fn observe_request(
        &self,
        execution_id: String,
        request: &TesterExecutionStepRequest,
    ) -> TesterExecutionObservation {
        let observation_result = match &request.adapter {
            TesterAdapterRequest::WorkspacePython { script_path, args } => {
                self.run_workspace_script(request, "python3", script_path, args)
            }
            TesterAdapterRequest::WorkspaceNode { script_path, args } => {
                self.run_workspace_script(request, "node", script_path, args)
            }
            TesterAdapterRequest::ProjectVerification {
                jobpack_id,
                change_set_id,
            } => self.run_project_verification(request, jobpack_id, change_set_id),
        };
        match observation_result {
            Ok(observation) => observation,
            Err(error) => TesterExecutionObservation::blocked(
                execution_id,
                request,
                format!("Tester adapter execution blocked: {error:#}"),
            ),
        }
    }

    fn has_completed_execution(&self) -> bool {
        self.observations
            .iter()
            .any(|observation| observation.status == TesterExecutionStatus::Completed)
    }

    fn has_unresolved_execution(&self) -> Result<bool> {
        self.registry.has_unresolved_tester_execution_steps(
            self.graph_version,
            &self.checkpoint_id,
            &self.attempt_id,
        )
    }

    fn run_workspace_script(
        &self,
        request: &TesterExecutionStepRequest,
        interpreter: &str,
        script_path: &str,
        args: &[String],
    ) -> Result<TesterExecutionObservation> {
        let artifact = self.workspace.artifact_ref(script_path)?;
        let mut argv = vec![interpreter.to_owned(), artifact.path.clone()];
        argv.extend(args.iter().cloned());
        let execution_id = request.fence_key(
            self.graph_version,
            &self.checkpoint_id,
            &self.attempt_id,
            &self
                .target
                .fingerprint(self.graph_version, &self.checkpoint_id)?,
        )?;
        let process = self
            .runner
            .run(self.workspace.root(), &argv, DEFAULT_TESTER_TIMEOUT)?;
        Ok(TesterExecutionObservation::from_process(
            execution_id,
            request,
            process,
        ))
    }

    fn run_project_verification(
        &self,
        request: &TesterExecutionStepRequest,
        jobpack_id: &str,
        change_set_id: &str,
    ) -> Result<TesterExecutionObservation> {
        let target_matches = self.target.prerequisites.iter().any(|item| {
            item.jobpack_id == jobpack_id && item.change_set_id.as_deref() == Some(change_set_id)
        });
        if !target_matches {
            bail!("PROJECT_VERIFICATION target is not part of the Tester target binding");
        }

        let result =
            VerificationController::new(self.registry, self.project_root, self.lease_owner)
                .verify(self.graph_version, jobpack_id, change_set_id, &[])?;
        let run_id = self
            .registry
            .latest_verification_run_id(self.graph_version, jobpack_id, change_set_id)?
            .context("project verification did not persist a verification run")?;
        let evidence = self
            .registry
            .verification_run_evidence(run_id)?
            .context("project verification run evidence is missing")?;

        let mut evidence_refs = Vec::new();
        for command in &evidence.commands {
            if !command.stdout.is_empty() {
                evidence_refs.push(TesterEvidenceRef::VerificationObservation {
                    run_id,
                    command_id: command.command_id.clone(),
                    field: VerificationObservationField::Stdout,
                    observed: crate::tester_evidence::ObservedValue::Text(command.stdout.clone()),
                });
            }
            if !command.stderr.is_empty() {
                evidence_refs.push(TesterEvidenceRef::VerificationObservation {
                    run_id,
                    command_id: command.command_id.clone(),
                    field: VerificationObservationField::Stderr,
                    observed: crate::tester_evidence::ObservedValue::Text(command.stderr.clone()),
                });
            }
            if let Some(code) = command.exit_code {
                evidence_refs.push(TesterEvidenceRef::VerificationObservation {
                    run_id,
                    command_id: command.command_id.clone(),
                    field: VerificationObservationField::ExitCode,
                    observed: crate::tester_evidence::ObservedValue::Integer(i64::from(code)),
                });
            }
        }

        let (status, blocked_reason) = match result {
            VerificationResult::TestPass => (TesterExecutionStatus::Completed, None),
            VerificationResult::Fail => (TesterExecutionStatus::Failed, None),
            VerificationResult::Blocked => (
                TesterExecutionStatus::Blocked,
                Some("deterministic project verification is blocked".into()),
            ),
            VerificationResult::NotApplicable => (
                TesterExecutionStatus::Blocked,
                Some("deterministic project verification is not applicable".into()),
            ),
        };

        Ok(TesterExecutionObservation {
            execution_id: request.fence_key(
                self.graph_version,
                &self.checkpoint_id,
                &self.attempt_id,
                &self
                    .target
                    .fingerprint(self.graph_version, &self.checkpoint_id)?,
            )?,
            step_id: request.step_id.clone(),
            adapter_id: request.adapter.adapter_id().into(),
            replay_safety: request.replay_safety,
            fence_key: request.fence_key(
                self.graph_version,
                &self.checkpoint_id,
                &self.attempt_id,
                &self
                    .target
                    .fingerprint(self.graph_version, &self.checkpoint_id)?,
            )?,
            status,
            exit_code: None,
            duration_ms: evidence.commands.iter().map(|item| item.duration_ms).sum(),
            timed_out: evidence.commands.iter().any(|item| item.timed_out),
            blocked_reason,
            stdout: String::new(),
            stderr: String::new(),
            evidence_refs,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TesterReportSubmission {
    pub mode_results: Vec<TesterModeResult>,
    #[serde(default)]
    pub classifications: Vec<TesterClassification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment: Option<ExperimentContext>,
    #[serde(default)]
    pub outputs: Vec<TesterEvidenceOutputRecord>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

pub struct TesterWorkflow<'a> {
    ollama: &'a OllamaClient,
    harnesses: &'a HarnessRegistry,
    registry: &'a Registry,
    config: &'a AppConfig,
    session: &'a Session,
    project_root: &'a Path,
    lease_owner: &'a str,
}

impl<'a> TesterWorkflow<'a> {
    pub fn new(
        ollama: &'a OllamaClient,
        harnesses: &'a HarnessRegistry,
        registry: &'a Registry,
        config: &'a AppConfig,
        session: &'a Session,
        project_root: &'a Path,
        lease_owner: &'a str,
    ) -> Self {
        Self {
            ollama,
            harnesses,
            registry,
            config,
            session,
            project_root,
            lease_owner,
        }
    }

    pub async fn run(
        &self,
        requirement: &str,
        graph_version: i64,
        checkpoint: &TestCheckpointSpec,
        target: TesterTargetBinding,
        attempt_id: &str,
        project_tools: &mut ProjectToolRuntime,
    ) -> Result<TesterAttemptEvidence> {
        let plan = self
            .registry
            .plan_for_current_execution_graph(graph_version)?;
        let evidence_needs = plan
            .artifact
            .evidence_needs
            .iter()
            .filter(|need| checkpoint.evidence_need_ids.iter().any(|id| id == &need.id))
            .cloned()
            .collect::<Vec<EvidenceNeed>>();

        let mut execution = TesterExecutionRuntime::new(
            self.registry,
            self.project_root,
            self.lease_owner,
            graph_version,
            &checkpoint.id,
            attempt_id,
            target.clone(),
        )?;
        let resumed_observations = execution.resume_prepared()?;
        let resume_state = self.registry.latest_tester_resume_state()?;

        let model = self.model_for(AgentId::Tester).await?;
        let mut system = self.harnesses.compose(AgentId::Tester)?;
        system.push_str(&format!(
            "\n\nPROJECT ROOT: {}\nTESTER WORKSPACE: {}\nUse project tools only for read-only product inspection. Author tests only with Tester workspace tools. Execute only through tester_execute fixed adapters. submit_tester_report must be the only tool call in the final response.",
            self.project_root.display(),
            execution.workspace().root().display()
        ));
        let packet = json!({
            "original_requirement": requirement,
            "graph_version": graph_version,
            "checkpoint": checkpoint,
            "target": target,
            "evidence_needs": evidence_needs,
            "resume_state": resume_state,
            "resumed_execution_observations": resumed_observations,
            "instruction": "Independently ground on the checkpoint contract and persisted resume state. Reuse completed execution evidence, do not duplicate completed steps, plan any remaining tests/experiments, author Tester-owned artifacts if needed, execute through fixed adapters, analyze/adapt within checkpoint scope, then submit a structured report. Do not invent runtime facts or thresholds."
        });
        let mut messages = vec![
            ChatMessage::system(system),
            ChatMessage::user(packet.to_string()),
        ];
        let mut definitions = project_tools.tool_definitions(AgentId::Tester);
        definitions.extend(execution.workspace().tool_definitions(AgentId::Tester));
        definitions.push(tester_execute_tool());
        definitions.push(tester_report_tool());

        for round in 0..MAX_TESTER_TOOL_ROUNDS {
            let response = self
                .ollama
                .chat_stream_with_tools(&model, &messages, &definitions, |_| {})
                .await?;
            let calls = response.tool_calls.clone();
            messages.push(response);

            if calls.is_empty() {
                bail!("Tester stopped without submit_tester_report");
            }
            if calls
                .iter()
                .any(|call| call.function.name == "submit_tester_report")
            {
                if let Err(error) =
                    validate_report_call_shape(calls.iter().map(|call| call.function.name.as_str()))
                {
                    if should_retry_invalid_report(round) {
                        messages.push(invalid_report_feedback(&error));
                        continue;
                    }
                    return Err(error)
                        .context("Tester report remained invalid after bounded retries");
                }
                let report_result = (|| -> Result<TesterAttemptEvidence> {
                    let report: TesterReportSubmission =
                        serde_json::from_value(calls[0].function.arguments.clone())
                            .context("invalid submit_tester_report arguments")?;
                    validate_report_execution_coverage(
                        &report,
                        execution.has_completed_execution(),
                        execution.has_unresolved_execution()?,
                    )?;
                    let attempt = TesterAttemptEvidence {
                        graph_version,
                        checkpoint_id: checkpoint.id.clone(),
                        attempt_id: attempt_id.to_owned(),
                        target: target.clone(),
                        mode_results: report.mode_results,
                        classifications: report.classifications,
                        experiment: report.experiment,
                        outputs: report.outputs,
                        limitations: report.limitations,
                    };
                    attempt.validate_against_checkpoint(checkpoint)?;
                    self.registry.record_tester_attempt_evidence(
                        self.project_root,
                        self.lease_owner,
                        &attempt,
                    )?;
                    Ok(attempt)
                })();

                match report_result {
                    Ok(attempt) => return Ok(attempt),
                    Err(error) if should_retry_invalid_report(round) => {
                        messages.push(invalid_report_feedback(&error));
                        continue;
                    }
                    Err(error) => {
                        return Err(error)
                            .context("Tester report remained invalid after bounded retries");
                    }
                }
            }

            if round + 1 >= MAX_TESTER_TOOL_ROUNDS {
                bail!("Tester exceeded bounded tool rounds");
            }
            for call in calls {
                messages.push(self.execute_tool(project_tools, &mut execution, &call));
            }
        }
        unreachable!("bounded Tester loop must return or fail")
    }

    fn execute_tool(
        &self,
        project_tools: &mut ProjectToolRuntime,
        execution: &mut TesterExecutionRuntime<'_>,
        call: &ToolCall,
    ) -> ChatMessage {
        let name = call.function.name.as_str();
        let result = if name.starts_with("project_") {
            project_tools.execute(AgentId::Tester, name, &call.function.arguments)
        } else if name.starts_with("tester_workspace_") {
            execution
                .workspace()
                .execute(AgentId::Tester, name, &call.function.arguments)
        } else if name == "tester_execute" {
            execution.execute(&call.function.arguments)
        } else {
            Err(anyhow::anyhow!("unsupported Tester tool {name}"))
        };

        match result {
            Ok(value) => ChatMessage::tool(name, json!({"ok":true,"result":value}).to_string()),
            Err(error) => ChatMessage::tool(
                name,
                json!({"ok":false,"error":format!("{error:#}")}).to_string(),
            ),
        }
    }

    async fn model_for(&self, agent: AgentId) -> Result<String> {
        if let Some(model) = self.session.resolved_model(self.config, agent) {
            return Ok(model.to_owned());
        }
        self.ollama
            .list_models()
            .await?
            .into_iter()
            .next()
            .context("no Ollama model is configured or installed")
    }
}

fn validate_report_execution_coverage(
    report: &TesterReportSubmission,
    has_completed_execution: bool,
    has_unresolved_execution: bool,
) -> Result<()> {
    let claims_success = report
        .mode_results
        .iter()
        .any(TesterModeResult::satisfies_output_requirement);
    if claims_success && has_unresolved_execution {
        bail!("Tester cannot report PASS/COMPLETE while execution steps remain PREPARED");
    }
    if claims_success && !has_completed_execution {
        bail!("Tester cannot report PASS/COMPLETE without at least one completed execution step");
    }
    Ok(())
}

fn validate_report_call_shape<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let names = names.into_iter().collect::<Vec<_>>();
    if names.len() != 1 || names[0] != "submit_tester_report" {
        bail!("submit_tester_report must be the only tool call in its response");
    }
    Ok(())
}

fn should_retry_invalid_report(round: usize) -> bool {
    round + 1 < MAX_TESTER_TOOL_ROUNDS
}

fn invalid_report_feedback(error: &anyhow::Error) -> ChatMessage {
    ChatMessage::tool(
        "submit_tester_report",
        json!({
            "ok": false,
            "error": format!("{error:#}"),
            "instruction": "Correct the report or gather missing evidence inside the same checkpoint scope, then resubmit."
        })
        .to_string(),
    )
}

fn tester_execute_tool() -> ToolDefinition {
    ToolDefinition::function(
        "tester_execute",
        "Execute one bounded Tester step through a fixed adapter. Shell commands and arbitrary executables are not accepted.",
        json!({
            "type":"object",
            "required":["step_id","replay_safety","adapter"],
            "properties":{
                "step_id":{"type":"string","minLength":1},
                "replay_safety":{"type":"string","enum":["OBSERVE_ONLY","IDEMPOTENT","NON_IDEMPOTENT"]},
                "adapter":{"type":"string","enum":["WORKSPACE_PYTHON","WORKSPACE_NODE","PROJECT_VERIFICATION"]},
                "script_path":{"type":"string"},
                "args":{"type":"array","items":{"type":"string"}},
                "jobpack_id":{"type":"string"},
                "change_set_id":{"type":"string"}
            }
        }),
    )
}

fn tester_report_tool() -> ToolDefinition {
    ToolDefinition::function(
        "submit_tester_report",
        "Submit the complete exact-target Tester report. Copy runtime evidence refs returned by tester_execute; do not invent ids or observed values. Runtime validates mode outcomes, provenance, evidence refs and applicability before persistence.",
        json!({
            "type":"object",
            "required":["mode_results","classifications","outputs","limitations"],
            "properties":{
                "mode_results":{
                    "type":"array",
                    "items":{
                        "type":"object",
                        "required":["mode","outcome"],
                        "properties":{
                            "mode":{"type":"string","enum":["VERIFY","MEASURE","PROBE"]},
                            "outcome":{"type":"string","enum":["PASS","FAIL","COMPLETE","BLOCKED","NEEDS_HUMAN"]},
                            "reason":{"type":["string","null"]}
                        }
                    }
                },
                "classifications":{
                    "type":"array",
                    "items":{"type":"string","enum":["PRODUCT_FAILURE","TEST_FAILURE","ENVIRONMENT_FAILURE","NOT_READY","INTEGRATION_NOT_READY","SPEC_GAP"]}
                },
                "experiment":{
                    "description":"For completed MEASURE/PROBE, provide dimensions and samples. Each sample contains variables, boundary_event/target/runtime identity when observed, and observations with copied runtime evidence_refs.",
                    "type":["object","null"]
                },
                "outputs":{
                    "type":"array",
                    "description":"Named checkpoint outputs. OBSERVED outputs require value plus runtime-backed evidence_refs returned by tester_execute.",
                    "items":{
                        "type":"object",
                        "required":["output_id","mode","provenance","evidence_refs","limitations","applicability"],
                        "properties":{
                            "output_id":{"type":"string","minLength":1},
                            "mode":{"type":"string","enum":["VERIFY","MEASURE","PROBE"]},
                            "provenance":{"type":"string","enum":["OBSERVED","IMPLICATION","UNRESOLVED"]},
                            "value":{"type":["object","null"],"description":"ObservedValue uses {type: TEXT|INTEGER|DECIMAL|BOOLEAN, value: ...}."},
                            "unit":{"type":["string","null"]},
                            "evidence_refs":{"type":"array","items":{"type":"object"},"description":"Copy exact refs returned by tester_execute or other runtime-validated refs."},
                            "limitations":{"type":"array","items":{"type":"string"}},
                            "applicability":{"type":"object","description":"Provide policy REUSE_IF_MATCHES or ALWAYS_REVALIDATE plus finite matcher objects defined by the approved evidence contract."}
                        }
                    }
                },
                "limitations":{"type":"array","items":{"type":"string"}}
            }
        }),
    )
}

fn validate_test_script_path(path: &str) -> Result<()> {
    require_text("Tester script_path", path)?;
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.split('/').any(|part| part == "..")
        || !normalized.starts_with("tests/")
    {
        bail!("Tester executable script must be a relative path under tests/");
    }
    Ok(())
}

fn validate_args(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg.contains('\0')) {
        bail!("Tester adapter args must not contain NUL");
    }
    Ok(())
}

fn require_text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{name} must not be empty");
    }
    Ok(())
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    let bytes = digest.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_verification_is_observe_only() {
        let request = TesterExecutionStepRequest {
            step_id: "verify".into(),
            replay_safety: ReplaySafety::Idempotent,
            adapter: TesterAdapterRequest::ProjectVerification {
                jobpack_id: "JP1".into(),
                change_set_id: "change-1".into(),
            },
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn fixed_workspace_adapters_reject_shell_like_script_paths() {
        for path in [
            "/tmp/test.py",
            "../test.py",
            "tests/../escape.py",
            "src/test.py",
        ] {
            let request = TesterExecutionStepRequest {
                step_id: "run".into(),
                replay_safety: ReplaySafety::Idempotent,
                adapter: TesterAdapterRequest::WorkspacePython {
                    script_path: path.into(),
                    args: vec![],
                },
            };
            assert!(request.validate().is_err(), "{path}");
        }
    }

    #[test]
    fn success_requires_a_completed_execution() {
        let successful = TesterReportSubmission {
            mode_results: vec![TesterModeResult {
                mode: crate::plan::EvidenceMode::Verify,
                outcome: crate::tester_evidence::TesterModeOutcome::Pass,
                reason: None,
            }],
            classifications: vec![],
            experiment: None,
            outputs: vec![],
            limitations: vec![],
        };
        assert!(validate_report_execution_coverage(&successful, false, false).is_err());
        assert!(validate_report_execution_coverage(&successful, true, false).is_ok());
        assert!(validate_report_execution_coverage(&successful, true, true).is_err());

        let blocked = TesterReportSubmission {
            mode_results: vec![TesterModeResult {
                mode: crate::plan::EvidenceMode::Verify,
                outcome: crate::tester_evidence::TesterModeOutcome::Blocked,
                reason: Some("sandbox unavailable".into()),
            }],
            classifications: vec![TesterClassification::EnvironmentFailure],
            experiment: None,
            outputs: vec![],
            limitations: vec![],
        };
        assert!(validate_report_execution_coverage(&blocked, false, true).is_ok());
    }

    #[test]
    fn invalid_report_correction_is_bounded() {
        assert!(should_retry_invalid_report(0));
        assert!(should_retry_invalid_report(MAX_TESTER_TOOL_ROUNDS - 2));
        assert!(!should_retry_invalid_report(MAX_TESTER_TOOL_ROUNDS - 1));
    }

    #[test]
    fn mixed_report_and_tool_calls_are_invalid_report_shape() {
        assert!(validate_report_call_shape(["submit_tester_report"]).is_ok());
        assert!(
            validate_report_call_shape(["submit_tester_report", "tester_workspace_read",]).is_err()
        );
        assert!(
            validate_report_call_shape(["tester_workspace_read", "submit_tester_report",]).is_err()
        );
    }

    #[test]
    fn fence_key_is_deterministic_and_request_bound() {
        let request = TesterExecutionStepRequest {
            step_id: "probe".into(),
            replay_safety: ReplaySafety::ObserveOnly,
            adapter: TesterAdapterRequest::WorkspacePython {
                script_path: "tests/probe.py".into(),
                args: vec!["a".into()],
            },
        };
        let first = request.fence_key(1, "CP1", "ATT1", "target").unwrap();
        let second = request.fence_key(1, "CP1", "ATT1", "target").unwrap();
        assert_eq!(first, second);
        let mut changed = request.clone();
        if let TesterAdapterRequest::WorkspacePython { args, .. } = &mut changed.adapter {
            args.push("b".into());
        }
        assert_ne!(
            first,
            changed.fence_key(1, "CP1", "ATT1", "target").unwrap()
        );
    }
}
