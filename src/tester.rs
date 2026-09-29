use crate::{
    config::AppConfig,
    controller::ActiveWork,
    harness::{AgentId, HarnessRegistry},
    ollama::{ChatMessage, OllamaClient, ToolCall, ToolDefinition},
    registry::Registry,
    session::Session,
    tools::ProjectToolRuntime,
    verification::{RecordedVerification, VerificationCapability, VerificationResult},
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

const MAX_TESTER_TOOL_ROUNDS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TesterVerdict {
    Pass,
    Fail,
    Blocked,
    NotApplicable,
}

impl TesterVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Blocked => "BLOCKED",
            Self::NotApplicable => "NOT_APPLICABLE",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "PASS" => Ok(Self::Pass),
            "FAIL" => Ok(Self::Fail),
            "BLOCKED" => Ok(Self::Blocked),
            "NOT_APPLICABLE" => Ok(Self::NotApplicable),
            other => bail!("unknown Tester verdict {other}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterRunRecord {
    pub id: i64,
    pub graph_version: i64,
    pub jobpack_id: String,
    pub change_set_id: String,
    pub verification_run_id: i64,
    pub verdict: TesterVerdict,
    pub findings: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TesterEligibility {
    Run,
    SkipNotApplicable,
    RepairBeforeTester,
    BlockedBeforeTester,
}

pub fn tester_eligibility(verification: &RecordedVerification) -> TesterEligibility {
    match verification.result {
        VerificationResult::Fail => TesterEligibility::RepairBeforeTester,
        VerificationResult::Blocked => TesterEligibility::BlockedBeforeTester,
        VerificationResult::NotApplicable => TesterEligibility::SkipNotApplicable,
        VerificationResult::TestPass => {
            if verification
                .evidence
                .profile
                .capabilities
                .iter()
                .any(|capability| {
                    matches!(
                        capability,
                        VerificationCapability::Unit
                            | VerificationCapability::Integration
                            | VerificationCapability::Browser
                    )
                })
            {
                TesterEligibility::Run
            } else {
                TesterEligibility::SkipNotApplicable
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TesterOutcome {
    Pass {
        tester_run_id: i64,
    },
    Fail {
        tester_run_id: i64,
        findings: Vec<String>,
    },
    Blocked {
        tester_run_id: Option<i64>,
        reason: String,
    },
    NotApplicable {
        tester_run_id: Option<i64>,
        reason: String,
    },
    SkippedNotApplicable {
        reason: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct TesterSubmission {
    verdict: String,
    #[serde(default)]
    findings: Vec<String>,
    #[serde(default)]
    evidence: Vec<String>,
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
        active_work: &ActiveWork,
        verification: &RecordedVerification,
        tool_runtime: &mut ProjectToolRuntime,
    ) -> Result<TesterOutcome> {
        match tester_eligibility(verification) {
            TesterEligibility::RepairBeforeTester => {
                return Ok(TesterOutcome::Blocked {
                    tester_run_id: None,
                    reason: "deterministic verification failed before Tester".into(),
                })
            }
            TesterEligibility::BlockedBeforeTester => {
                return Ok(TesterOutcome::Blocked {
                    tester_run_id: None,
                    reason: "deterministic verification is blocked before Tester".into(),
                })
            }
            TesterEligibility::SkipNotApplicable => {
                return Ok(TesterOutcome::SkippedNotApplicable {
                    reason: "verification evidence does not expose a testable semantic capability"
                        .into(),
                })
            }
            TesterEligibility::Run => {}
        }

        if verification.graph_version != active_work.graph_version
            || verification.jobpack_id != active_work.jobpack_id
        {
            bail!("Tester target does not match ACTIVE Job Pack");
        }

        let (acceptance, verification_hints) = self
            .registry
            .jobpack_test_contract(active_work.graph_version, &active_work.jobpack_id)?
            .context("ACTIVE Job Pack test contract is missing")?;
        let tasks = self
            .registry
            .jobpack_code_tasks(active_work.graph_version, &active_work.jobpack_id)?;

        let packet = json!({
            "target": {
                "graph_version": verification.graph_version,
                "jobpack_id": verification.jobpack_id,
                "change_set_id": verification.change_set_id,
                "verification_run_id": verification.id
            },
            "jobpack": {
                "goal": active_work.goal,
                "acceptance": acceptance,
                "verification_hints": verification_hints,
                "tasks": tasks
            },
            "verification": verification,
            "instruction": "Perform a read-only semantic Tester review of this exact verified target. Use project_list/project_read/project_search as needed. Submit exactly one structured Tester verdict. Do not modify product source."
        });

        let model = self.model_for(AgentId::Tester).await?;
        let mut system = self.harnesses.compose(AgentId::Tester)?;
        system.push_str(&format!(
            "\n\nPROJECT ROOT: {}\nThe target identity is immutable. Source writes are forbidden. You must call submit_test_result to finish.",
            self.project_root.display()
        ));
        let mut messages = vec![ChatMessage::system(system), ChatMessage::user(packet.to_string())];
        let mut definitions = tool_runtime.tool_definitions(AgentId::Tester);
        definitions.push(tester_result_tool());

        for round in 0..MAX_TESTER_TOOL_ROUNDS {
            let response = self
                .ollama
                .chat_stream_with_tools(&model, &messages, &definitions, |_| {})
                .await?;
            let calls = response.tool_calls.clone();
            messages.push(response);

            let submissions = calls
                .iter()
                .filter(|call| call.function.name == "submit_test_result")
                .collect::<Vec<_>>();

            if !submissions.is_empty() {
                if submissions.len() != 1 || calls.len() != 1 {
                    return Ok(TesterOutcome::Blocked {
                        tester_run_id: None,
                        reason: "Tester mixed the final structured verdict with other tool calls"
                            .into(),
                    });
                }
                let submission: TesterSubmission =
                    serde_json::from_value(submissions[0].function.arguments.clone())
                        .context("Tester returned malformed structured result")?;
                let verdict = TesterVerdict::parse(&submission.verdict)?;
                let record = self.registry.record_tester_evidence(
                    self.project_root,
                    self.lease_owner,
                    active_work.graph_version,
                    &active_work.jobpack_id,
                    &verification.change_set_id,
                    verification.id,
                    verdict,
                    &submission.findings,
                    &submission.evidence,
                )?;

                if verdict == TesterVerdict::Fail {
                    self.registry.route_post_review_failure_to_internal_fix(
                        self.project_root,
                        self.lease_owner,
                        active_work.graph_version,
                        &active_work.jobpack_id,
                        &verification.change_set_id,
                        verification.id,
                        "TESTER_FAIL",
                        &submission.findings,
                    )?;
                }
                return Ok(outcome_from_record(record));
            }

            if calls.is_empty() {
                return Ok(TesterOutcome::Blocked {
                    tester_run_id: None,
                    reason: "Tester did not submit a structured verdict".into(),
                });
            }

            if round + 1 >= MAX_TESTER_TOOL_ROUNDS {
                return Ok(TesterOutcome::Blocked {
                    tester_run_id: None,
                    reason: format!(
                        "Tester exceeded maximum project-tool rounds ({MAX_TESTER_TOOL_ROUNDS})"
                    ),
                });
            }

            for call in calls {
                messages.push(execute_tester_tool(tool_runtime, &call));
            }
        }

        unreachable!("bounded Tester tool loop must return")
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

fn outcome_from_record(record: TesterRunRecord) -> TesterOutcome {
    match record.verdict {
        TesterVerdict::Pass => TesterOutcome::Pass {
            tester_run_id: record.id,
        },
        TesterVerdict::Fail => TesterOutcome::Fail {
            tester_run_id: record.id,
            findings: record.findings,
        },
        TesterVerdict::Blocked => TesterOutcome::Blocked {
            tester_run_id: Some(record.id),
            reason: record
                .findings
                .first()
                .cloned()
                .unwrap_or_else(|| "Tester reported BLOCKED".into()),
        },
        TesterVerdict::NotApplicable => TesterOutcome::NotApplicable {
            tester_run_id: Some(record.id),
            reason: record
                .findings
                .first()
                .cloned()
                .unwrap_or_else(|| "Tester reported NOT_APPLICABLE".into()),
        },
    }
}

fn execute_tester_tool(
    tool_runtime: &mut ProjectToolRuntime,
    call: &ToolCall,
) -> ChatMessage {
    let payload = match tool_runtime.execute(
        AgentId::Tester,
        &call.function.name,
        &call.function.arguments,
    ) {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => json!({"ok": false, "error": format!("{error:#}")}),
    };
    ChatMessage::tool(call.function.name.clone(), payload.to_string())
}

fn tester_result_tool() -> ToolDefinition {
    ToolDefinition::function(
        "submit_test_result",
        "Submit the final read-only Tester verdict for the exact immutable verification target.",
        json!({
            "type": "object",
            "properties": {
                "verdict": {
                    "type": "string",
                    "enum": ["PASS", "FAIL", "BLOCKED", "NOT_APPLICABLE"]
                },
                "findings": {
                    "type": "array",
                    "items": {"type": "string"}
                },
                "evidence": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            },
            "required": ["verdict", "findings", "evidence"],
            "additionalProperties": false
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verification::{
        DiscoveryStatus, VerificationEvidence, VerificationProfile,
    };

    fn recorded(
        result: VerificationResult,
        capabilities: Vec<VerificationCapability>,
    ) -> RecordedVerification {
        RecordedVerification {
            id: 7,
            graph_version: 1,
            jobpack_id: "JP1".into(),
            change_set_id: "change".into(),
            result,
            evidence: VerificationEvidence {
                profile: VerificationProfile {
                    status: DiscoveryStatus::Applicable,
                    capabilities,
                    commands: vec![],
                    reason: None,
                },
                commands: vec![],
                test_surface_changed: false,
            },
        }
    }

    #[test]
    fn eligibility_is_deterministic() {
        assert_eq!(
            tester_eligibility(&recorded(
                VerificationResult::Fail,
                vec![VerificationCapability::Integration]
            )),
            TesterEligibility::RepairBeforeTester
        );
        assert_eq!(
            tester_eligibility(&recorded(
                VerificationResult::Blocked,
                vec![VerificationCapability::Integration]
            )),
            TesterEligibility::BlockedBeforeTester
        );
        assert_eq!(
            tester_eligibility(&recorded(
                VerificationResult::NotApplicable,
                vec![VerificationCapability::BuildOnly]
            )),
            TesterEligibility::SkipNotApplicable
        );
        assert_eq!(
            tester_eligibility(&recorded(
                VerificationResult::TestPass,
                vec![
                    VerificationCapability::BuildOnly,
                    VerificationCapability::Integration
                ]
            )),
            TesterEligibility::Run
        );
        assert_eq!(
            tester_eligibility(&recorded(
                VerificationResult::TestPass,
                vec![VerificationCapability::BuildOnly]
            )),
            TesterEligibility::SkipNotApplicable
        );
    }
}
