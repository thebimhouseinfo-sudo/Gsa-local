use crate::{
    config::AppConfig,
    controller::ActiveWork,
    execution_graph::ExecutionGraph,
    harness::{AgentId, HarnessRegistry},
    ollama::{ChatMessage, OllamaClient, ToolCall, ToolDefinition},
    plan::{PlanArtifact, PlanRevision},
    registry::{ChecklistClaim, CodeTodoState, PlanBinding, Registry, ReviewActor, ReviewVerdict},
    session::Session,
    tools::ProjectToolRuntime,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{fs, io::Read, path::Path};

const DEFAULT_MAX_ATTEMPTS: u32 = 5;
const MAX_CONTEXT_PATHS: usize = 500;
const MAX_CONTEXT_FILES: usize = 200;
const MAX_CONTEXT_BYTES: usize = 128 * 1024;
const MAX_FILE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodingOutcome {
    ReviewPass {
        change_set_id: String,
    },
    Paused {
        change_set_id: Option<String>,
        reason: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct CodeCheckpointSubmission {
    summary: String,
    completed_checklist: Vec<ChecklistClaim>,
    goal_recheck: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CodeReviewDecision {
    verdict: String,
    findings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanningStage {
    Planner,
    Reviewer,
    InternalFix,
    LocalCr,
    Approved,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanningOutcome {
    Registered {
        plan: PlanBinding,
        graph_version: i64,
    },
    Paused {
        stage: PlanningStage,
        revision: Option<i64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningRoute {
    pub stage: PlanningStage,
    pub reviewer_attempts: u32,
    pub cr_attempts: u32,
    pub max_attempts: u32,
}

impl PlanningRoute {
    pub fn new(max_attempts: u32) -> Self {
        Self {
            stage: PlanningStage::Planner,
            reviewer_attempts: 0,
            cr_attempts: 0,
            max_attempts,
        }
    }

    pub fn enter_reviewer(&mut self) -> bool {
        if self.reviewer_attempts >= self.max_attempts {
            self.stage = PlanningStage::Paused;
            return false;
        }
        self.reviewer_attempts += 1;
        self.stage = PlanningStage::Reviewer;
        true
    }

    pub fn reviewer_result(&mut self, verdict: ReviewVerdict) {
        self.stage = match verdict {
            ReviewVerdict::Pass => PlanningStage::LocalCr,
            ReviewVerdict::Revise => PlanningStage::Planner,
        };
    }

    pub fn enter_cr(&mut self) -> bool {
        if self.cr_attempts >= self.max_attempts {
            self.stage = PlanningStage::Paused;
            return false;
        }
        self.cr_attempts += 1;
        self.stage = PlanningStage::LocalCr;
        true
    }

    pub fn cr_result(&mut self, verdict: ReviewVerdict) {
        self.stage = match verdict {
            ReviewVerdict::Pass => PlanningStage::Approved,
            ReviewVerdict::Revise => PlanningStage::InternalFix,
        };
    }

    pub fn after_internal_fix(&mut self) {
        self.stage = PlanningStage::Reviewer;
    }

    pub fn unchanged_revision(&mut self) {
        self.stage = PlanningStage::Paused;
    }
}

#[derive(Debug, Clone, Serialize)]
struct ProjectContext {
    paths: Vec<String>,
    text_files: Vec<ProjectContextFile>,
    truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ProjectContextFile {
    path: String,
    content: String,
    truncated: bool,
}

pub struct PlanningWorkflow<'a> {
    ollama: &'a OllamaClient,
    harnesses: &'a HarnessRegistry,
    registry: &'a Registry,
    config: &'a AppConfig,
    session: &'a Session,
    project_root: &'a Path,
    max_attempts: u32,
}

impl<'a> PlanningWorkflow<'a> {
    pub fn new(
        ollama: &'a OllamaClient,
        harnesses: &'a HarnessRegistry,
        registry: &'a Registry,
        config: &'a AppConfig,
        session: &'a Session,
        project_root: &'a Path,
    ) -> Self {
        Self {
            ollama,
            harnesses,
            registry,
            config,
            session,
            project_root,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
        }
    }

    #[cfg(test)]
    pub fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    pub async fn run(&self, requirement: &str) -> Result<PlanningOutcome> {
        self.registry.begin_plan_workflow()?;
        let mut route = PlanningRoute::new(self.max_attempts);
        let project_context = build_project_context(self.project_root)?;

        let artifact = self
            .invoke_plan_agent(
                AgentId::Planner,
                requirement,
                None,
                &[],
                &project_context,
                "Create the first Implementation Plan revision.",
            )
            .await?;
        let mut current = self.registry.persist_plan_revision(&artifact)?;

        loop {
            if !route.enter_reviewer() {
                return self.pause(&route, Some(current.revision));
            }
            self.persist_route(&route, Some(current.revision))?;

            let review = self
                .invoke_reviewer(requirement, &current, &project_context)
                .await?;
            let review_verdict = if review.verdict == "PASS" {
                ReviewVerdict::Pass
            } else if review.verdict == "CHANGES_REQUIRED" {
                ReviewVerdict::Revise
            } else {
                bail!("Reviewer returned unsupported verdict {}", review.verdict);
            };
            self.registry.record_plan_verdict(
                ReviewActor::Reviewer,
                current.revision,
                &current.hash,
                review_verdict,
                &review.findings,
            )?;
            route.reviewer_result(review_verdict);

            if review_verdict == ReviewVerdict::Revise {
                if route.reviewer_attempts >= route.max_attempts {
                    return self.pause(&route, Some(current.revision));
                }
                let revised = self
                    .invoke_plan_agent(
                        AgentId::Planner,
                        requirement,
                        Some(&current),
                        &review.findings,
                        &project_context,
                        "Revise the plan to resolve Reviewer findings. Produce a new complete plan.",
                    )
                    .await?;
                if revised.hash()? == current.hash {
                    route.unchanged_revision();
                    return self.pause(&route, Some(current.revision));
                }
                current = self.registry.persist_plan_revision(&revised)?;
                continue;
            }

            if !route.enter_cr() {
                return self.pause(&route, Some(current.revision));
            }
            self.persist_route(&route, Some(current.revision))?;

            let cr = self
                .invoke_cr(requirement, &current, &project_context)
                .await?;
            let cr_verdict = if cr.verdict == "PASS" {
                ReviewVerdict::Pass
            } else if cr.verdict == "REVISE" {
                ReviewVerdict::Revise
            } else {
                bail!("Local CR returned unsupported verdict {}", cr.verdict);
            };
            self.registry.record_plan_verdict(
                ReviewActor::LocalCr,
                current.revision,
                &current.hash,
                cr_verdict,
                &cr.findings,
            )?;
            route.cr_result(cr_verdict);

            if cr_verdict == ReviewVerdict::Pass {
                let binding = self
                    .registry
                    .approve_current_plan(current.revision, &current.hash)?;
                self.registry.set_workflow_state(
                    Some(current.revision),
                    route.reviewer_attempts,
                    route.cr_attempts,
                    "JOB_BUILDER",
                )?;
                let graph = self
                    .invoke_job_builder(requirement, &current, &project_context)
                    .await?;
                let graph_version = self.registry.register_execution_graph(
                    binding.revision,
                    &binding.hash,
                    &graph,
                )?;
                return Ok(PlanningOutcome::Registered {
                    plan: binding,
                    graph_version,
                });
            }

            if route.cr_attempts >= route.max_attempts {
                return self.pause(&route, Some(current.revision));
            }

            self.persist_route(&route, Some(current.revision))?;
            let fixed = self
                .invoke_plan_agent(
                    AgentId::InternalFix,
                    requirement,
                    Some(&current),
                    &cr.findings,
                    &project_context,
                    "Repair only the CR findings inside the existing planning scope. Return a complete revised plan.",
                )
                .await?;
            if fixed.hash()? == current.hash {
                route.unchanged_revision();
                return self.pause(&route, Some(current.revision));
            }
            current = self.registry.persist_plan_revision(&fixed)?;
            route.after_internal_fix();
        }
    }

    fn pause(&self, route: &PlanningRoute, revision: Option<i64>) -> Result<PlanningOutcome> {
        self.registry.set_workflow_state(
            revision,
            route.reviewer_attempts,
            route.cr_attempts,
            "PAUSED",
        )?;
        Ok(PlanningOutcome::Paused {
            stage: PlanningStage::Paused,
            revision,
        })
    }

    fn persist_route(&self, route: &PlanningRoute, revision: Option<i64>) -> Result<()> {
        self.registry.set_workflow_state(
            revision,
            route.reviewer_attempts,
            route.cr_attempts,
            stage_name(route.stage),
        )
    }

    async fn invoke_plan_agent(
        &self,
        agent: AgentId,
        requirement: &str,
        current: Option<&PlanRevision>,
        findings: &[String],
        project_context: &ProjectContext,
        instruction: &str,
    ) -> Result<PlanArtifact> {
        let mut packet = json!({
            "original_requirement": requirement,
            "instruction": instruction,
            "project_root": self.project_root.display().to_string(),
            "project_context": project_context,
            "findings": findings,
        });
        if let Some(current) = current {
            packet["current_plan"] = serde_json::to_value(current)?;
        }

        let response = self
            .invoke_with_tool(
                agent,
                vec![ChatMessage::user(packet.to_string())],
                plan_tool(),
            )
            .await?;
        extract_tool_args(&response, "submit_plan")
    }

    async fn invoke_reviewer(
        &self,
        requirement: &str,
        current: &PlanRevision,
        project_context: &ProjectContext,
    ) -> Result<ReviewDecision> {
        let packet = json!({
            "original_requirement": requirement,
            "project_context": project_context,
            "target": {
                "revision": current.revision,
                "hash": current.hash,
                "plan": current.artifact
            },
            "instruction": "Review only this exact plan revision. Return PASS or CHANGES_REQUIRED with actionable findings."
        });
        let response = self
            .invoke_with_tool(
                AgentId::Reviewer,
                vec![ChatMessage::user(packet.to_string())],
                review_tool(),
            )
            .await?;
        extract_tool_args(&response, "submit_review")
    }

    async fn invoke_cr(
        &self,
        requirement: &str,
        current: &PlanRevision,
        project_context: &ProjectContext,
    ) -> Result<CrDecision> {
        let packet = json!({
            "original_requirement": requirement,
            "project_context": project_context,
            "target": {
                "revision": current.revision,
                "hash": current.hash,
                "plan": current.artifact
            },
            "instruction": "Form an independent verdict from this clean packet. Do not assume Reviewer PASS. Return PASS or REVISE."
        });
        let response = self
            .invoke_with_tool(
                AgentId::LocalCr,
                vec![ChatMessage::user(packet.to_string())],
                cr_tool(),
            )
            .await?;
        extract_tool_args(&response, "submit_cr_review")
    }

    async fn invoke_job_builder(
        &self,
        requirement: &str,
        current: &PlanRevision,
        project_context: &ProjectContext,
    ) -> Result<ExecutionGraph> {
        let packet = json!({
            "original_requirement": requirement,
            "project_context": project_context,
            "approved_plan": {
                "revision": current.revision,
                "hash": current.hash,
                "plan": current.artifact
            },
            "instruction": "Decompose this approved plan only. Do not redesign it. Return READY with a complete execution graph, or PLAN_GAP with explicit findings if the approved plan is insufficient."
        });
        let response = self
            .invoke_with_tool(
                AgentId::JobBuilder,
                vec![ChatMessage::user(packet.to_string())],
                execution_graph_tool(),
            )
            .await?;
        let submission: JobBuilderSubmission =
            extract_tool_args(&response, "submit_execution_graph")?;

        match submission.status.as_str() {
            "READY" => {
                if !submission.gap_findings.is_empty() {
                    bail!("Job Builder READY submission cannot include PLAN_GAP findings");
                }
                let graph = submission
                    .graph
                    .context("Job Builder READY submission is missing graph")?;
                graph.validate()?;
                Ok(graph)
            }
            "PLAN_GAP" => {
                if submission.gap_findings.is_empty() {
                    bail!("Job Builder PLAN_GAP must include at least one finding");
                }
                bail!(
                    "Job Builder PLAN_GAP: {}",
                    submission.gap_findings.join("; ")
                )
            }
            other => bail!("Job Builder returned unsupported status {other}"),
        }
    }

    async fn invoke_with_tool(
        &self,
        agent: AgentId,
        mut messages: Vec<ChatMessage>,
        tool: ToolDefinition,
    ) -> Result<ChatMessage> {
        let model = self.model_for(agent).await?;
        let mut system = self.harnesses.compose(agent)?;
        system.push_str(&format!(
            "\n\nPROJECT ROOT: {}\nUse the provided function tool to submit the structured result. Do not claim completion without calling it.",
            self.project_root.display()
        ));
        messages.insert(0, ChatMessage::system(system));

        let response = self
            .ollama
            .chat_stream_with_tools(&model, &messages, &[tool], |_| {})
            .await?;
        if response.tool_calls.is_empty() {
            bail!(
                "{} did not call the required workflow tool",
                agent.display_name()
            );
        }
        Ok(response)
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

pub struct CodingWorkflow<'a> {
    ollama: &'a OllamaClient,
    harnesses: &'a HarnessRegistry,
    registry: &'a Registry,
    config: &'a AppConfig,
    session: &'a Session,
    project_root: &'a Path,
    lease_owner: &'a str,
    max_attempts: u32,
}

impl<'a> CodingWorkflow<'a> {
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
            max_attempts: DEFAULT_MAX_ATTEMPTS,
        }
    }

    #[cfg(test)]
    pub fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    pub async fn run(
        &self,
        requirement: &str,
        active_work: &ActiveWork,
        tool_runtime: &mut ProjectToolRuntime,
    ) -> Result<CodingOutcome> {
        self.registry.begin_code_workflow(
            self.project_root,
            self.lease_owner,
            active_work.graph_version,
            &active_work.jobpack_id,
        )?;
        tool_runtime.clear_journal();

        let mut coder_attempts = 1u32;
        let mut reviewer_attempts = 0u32;
        let mut tasks = self
            .registry
            .jobpack_code_tasks(active_work.graph_version, &active_work.jobpack_id)?;
        let mut checkpoint = self
            .invoke_code_agent(
                AgentId::Coder,
                requirement,
                active_work,
                &tasks,
                &[],
                None,
                tool_runtime,
            )
            .await?;
        let mut change_set_id = self.persist_code_checkpoint(
            active_work,
            &checkpoint,
            tool_runtime,
            coder_attempts,
            reviewer_attempts,
        )?;

        loop {
            if reviewer_attempts >= self.max_attempts {
                return self.pause(
                    active_work,
                    Some(change_set_id),
                    coder_attempts,
                    reviewer_attempts,
                    "Reviewer attempt limit exhausted",
                );
            }
            if let Err(error) = tool_runtime.verify_journal_current() {
                return self.pause(
                    active_work,
                    Some(change_set_id),
                    coder_attempts,
                    reviewer_attempts,
                    &format!("stale change set before review: {error:#}"),
                );
            }

            reviewer_attempts += 1;
            tasks = self
                .registry
                .jobpack_code_tasks(active_work.graph_version, &active_work.jobpack_id)?;
            let review = self
                .invoke_code_reviewer(
                    requirement,
                    active_work,
                    &tasks,
                    &checkpoint,
                    &change_set_id,
                    tool_runtime,
                )
                .await?;

            if let Err(error) = tool_runtime.verify_journal_current() {
                return self.pause(
                    active_work,
                    Some(change_set_id),
                    coder_attempts,
                    reviewer_attempts,
                    &format!("stale change set after review: {error:#}"),
                );
            }

            let verdict = match review.verdict.as_str() {
                "PASS" => ReviewVerdict::Pass,
                "CHANGES_REQUIRED" => ReviewVerdict::Revise,
                other => bail!("Reviewer returned unsupported code verdict {other}"),
            };
            self.registry.record_code_review(
                self.project_root,
                self.lease_owner,
                active_work.graph_version,
                &active_work.jobpack_id,
                &change_set_id,
                verdict,
                &review.findings,
                coder_attempts,
                reviewer_attempts,
            )?;

            if verdict == ReviewVerdict::Pass {
                return Ok(CodingOutcome::ReviewPass { change_set_id });
            }

            if coder_attempts >= self.max_attempts {
                return self.pause(
                    active_work,
                    Some(change_set_id),
                    coder_attempts,
                    reviewer_attempts,
                    "Internal Fix attempt limit exhausted",
                );
            }

            coder_attempts += 1;
            tasks = self
                .registry
                .jobpack_code_tasks(active_work.graph_version, &active_work.jobpack_id)?;
            let journal_len_before_fix = tool_runtime.journal().len();
            let previous_change_set_id = change_set_id.clone();
            checkpoint = self
                .invoke_code_agent(
                    AgentId::InternalFix,
                    requirement,
                    active_work,
                    &tasks,
                    &review.findings,
                    Some(&checkpoint),
                    tool_runtime,
                )
                .await?;
            let repaired_change_set_id = self.persist_code_checkpoint(
                active_work,
                &checkpoint,
                tool_runtime,
                coder_attempts,
                reviewer_attempts,
            )?;
            if !repair_progressed(
                journal_len_before_fix,
                tool_runtime.journal().len(),
                &previous_change_set_id,
                &repaired_change_set_id,
            ) {
                return self.pause(
                    active_work,
                    Some(previous_change_set_id),
                    coder_attempts,
                    reviewer_attempts,
                    "Internal Fix produced no new source mutation/change set",
                );
            }
            change_set_id = repaired_change_set_id;
        }
    }

    fn persist_code_checkpoint(
        &self,
        active_work: &ActiveWork,
        checkpoint: &CodeCheckpointSubmission,
        tool_runtime: &ProjectToolRuntime,
        coder_attempts: u32,
        reviewer_attempts: u32,
    ) -> Result<String> {
        tool_runtime.verify_journal_current()?;
        let change_set_id = tool_runtime.change_set_id()?;
        let journal = serde_json::to_value(tool_runtime.journal())?;
        self.registry.record_code_checkpoint(
            self.project_root,
            self.lease_owner,
            active_work.graph_version,
            &active_work.jobpack_id,
            &change_set_id,
            &checkpoint.summary,
            &checkpoint.completed_checklist,
            &checkpoint.goal_recheck,
            &journal,
            coder_attempts,
            reviewer_attempts,
        )?;
        Ok(change_set_id)
    }

    fn pause(
        &self,
        active_work: &ActiveWork,
        change_set_id: Option<String>,
        coder_attempts: u32,
        reviewer_attempts: u32,
        reason: &str,
    ) -> Result<CodingOutcome> {
        self.registry.pause_code_workflow(
            self.project_root,
            self.lease_owner,
            active_work.graph_version,
            &active_work.jobpack_id,
            coder_attempts,
            reviewer_attempts,
            reason,
        )?;
        Ok(CodingOutcome::Paused {
            change_set_id,
            reason: reason.to_owned(),
        })
    }

    async fn invoke_code_agent(
        &self,
        agent: AgentId,
        requirement: &str,
        active_work: &ActiveWork,
        tasks: &[CodeTodoState],
        findings: &[String],
        previous: Option<&CodeCheckpointSubmission>,
        tool_runtime: &mut ProjectToolRuntime,
    ) -> Result<CodeCheckpointSubmission> {
        let model = self.model_for(agent).await?;
        let mut system = self.harnesses.compose(agent)?;
        system.push_str(&format!(
            "\n\nPROJECT ROOT: {}\nWork only on the supplied ACTIVE Job Pack. Use project tools for real source work. Call submit_code_checkpoint only after source edits are complete; it must be the only tool call in that response.",
            self.project_root.display()
        ));
        let packet = json!({
            "original_instruction": requirement,
            "active_work": active_work_packet(active_work),
            "tasks": tasks,
            "review_findings": findings,
            "previous_checkpoint": previous.map(|item| json!({
                "summary": &item.summary,
                "completed_checklist": &item.completed_checklist,
                "goal_recheck": &item.goal_recheck
            })),
            "instruction": if agent == AgentId::Coder {
                "Implement the current ACTIVE Job Pack only. Inspect live source before writing. Submit claims only for checklist items actually satisfied by this checkpoint."
            } else {
                "Repair only the supplied Reviewer findings inside the same ACTIVE Job Pack. Preserve unrelated changes. Submit a complete revised checkpoint."
            }
        });
        let mut messages = vec![
            ChatMessage::system(system),
            ChatMessage::user(packet.to_string()),
        ];
        let mut definitions = tool_runtime.tool_definitions(agent);
        definitions.push(code_checkpoint_tool());

        for round in 0..8usize {
            let response = self
                .ollama
                .chat_stream_with_tools(&model, &messages, &definitions, |_| {})
                .await?;
            let calls = response.tool_calls.clone();
            messages.push(response);

            if calls.is_empty() {
                bail!(
                    "{} stopped without submit_code_checkpoint",
                    agent.display_name()
                );
            }
            if calls
                .iter()
                .any(|call| call.function.name == "submit_code_checkpoint")
            {
                if calls.len() != 1 || calls[0].function.name != "submit_code_checkpoint" {
                    bail!("submit_code_checkpoint must be the only tool call in its response");
                }
                return serde_json::from_value(calls[0].function.arguments.clone())
                    .context("invalid submit_code_checkpoint arguments");
            }

            if round + 1 >= 8 {
                bail!("{} exceeded coding tool rounds", agent.display_name());
            }
            for call in calls {
                messages.push(execute_project_tool_message(agent, tool_runtime, &call));
            }
        }
        unreachable!("bounded coding tool loop must return or fail")
    }

    async fn invoke_code_reviewer(
        &self,
        requirement: &str,
        active_work: &ActiveWork,
        tasks: &[CodeTodoState],
        checkpoint: &CodeCheckpointSubmission,
        change_set_id: &str,
        tool_runtime: &mut ProjectToolRuntime,
    ) -> Result<CodeReviewDecision> {
        let model = self.model_for(AgentId::Reviewer).await?;
        let mut system = self.harnesses.compose(AgentId::Reviewer)?;
        system.push_str(&format!(
            "\n\nPROJECT ROOT: {}\nThis is a fresh read-only code review. Inspect source with read tools as needed. Call submit_code_review only after forming the exact-target verdict; it must be the only tool call in that response.",
            self.project_root.display()
        ));
        let packet = json!({
            "original_instruction": requirement,
            "active_work": active_work_packet(active_work),
            "tasks": tasks,
            "target": {
                "graph_version": active_work.graph_version,
                "jobpack_id": &active_work.jobpack_id,
                "change_set_id": change_set_id,
                "mutation_journal": tool_runtime.journal(),
                "change_evidence": tool_runtime.review_evidence()
            },
            "coder_checkpoint": {
                "summary": &checkpoint.summary,
                "completed_checklist": &checkpoint.completed_checklist,
                "goal_recheck": &checkpoint.goal_recheck
            },
            "instruction": "Review only this exact current change set. Return PASS or CHANGES_REQUIRED with actionable findings. Do not repair source."
        });
        let mut messages = vec![
            ChatMessage::system(system),
            ChatMessage::user(packet.to_string()),
        ];
        let mut definitions = tool_runtime.tool_definitions(AgentId::Reviewer);
        definitions.push(code_review_tool());

        for round in 0..8usize {
            let response = self
                .ollama
                .chat_stream_with_tools(&model, &messages, &definitions, |_| {})
                .await?;
            let calls = response.tool_calls.clone();
            messages.push(response);

            if calls.is_empty() {
                bail!("Reviewer stopped without submit_code_review");
            }
            if calls
                .iter()
                .any(|call| call.function.name == "submit_code_review")
            {
                if calls.len() != 1 || calls[0].function.name != "submit_code_review" {
                    bail!("submit_code_review must be the only tool call in its response");
                }
                return serde_json::from_value(calls[0].function.arguments.clone())
                    .context("invalid submit_code_review arguments");
            }

            if round + 1 >= 8 {
                bail!("Reviewer exceeded code review tool rounds");
            }
            for call in calls {
                messages.push(execute_project_tool_message(
                    AgentId::Reviewer,
                    tool_runtime,
                    &call,
                ));
            }
        }
        unreachable!("bounded Reviewer tool loop must return or fail")
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

fn repair_progressed(
    journal_len_before: usize,
    journal_len_after: usize,
    previous_change_set_id: &str,
    repaired_change_set_id: &str,
) -> bool {
    journal_len_after > journal_len_before && repaired_change_set_id != previous_change_set_id
}

fn execute_project_tool_message(
    agent: AgentId,
    tool_runtime: &mut ProjectToolRuntime,
    call: &ToolCall,
) -> ChatMessage {
    let payload = match tool_runtime.execute(agent, &call.function.name, &call.function.arguments) {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => json!({"ok": false, "error": format!("{error:#}")}),
    };
    ChatMessage::tool(call.function.name.clone(), payload.to_string())
}

fn active_work_packet(active_work: &ActiveWork) -> serde_json::Value {
    json!({
        "graph_version": active_work.graph_version,
        "plan_revision": active_work.plan_revision,
        "plan_hash": &active_work.plan_hash,
        "milestone_id": &active_work.milestone_id,
        "milestone_title": &active_work.milestone_title,
        "jobpack_id": &active_work.jobpack_id,
        "jobpack_title": &active_work.jobpack_title,
        "goal": &active_work.goal,
        "required_inputs": &active_work.required_inputs,
        "expected_outputs": &active_work.expected_outputs,
        "acceptance": &active_work.acceptance,
        "verification_hints": &active_work.verification_hints
    })
}

fn build_project_context(root: &Path) -> Result<ProjectContext> {
    let root = root.canonicalize()?;
    let mut pending = vec![root.clone()];
    let mut paths = Vec::new();
    let mut text_files = Vec::new();
    let mut total_bytes = 0usize;
    let mut truncated = false;

    while let Some(dir) = pending.pop() {
        let mut entries = match fs::read_dir(&dir) {
            Ok(entries) => entries.filter_map(|entry| entry.ok()).collect::<Vec<_>>(),
            Err(_) => continue,
        };
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries.into_iter().rev() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_symlink() {
                continue;
            }

            let relative = match path.strip_prefix(&root) {
                Ok(relative) => relative,
                Err(_) => continue,
            };
            if should_skip(relative) {
                continue;
            }
            let relative_text = relative.to_string_lossy().replace('\\', "/");

            if file_type.is_dir() {
                pending.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }

            if paths.len() < MAX_CONTEXT_PATHS {
                paths.push(relative_text.clone());
            } else {
                truncated = true;
            }

            if text_files.len() >= MAX_CONTEXT_FILES || total_bytes >= MAX_CONTEXT_BYTES {
                truncated = true;
                continue;
            }

            let mut file = match fs::File::open(&path) {
                Ok(file) => file,
                Err(_) => continue,
            };
            let budget = MAX_FILE_BYTES.min(MAX_CONTEXT_BYTES.saturating_sub(total_bytes));
            if budget == 0 {
                truncated = true;
                continue;
            }
            let mut bytes = Vec::new();
            if file
                .by_ref()
                .take((budget + 1) as u64)
                .read_to_end(&mut bytes)
                .is_err()
            {
                continue;
            }
            let file_truncated = bytes.len() > budget;
            if file_truncated {
                bytes.truncate(budget);
            }
            let Ok(content) = String::from_utf8(bytes) else {
                continue;
            };
            total_bytes += content.len();
            text_files.push(ProjectContextFile {
                path: relative_text,
                content,
                truncated: file_truncated,
            });
            truncated |= file_truncated;
        }
    }

    paths.sort();
    text_files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ProjectContext {
        paths,
        text_files,
        truncated,
    })
}

fn should_skip(relative: &Path) -> bool {
    const SKIP_DIRS: &[&str] = &[
        ".git",
        ".gsa",
        "target",
        "node_modules",
        ".next",
        "dist",
        "build",
    ];
    if relative
        .components()
        .any(|component| SKIP_DIRS.contains(&component.as_os_str().to_string_lossy().as_ref()))
    {
        return true;
    }

    let name = relative
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name == ".env"
        || name.starts_with(".env.")
        || name == ".npmrc"
        || name == "credentials.json"
        || name == "id_rsa"
        || name == "id_ed25519"
}

#[derive(Debug, Deserialize)]
struct JobBuilderSubmission {
    status: String,
    #[serde(default)]
    gap_findings: Vec<String>,
    #[serde(default)]
    graph: Option<ExecutionGraph>,
}

#[derive(Debug, Deserialize)]
struct ReviewDecision {
    verdict: String,
    #[serde(default)]
    findings: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CrDecision {
    verdict: String,
    #[serde(default)]
    findings: Vec<String>,
}

fn extract_tool_args<T>(message: &ChatMessage, name: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    if message.tool_calls.len() != 1 {
        bail!(
            "expected exactly one workflow tool call {name}, received {}",
            message.tool_calls.len()
        );
    }
    let call = &message.tool_calls[0];
    if call.function.name != name {
        bail!(
            "expected workflow tool call {name}, received {}",
            call.function.name
        );
    }
    serde_json::from_value(call.function.arguments.clone())
        .with_context(|| format!("invalid arguments for tool {name}"))
}

fn plan_tool() -> ToolDefinition {
    ToolDefinition::function(
        "submit_plan",
        "Submit one complete GSA Local Implementation Plan revision.",
        json!({
            "type": "object",
            "required": [
                "goal",
                "current_architecture",
                "required_changes",
                "implementation_approach",
                "dependencies",
                "sequence",
                "risks",
                "acceptance_direction",
                "evidence_needs"
            ],
            "properties": {
                "goal": {"type": "string"},
                "current_architecture": {"type": "string"},
                "required_changes": {"type": "array", "items": {"type": "string"}},
                "implementation_approach": {"type": "array", "items": {"type": "string"}},
                "dependencies": {"type": "array", "items": {"type": "string"}},
                "sequence": {"type": "array", "items": {"type": "string"}},
                "risks": {"type": "array", "items": {"type": "string"}},
                "acceptance_direction": {"type": "array", "items": {"type": "string"}},
                "evidence_needs": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": [
                            "id", "question", "purpose", "required",
                            "consumer", "modes", "intent"
                        ],
                        "properties": {
                            "id": {"type": "string", "minLength": 1},
                            "question": {"type": "string", "minLength": 1},
                            "purpose": {"type": "string", "minLength": 1},
                            "required": {"type": "boolean"},
                            "consumer": {"type": "string", "minLength": 1},
                            "modes": {
                                "type": "array",
                                "minItems": 1,
                                "items": {
                                    "type": "string",
                                    "enum": ["VERIFY", "MEASURE", "PROBE"]
                                }
                            },
                            "intent": {"type": "string", "minLength": 1}
                        }
                    }
                }
            }
        }),
    )
}

fn review_tool() -> ToolDefinition {
    ToolDefinition::function(
        "submit_review",
        "Submit the Reviewer verdict for one exact plan revision/hash.",
        json!({
            "type": "object",
            "required": ["verdict", "findings"],
            "properties": {
                "verdict": {"type": "string", "enum": ["PASS", "CHANGES_REQUIRED"]},
                "findings": {"type": "array", "items": {"type": "string"}}
            }
        }),
    )
}

fn cr_tool() -> ToolDefinition {
    ToolDefinition::function(
        "submit_cr_review",
        "Submit an independent Local CR verdict for one exact plan revision/hash.",
        json!({
            "type": "object",
            "required": ["verdict", "findings"],
            "properties": {
                "verdict": {"type": "string", "enum": ["PASS", "REVISE"]},
                "findings": {"type": "array", "items": {"type": "string"}}
            }
        }),
    )
}

fn code_checkpoint_tool() -> ToolDefinition {
    ToolDefinition::function(
        "submit_code_checkpoint",
        "Submit the Coder/Internal Fix checkpoint after real project edits. Source identity is computed by runtime; do not provide hashes or change_set_id.",
        json!({
            "type": "object",
            "required": ["summary", "completed_checklist", "goal_recheck"],
            "properties": {
                "summary": {"type": "string", "minLength": 1},
                "completed_checklist": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["todo_id", "position"],
                        "properties": {
                            "todo_id": {"type": "string", "minLength": 1},
                            "position": {"type": "integer", "minimum": 1}
                        }
                    }
                },
                "goal_recheck": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            }
        }),
    )
}

fn code_review_tool() -> ToolDefinition {
    ToolDefinition::function(
        "submit_code_review",
        "Submit the read-only Reviewer verdict for the exact runtime-bound change set.",
        json!({
            "type": "object",
            "required": ["verdict", "findings"],
            "properties": {
                "verdict": {
                    "type": "string",
                    "enum": ["PASS", "CHANGES_REQUIRED"]
                },
                "findings": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            }
        }),
    )
}

fn execution_graph_tool() -> ToolDefinition {
    ToolDefinition::function(
        "submit_execution_graph",
        "Submit a Job Builder execution graph bound to the approved plan, or PLAN_GAP if the approved plan cannot be safely decomposed.",
        json!({
            "type": "object",
            "required": ["status", "gap_findings"],
            "properties": {
                "status": {"type": "string", "enum": ["READY", "PLAN_GAP"]},
                "gap_findings": {"type": "array", "items": {"type": "string"}},
                "graph": {
                    "type": "object",
                    "properties": {
                        "milestones": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["id", "title", "order"],
                                "properties": {
                                    "id": {"type": "string"},
                                    "title": {"type": "string"},
                                    "order": {"type": "integer", "minimum": 1}
                                }
                            }
                        },
                        "jobpacks": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": [
                                    "id", "milestone_id", "title", "goal", "todo_ids",
                                    "depends_on", "required_inputs", "expected_outputs",
                                    "acceptance", "verification_hints"
                                ],
                                "properties": {
                                    "id": {"type": "string"},
                                    "milestone_id": {"type": "string"},
                                    "title": {"type": "string"},
                                    "goal": {"type": "string"},
                                    "todo_ids": {"type": "array", "items": {"type": "string"}},
                                    "depends_on": {"type": "array", "items": {"type": "string"}},
                                    "required_inputs": {"type": "array", "items": {"type": "string"}},
                                    "expected_outputs": {"type": "array", "items": {"type": "string"}},
                                    "acceptance": {"type": "array", "items": {"type": "string"}},
                                    "verification_hints": {"type": "array", "items": {"type": "string"}}
                                }
                            }
                        },
                        "todos": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["id", "jobpack_id", "title", "checklist"],
                                "properties": {
                                    "id": {"type": "string"},
                                    "jobpack_id": {"type": "string"},
                                    "title": {"type": "string"},
                                    "checklist": {"type": "array", "items": {"type": "string"}}
                                }
                            }
                        }
                    },
                    "required": ["milestones", "jobpacks", "todos"]
                }
            }
        }),
    )
}

fn stage_name(stage: PlanningStage) -> &'static str {
    match stage {
        PlanningStage::Planner => "PLANNER",
        PlanningStage::Reviewer => "REVIEWER",
        PlanningStage::InternalFix => "INTERNAL_FIX",
        PlanningStage::LocalCr => "LOCAL_CR",
        PlanningStage::Approved => "APPROVED",
        PlanningStage::Paused => "PAUSED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cr_revise_routes_through_internal_fix_then_reviewer() {
        let mut route = PlanningRoute::new(5);
        assert!(route.enter_reviewer());
        route.reviewer_result(ReviewVerdict::Pass);
        assert!(route.enter_cr());
        route.cr_result(ReviewVerdict::Revise);
        assert_eq!(route.stage, PlanningStage::InternalFix);
        route.after_internal_fix();
        assert_eq!(route.stage, PlanningStage::Reviewer);
    }

    #[test]
    fn unchanged_revision_routes_to_paused() {
        let mut route = PlanningRoute::new(5);
        route.unchanged_revision();
        assert_eq!(route.stage, PlanningStage::Paused);
    }

    #[test]
    fn project_context_is_bounded_and_excludes_runtime_state() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join(".gsa/state")).unwrap();
        std::fs::create_dir_all(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join(".gsa/state/secret.txt"), "secret").unwrap();
        std::fs::write(dir.path().join("target/generated.rs"), "generated").unwrap();
        std::fs::write(dir.path().join(".env"), "TOKEN=secret").unwrap();

        let context = build_project_context(dir.path()).unwrap();
        assert!(context.paths.iter().any(|path| path == "src/main.rs"));
        assert!(!context.paths.iter().any(|path| path.contains(".gsa")));
        assert!(!context.paths.iter().any(|path| path.contains("target")));
        assert!(!context.paths.iter().any(|path| path == ".env"));
        assert!(context
            .text_files
            .iter()
            .any(|file| file.path == "src/main.rs" && file.content.contains("fn main")));
    }

    #[test]
    fn submit_plan_contract_requires_structured_evidence_needs() {
        let tool = plan_tool();
        let parameters = &tool.function.parameters;
        let required = parameters["required"].as_array().unwrap();
        assert!(required.iter().any(|item| item == "evidence_needs"));

        let evidence = &parameters["properties"]["evidence_needs"];
        assert_eq!(evidence["type"], "array");
        let item_required = evidence["items"]["required"].as_array().unwrap();
        for field in [
            "id",
            "question",
            "purpose",
            "required",
            "consumer",
            "modes",
            "intent",
        ] {
            assert!(item_required.iter().any(|item| item == field));
        }
        assert_eq!(
            evidence["items"]["properties"]["modes"]["items"]["enum"],
            serde_json::json!(["VERIFY", "MEASURE", "PROBE"])
        );
    }

    #[test]
    fn structured_submission_requires_exactly_one_total_tool_call() {
        use crate::ollama::{ToolCall, ToolFunctionCall};

        let expected = ToolCall {
            kind: Some("function".into()),
            function: ToolFunctionCall {
                index: Some(0),
                name: "submit_review".into(),
                arguments: serde_json::json!({
                    "verdict": "PASS",
                    "findings": []
                }),
            },
        };
        let extra = ToolCall {
            kind: Some("function".into()),
            function: ToolFunctionCall {
                index: Some(1),
                name: "unexpected_tool".into(),
                arguments: serde_json::json!({}),
            },
        };
        let mut message = ChatMessage::assistant("");
        message.tool_calls = vec![expected, extra];

        let result: Result<ReviewDecision> = extract_tool_args(&message, "submit_review");
        assert!(result.is_err());
    }

    #[test]
    fn internal_fix_requires_new_runtime_mutation_and_change_set() {
        assert!(!repair_progressed(2, 2, "change-a", "change-a"));
        assert!(!repair_progressed(2, 3, "change-a", "change-a"));
        assert!(!repair_progressed(2, 2, "change-a", "change-b"));
        assert!(repair_progressed(2, 3, "change-a", "change-b"));
    }

    #[test]
    fn loop_exhaustion_fails_closed_to_paused() {
        let mut route = PlanningRoute::new(1);
        assert!(route.enter_reviewer());
        route.reviewer_result(ReviewVerdict::Revise);
        assert!(!route.enter_reviewer());
        assert_eq!(route.stage, PlanningStage::Paused);
    }
}
