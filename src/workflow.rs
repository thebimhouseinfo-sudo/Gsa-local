use crate::{
    config::AppConfig,
    harness::{AgentId, HarnessRegistry},
    ollama::{ChatMessage, OllamaClient, ToolDefinition},
    plan::{PlanArtifact, PlanRevision},
    registry::{PlanBinding, Registry, ReviewActor, ReviewVerdict},
    session::Session,
};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

const DEFAULT_MAX_ATTEMPTS: u32 = 5;

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
    Approved(PlanBinding),
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

        let artifact = self
            .invoke_plan_agent(
                AgentId::Planner,
                requirement,
                None,
                &[],
                "Create the first Implementation Plan revision.",
            )
            .await?;
        let mut current = self.registry.persist_plan_revision(&artifact)?;

        loop {
            if !route.enter_reviewer() {
                return self.pause(&route, Some(current.revision));
            }
            self.persist_route(&route, Some(current.revision))?;

            let review = self.invoke_reviewer(requirement, &current).await?;
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
                        "Revise the plan to resolve Reviewer findings. Produce a new complete plan.",
                    )
                    .await?;
                current = self.registry.persist_plan_revision(&revised)?;
                continue;
            }

            if !route.enter_cr() {
                return self.pause(&route, Some(current.revision));
            }
            self.persist_route(&route, Some(current.revision))?;

            let cr = self.invoke_cr(requirement, &current).await?;
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
                return Ok(PlanningOutcome::Approved(binding));
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
                    "Repair only the CR findings inside the existing planning scope. Return a complete revised plan.",
                )
                .await?;
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
        instruction: &str,
    ) -> Result<PlanArtifact> {
        let mut packet = json!({
            "original_requirement": requirement,
            "instruction": instruction,
            "project_root": self.project_root.display().to_string(),
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
    ) -> Result<ReviewDecision> {
        let packet = json!({
            "original_requirement": requirement,
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

    async fn invoke_cr(&self, requirement: &str, current: &PlanRevision) -> Result<CrDecision> {
        let packet = json!({
            "original_requirement": requirement,
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
    let mut matching = message
        .tool_calls
        .iter()
        .filter(|call| call.function.name == name);
    let call = matching
        .next()
        .with_context(|| format!("missing required tool call {name}"))?;
    if matching.next().is_some() {
        bail!("multiple {name} tool calls are not allowed");
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
                "acceptance_direction"
            ],
            "properties": {
                "goal": {"type": "string"},
                "current_architecture": {"type": "string"},
                "required_changes": {"type": "array", "items": {"type": "string"}},
                "implementation_approach": {"type": "array", "items": {"type": "string"}},
                "dependencies": {"type": "array", "items": {"type": "string"}},
                "sequence": {"type": "array", "items": {"type": "string"}},
                "risks": {"type": "array", "items": {"type": "string"}},
                "acceptance_direction": {"type": "array", "items": {"type": "string"}}
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
    fn loop_exhaustion_fails_closed_to_paused() {
        let mut route = PlanningRoute::new(1);
        assert!(route.enter_reviewer());
        route.reviewer_result(ReviewVerdict::Revise);
        assert!(!route.enter_reviewer());
        assert_eq!(route.stage, PlanningStage::Paused);
    }
}
