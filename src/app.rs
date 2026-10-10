use crate::{
    agent_runtime::{
        dispatch_tool_calls, resolve_model_name, run_with_project_tools, MAX_TOOL_ROUNDS,
    },
    cli::{self, InputLine, SlashCommand},
    config::AppConfig,
    controller::{ActiveWork, MilestoneController, NextWork},
    harness::{AgentId, HarnessRegistry},
    ollama::{ChatMessage, OllamaClient, ToolDefinition},
    registry::{Registry, TesterCheckpointDisposition},
    session::Session,
    tester_execution::{available_tester_capabilities, TesterWorkflow},
    tools::ProjectToolRuntime,
    verification::{discover_profile, VerificationController},
    workflow::{
        CodeCrOutcome, CodeCrWorkflow, CodingOutcome, CodingWorkflow, PlanningOutcome,
        PlanningWorkflow,
    },
};
use anyhow::{bail, Context, Result};
use std::{
    collections::HashMap,
    io::{self, Write},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

const MAX_TESTER_PRODUCT_REPAIRS: usize = 3;
const MAX_CODE_CR_REPAIRS: usize = 3;

pub struct App {
    project_root: PathBuf,
    config: AppConfig,
    session: Session,
    ollama: OllamaClient,
    harnesses: HarnessRegistry,
    registry: Registry,
    lease_owner: String,
    history: HashMap<AgentId, Vec<ChatMessage>>,
    active_work: Option<ActiveWork>,
    tool_runtime: ProjectToolRuntime,
}

impl App {
    pub fn new() -> Result<Self> {
        let project_root = std::env::current_dir()
            .context("failed to resolve current directory")?
            .canonicalize()
            .context("failed to canonicalize current directory")?;
        let config = AppConfig::load()?;
        let ollama =
            OllamaClient::with_num_ctx(config.ollama_base_url.clone(), config.ollama_num_ctx);
        let tool_runtime = ProjectToolRuntime::new(&project_root)?;
        let registry = Registry::open(&project_root)?;
        let lease_owner = format!("pid:{}", std::process::id());
        registry.acquire_lease(
            &project_root,
            &lease_owner,
            Duration::from_secs(6 * 60 * 60),
        )?;
        let active_work = None;

        Ok(Self {
            project_root,
            config,
            session: Session::default(),
            ollama,
            harnesses: HarnessRegistry::default(),
            registry,
            lease_owner,
            history: HashMap::new(),
            active_work,
            tool_runtime,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        self.print_welcome();
        let startup_profile = discover_profile(&self.project_root)?;
        let startup_capabilities = available_tester_capabilities(&startup_profile);
        let startup_resume = self
            .registry
            .resolve_resume_decision(&self.project_root, &startup_capabilities)?;
        println!(
            "Resume: {:?} -> {:?} ({})",
            startup_resume.classification, startup_resume.action, startup_resume.reason
        );
        let stdin = io::stdin();

        loop {
            print!("> ");
            io::stdout().flush()?;

            let mut input = String::new();
            if stdin.read_line(&mut input)? == 0 {
                break;
            }

            match cli::parse_line(&input) {
                Ok(InputLine::Empty) => {}
                Ok(InputLine::UserText(text)) => {
                    if let Err(error) = self.dispatch_user_text(text).await {
                        eprintln!("error: {error:#}");
                    }
                }
                Ok(InputLine::Command(command)) => {
                    if let Err(error) = self.handle_command(command).await {
                        eprintln!("error: {error:#}");
                    }
                }
                Err(error) => eprintln!("error: {error}"),
            }
        }

        self.registry
            .release_lease(&self.project_root, &self.lease_owner)?;
        Ok(())
    }

    fn print_welcome(&self) {
        println!("GSA Local");
        println!("Project: {}", self.project_root.display());
        println!("Agent: {}", self.session.active_agent);
        println!("Commands: /agent  /model  /config");
        if let Some(work) = &self.active_work {
            println!("Work: {} / {}", work.milestone_id, work.jobpack_id);
        }
        println!("Ctrl-D to exit.");
    }

    async fn handle_command(&mut self, command: SlashCommand) -> Result<()> {
        match command {
            SlashCommand::Agent(arg) => self.handle_agent(arg),
            SlashCommand::Model(arg) => self.handle_model(arg).await,
            SlashCommand::Config => self.handle_config().await,
        }
    }

    fn handle_agent(&mut self, arg: Option<String>) -> Result<()> {
        let agent = if let Some(arg) = arg {
            let agent = AgentId::from_str(&arg)?;
            if !agent.is_user_selectable() {
                bail!("{} is internal-only", agent.display_name());
            }
            agent
        } else {
            let agents = AgentId::user_selectable();
            for (index, agent) in agents.iter().enumerate() {
                println!("{}. {}", index + 1, agent.display_name());
            }
            let choice = prompt("Select agent")?;
            agents[parse_index(&choice, agents.len())?]
        };

        self.session.set_active_agent(agent);
        println!("Active agent: {agent}");
        Ok(())
    }

    async fn handle_model(&mut self, arg: Option<String>) -> Result<()> {
        let models = self.ollama.list_models().await?;
        if models.is_empty() {
            bail!("Ollama returned no installed models");
        }

        let model = if let Some(model) = arg {
            if !models.iter().any(|item| item == &model) {
                bail!("model {model} is not present in Ollama");
            }
            model
        } else {
            print_models(&models);
            let choice = prompt("Select model")?;
            models[parse_index(&choice, models.len())?].clone()
        };

        let agent = self.session.active_agent;
        self.session.set_model_override(agent, model.clone());
        println!("Session model for {}: {}", agent.display_name(), model);
        Ok(())
    }

    async fn handle_config(&mut self) -> Result<()> {
        let models = self.ollama.list_models().await?;
        if models.is_empty() {
            bail!("Ollama returned no installed models");
        }
        print_models(&models);

        let current_default = self.config.default_model.as_deref().unwrap_or("<none>");
        let default_choice = prompt(&format!(
            "Default model [current: {}] (number, blank=keep, none=clear)",
            current_default
        ))?;
        apply_default_model_choice(&mut self.config, &models, &default_choice)?;

        for agent in AgentId::user_selectable() {
            let current = self.config.agent_model(*agent).unwrap_or("<none>");
            let choice = prompt(&format!(
                "{} model [current: {}] (number, blank=keep)",
                agent.display_name(),
                current
            ))?;
            if choice.trim().is_empty() {
                continue;
            }
            let model = models[parse_index(&choice, models.len())?].clone();
            self.config.set_agent_model(*agent, model);
        }

        self.config.save()?;
        println!("Saved {}", AppConfig::config_path()?.display());
        Ok(())
    }

    async fn resolve_coder_work_after_testers(&mut self) -> Result<Option<ActiveWork>> {
        self.active_work = None;
        let mut product_repairs = 0usize;
        let mut cr_repairs = 0usize;
        loop {
            let profile = discover_profile(&self.project_root)?;
            let available_capabilities = available_tester_capabilities(&profile);
            let next =
                MilestoneController::new(&self.registry, &self.project_root, &self.lease_owner)
                    .resolve_next(&available_capabilities)?;

            match next {
                None => return Ok(None),
                Some(NextWork::Coder(work)) => return Ok(Some(work)),
                Some(NextWork::Repair(repair)) => {
                    if product_repairs >= MAX_TESTER_PRODUCT_REPAIRS {
                        bail!(
                            "Tester PRODUCT_FAILURE repair limit exhausted for checkpoint {}",
                            repair.checkpoint.id
                        );
                    }
                    product_repairs += 1;
                    println!(
                        "TEST_PRODUCT_FAILURE checkpoint={} failed_attempt={} repair_round={}",
                        repair.checkpoint.id,
                        repair.retest_context.failed_attempt_id,
                        product_repairs
                    );
                    let prior_failed_execution_steps = self.registry.tester_execution_steps(
                        repair.active_work.graph_version,
                        &repair.checkpoint.id,
                        &repair.retest_context.failed_attempt_id,
                    )?;
                    if prior_failed_execution_steps.iter().any(|step| {
                        step.target_fingerprint != repair.retest_context.failed_target_fingerprint
                    }) {
                        bail!(
                            "Tester PRODUCT_FAILURE evidence target does not match failed target fingerprint"
                        );
                    }
                    let repair_context = serde_json::json!({
                        "checkpoint": &repair.checkpoint,
                        "failed_target": &repair.failed_target,
                        "retest_context": &repair.retest_context,
                        "prior_failed_execution_steps": prior_failed_execution_steps,
                        "evidence_role": "context_only_not_quality_pass"
                    });
                    let requirement = format!(
                        "Repair the product source for a declared Tester PRODUCT_FAILURE. \
Checkpoint: {}. Goal: {}. Criteria: {}. Failed attempt: {}. \
Failed target fingerprint: {}. Observed failure: {}. \
Do not edit Tester-owned artifacts as the product fix. Use the structured repair_context as observed failure evidence, keep the repair within the active Job Pack, perform the normal Coder self-check, and return through independent Reviewer before any retest.",
                        repair.checkpoint.id,
                        repair.checkpoint.goal,
                        repair.checkpoint.criteria.join("; "),
                        repair.retest_context.failed_attempt_id,
                        repair.retest_context.failed_target_fingerprint,
                        repair.retest_context.failure_summary
                    );
                    let workflow = CodingWorkflow::new(
                        &self.ollama,
                        &self.harnesses,
                        &self.registry,
                        &self.config,
                        &self.session,
                        &self.project_root,
                        &self.lease_owner,
                    );
                    match workflow
                        .run_with_context(
                            &requirement,
                            &repair.active_work,
                            Some(&repair_context),
                            &mut self.tool_runtime,
                        )
                        .await?
                    {
                        CodingOutcome::ReviewPass { change_set_id } => {
                            println!(
                                "TEST_REPAIR_REVIEW_PASS checkpoint={} jobpack={} change_set={}",
                                repair.checkpoint.id, repair.active_work.jobpack_id, change_set_id
                            );
                            let changed_paths = self
                                .tool_runtime
                                .review_evidence()
                                .into_iter()
                                .map(|item| item.path)
                                .collect::<Vec<_>>();
                            let verification = VerificationController::new(
                                &self.registry,
                                &self.project_root,
                                &self.lease_owner,
                            )
                            .verify(
                                repair.active_work.graph_version,
                                &repair.active_work.jobpack_id,
                                &change_set_id,
                                &changed_paths,
                            )?;
                            println!(
                                "VERIFICATION_RESULT jobpack={} change_set={} result={}",
                                repair.active_work.jobpack_id,
                                change_set_id,
                                verification.as_str()
                            );
                            continue;
                        }
                        CodingOutcome::Paused {
                            change_set_id,
                            reason,
                        } => {
                            bail!(
                                "Tester PRODUCT_FAILURE repair paused checkpoint={} change_set={:?}: {}",
                                repair.checkpoint.id,
                                change_set_id,
                                reason
                            );
                        }
                    }
                }
                Some(NextWork::Cr(cr_work)) => {
                    let active_work: ActiveWork = self
                        .registry
                        .current_active_work()?
                        .map(Into::into)
                        .context("Local CR boundary requires an ACTIVE Job Pack")?;
                    if active_work.jobpack_id != cr_work.key.jobpack_id {
                        bail!(
                            "Local CR boundary target {} does not match ACTIVE Job Pack {}",
                            cr_work.key.jobpack_id,
                            active_work.jobpack_id
                        );
                    }

                    let workflow = CodeCrWorkflow::new(
                        &self.ollama,
                        &self.harnesses,
                        &self.registry,
                        &self.config,
                        &self.session,
                        &self.project_root,
                        &self.lease_owner,
                    );
                    let requirement = format!(
                        "Review mature Local CR boundary {} for Job Pack {} on exact change set {}.",
                        cr_work.key.boundary_id,
                        cr_work.key.jobpack_id,
                        cr_work.key.change_set_id
                    );
                    match workflow
                        .run(&requirement, &active_work, &cr_work, &mut self.tool_runtime)
                        .await?
                    {
                        CodeCrOutcome::Pass => {
                            println!(
                                "CODE_CR_PASS jobpack={} boundary={} change_set={}",
                                cr_work.key.jobpack_id,
                                cr_work.key.boundary_id,
                                cr_work.key.change_set_id
                            );
                            if cr_work.terminal {
                                self.active_work = MilestoneController::new(
                                    &self.registry,
                                    &self.project_root,
                                    &self.lease_owner,
                                )
                                .mark_active_jobpack_done()?;
                                println!(
                                    "JOBPACK_DONE jobpack={} change_set={}",
                                    cr_work.key.jobpack_id, cr_work.key.change_set_id
                                );
                            }
                            continue;
                        }
                        CodeCrOutcome::Revise { findings } => {
                            if cr_repairs >= MAX_CODE_CR_REPAIRS {
                                bail!(
                                    "Local CR repair limit exhausted for Job Pack {}",
                                    cr_work.key.jobpack_id
                                );
                            }
                            cr_repairs += 1;
                            println!(
                                "CODE_CR_REVISE jobpack={} boundary={} repair_round={}",
                                cr_work.key.jobpack_id, cr_work.key.boundary_id, cr_repairs
                            );
                            let repair_context = serde_json::json!({
                                "cr_boundary": &cr_work,
                                "findings": &findings,
                                "evidence_role": "local_cr_findings_require_normal_repair_and_revalidation"
                            });
                            let coding = CodingWorkflow::new(
                                &self.ollama,
                                &self.harnesses,
                                &self.registry,
                                &self.config,
                                &self.session,
                                &self.project_root,
                                &self.lease_owner,
                            );
                            match coding
                                .run_repair(
                                    "Repair only the Local CR findings for the current mature boundary. Return through Reviewer and exact-target verification before Local CR runs again.",
                                    &active_work,
                                    &findings,
                                    Some(&repair_context),
                                    &mut self.tool_runtime,
                                )
                                .await?
                            {
                                CodingOutcome::ReviewPass { change_set_id } => {
                                    let changed_paths = self
                                        .tool_runtime
                                        .review_evidence()
                                        .into_iter()
                                        .map(|item| item.path)
                                        .collect::<Vec<_>>();
                                    let verification = VerificationController::new(
                                        &self.registry,
                                        &self.project_root,
                                        &self.lease_owner,
                                    )
                                    .verify(
                                        active_work.graph_version,
                                        &active_work.jobpack_id,
                                        &change_set_id,
                                        &changed_paths,
                                    )?;
                                    println!(
                                        "CODE_CR_REPAIR_REVIEW_PASS jobpack={} change_set={} verification={}",
                                        active_work.jobpack_id,
                                        change_set_id,
                                        verification.as_str()
                                    );
                                    continue;
                                }
                                CodingOutcome::Paused {
                                    change_set_id,
                                    reason,
                                } => {
                                    bail!(
                                        "Local CR repair paused jobpack={} change_set={:?}: {}",
                                        active_work.jobpack_id,
                                        change_set_id,
                                        reason
                                    );
                                }
                            }
                        }
                    }
                }
                Some(NextWork::Tester(checkpoint_work)) => {
                    if checkpoint_work.disposition == TesterCheckpointDisposition::SpecGap {
                        let reason = checkpoint_work
                            .reason
                            .as_deref()
                            .unwrap_or("Tester reported a material specification gap");
                        let prior_plan = self
                            .registry
                            .plan_for_current_execution_graph(checkpoint_work.graph_version)?;
                        println!(
                            "TEST_SPEC_GAP checkpoint={} plan_revision={} reason={}",
                            checkpoint_work.checkpoint.id, prior_plan.revision, reason
                        );
                        let requirement = format!(
                            "Replan the current approved work because Tester found a material SPEC_GAP at declared checkpoint {}. Prior approved goal: {}. Checkpoint goal: {}. Criteria: {}. Observed gap: {}. Preserve valid observed evidence and do not invent missing runtime facts. Produce a new complete plan revision that resolves the gap or routes to Human when the requirement cannot be decided safely.",
                            checkpoint_work.checkpoint.id,
                            prior_plan.artifact.goal,
                            checkpoint_work.checkpoint.goal,
                            checkpoint_work.checkpoint.criteria.join("; "),
                            reason
                        );
                        let workflow = PlanningWorkflow::new(
                            &self.ollama,
                            &self.harnesses,
                            &self.registry,
                            &self.config,
                            &self.session,
                            &self.project_root,
                        );
                        match workflow.run(&requirement).await? {
                            PlanningOutcome::Registered {
                                plan,
                                graph_version,
                            } => {
                                println!(
                                    "TEST_SPEC_GAP_REPLANNED revision={} graph_version={}",
                                    plan.revision, graph_version
                                );
                                continue;
                            }
                            PlanningOutcome::Paused { revision, .. } => {
                                bail!(
                                    "Tester SPEC_GAP replanning paused at revision {:?}; Human decision is required",
                                    revision
                                );
                            }
                        }
                    }

                    if checkpoint_work.disposition != TesterCheckpointDisposition::Due {
                        let reason = checkpoint_work
                            .reason
                            .as_deref()
                            .unwrap_or("checkpoint cannot progress");
                        bail!(
                            "Tester checkpoint {} is {}: {}",
                            checkpoint_work.checkpoint.id,
                            checkpoint_work.disposition.as_str(),
                            reason
                        );
                    }

                    let target = checkpoint_work
                        .target
                        .clone()
                        .context("DUE Tester checkpoint is missing exact target binding")?;
                    let attempt_id = checkpoint_work
                        .next_attempt_id
                        .as_deref()
                        .context("DUE Tester checkpoint is missing next attempt id")?;
                    let requirement = if let Some(retest) = &checkpoint_work.retest_context {
                        println!(
                            "TEST_CHECKPOINT_RETEST_DUE checkpoint={} attempt={} prior_attempt={}",
                            checkpoint_work.checkpoint.id, attempt_id, retest.failed_attempt_id
                        );
                        format!(
                            "{} RETEST_CONTEXT: rerun the relevant failing and regression cases from prior attempt {} on failed target {}. Prior observed failure: {}",
                            checkpoint_work.checkpoint.goal,
                            retest.failed_attempt_id,
                            retest.failed_target_fingerprint,
                            retest.failure_summary
                        )
                    } else {
                        println!(
                            "TEST_CHECKPOINT_DUE checkpoint={} attempt={}",
                            checkpoint_work.checkpoint.id, attempt_id
                        );
                        checkpoint_work.checkpoint.goal.clone()
                    };
                    let workflow = TesterWorkflow::new(
                        &self.ollama,
                        &self.harnesses,
                        &self.registry,
                        &self.config,
                        &self.session,
                        &self.project_root,
                        &self.lease_owner,
                    );
                    let attempt = workflow
                        .run(
                            &requirement,
                            checkpoint_work.graph_version,
                            &checkpoint_work.checkpoint,
                            target,
                            attempt_id,
                            checkpoint_work.retest_context.as_ref(),
                            &mut self.tool_runtime,
                        )
                        .await?;
                    println!(
                        "TEST_CHECKPOINT_RECORDED checkpoint={} attempt={}",
                        attempt.checkpoint_id, attempt.attempt_id
                    );
                }
            }
        }
    }

    async fn dispatch_planner_text(&mut self, text: String) -> Result<()> {
        if is_explicit_planning_workflow_request(&text) {
            return self.run_planning_workflow(&text).await;
        }

        let agent = AgentId::Planner;
        let model = resolve_model_name(&self.ollama, &self.session, &self.config, agent).await?;

        let mut messages = self.history.get(&agent).cloned().unwrap_or_else(|| {
            let mut system = self
                .harnesses
                .compose(agent)
                .unwrap_or_else(|error| format!("Harness load error: {error}"));
            system.push_str(&format!(
                "\n\nPROJECT ROOT: {}\nYou are in interactive Planner conversation mode. Use project_list/project_search/project_read for read-only repository questions. Call start_planning_workflow only when the user explicitly asks you to create, revise, or start an implementation plan. Do not call it for greetings, status questions, explanations, or planning discussion.",
                self.project_root.display()
            ));
            vec![ChatMessage::system(system)]
        });
        messages.push(ChatMessage::user(text.clone()));

        let mut definitions = self.tool_runtime.tool_definitions(agent);
        definitions.push(planning_start_tool());

        print!("{} [{}]: ", agent.display_name(), model);
        io::stdout().flush()?;

        for round in 0..MAX_TOOL_ROUNDS {
            let response = self
                .ollama
                .chat_stream_with_tools(&model, &messages, &definitions, |token| {
                    print!("{token}");
                    let _ = io::stdout().flush();
                })
                .await?;
            let calls = response.tool_calls.clone();

            if calls.is_empty() {
                messages.push(response);
                self.history.insert(agent, messages);
                println!();
                return Ok(());
            }

            if calls
                .iter()
                .any(|call| call.function.name == "start_planning_workflow")
            {
                if calls.len() != 1 || calls[0].function.name != "start_planning_workflow" {
                    bail!("start_planning_workflow must be the only tool call in its response");
                }
                let requirement = calls[0].function.arguments["requirement"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .context("start_planning_workflow requires a non-empty requirement")?
                    .to_owned();
                self.history.insert(agent, messages);
                println!();
                return self.run_planning_workflow(&requirement).await;
            }

            messages.push(response);
            if round + 1 >= MAX_TOOL_ROUNDS {
                bail!("Planner exceeded interactive project-tool rounds");
            }
            messages.extend(dispatch_tool_calls(agent, &mut self.tool_runtime, &calls));
        }

        unreachable!("bounded Planner conversation loop must return or fail")
    }

    async fn run_planning_workflow(&mut self, requirement: &str) -> Result<()> {
        if let Some(state) = self.registry.planning_run_state()? {
            if state.stage == "LEGACY_RECOVERY_REQUIRED" {
                println!(
                    "LEGACY_RECOVERY_REQUIRED: previous incomplete planning history is preserved."
                );
                println!(
                    "To abandon that old planning cursor and start the exact new requirement, type: ABANDON LEGACY PLANNING"
                );
                println!("Any other response (including empty) cancels without database changes.");
                print!("Human confirmation: ");
                io::stdout().flush()?;
                let mut response = String::new();
                io::stdin().read_line(&mut response)?;
                if response.trim() != "ABANDON LEGACY PLANNING" {
                    println!("LEGACY_RECOVERY_CANCELLED: previous planning history unchanged.");
                    return Ok(());
                }
                self.registry
                    .confirm_legacy_planning_recovery(requirement, true)?;
                println!("LEGACY_PLANNING_ABANDONED: old snapshot retained in durable history.");
            }
        }
        println!("Planning workflow: Planner → Reviewer → Local CR");
        let workflow = PlanningWorkflow::new(
            &self.ollama,
            &self.harnesses,
            &self.registry,
            &self.config,
            &self.session,
            &self.project_root,
        );
        match workflow.run(requirement).await? {
            PlanningOutcome::Registered {
                plan,
                graph_version,
            } => {
                println!(
                    "PLAN_APPROVED revision={} hash={}",
                    plan.revision, plan.hash
                );
                println!("EXECUTION_GRAPH_REGISTERED version={graph_version}");
                self.active_work = self.resolve_coder_work_after_testers().await?;
                if let Some(work) = &self.active_work {
                    println!(
                        "ACTIVE_WORK milestone={} jobpack={}",
                        work.milestone_id, work.jobpack_id
                    );
                }
            }
            PlanningOutcome::Paused { revision, .. } => {
                println!(
                    "Planning workflow PAUSED after bounded review attempts (revision={:?}).",
                    revision
                );
            }
        }
        Ok(())
    }

    async fn dispatch_user_text(&mut self, text: String) -> Result<()> {
        let agent = self.session.active_agent;

        if agent == AgentId::Planner {
            return self.dispatch_planner_text(text).await;
        }

        if agent == AgentId::Coder {
            self.active_work = self.resolve_coder_work_after_testers().await?;
            let active_work = self
                .active_work
                .clone()
                .context("no ACTIVE Job Pack is available for Coder")?;

            println!(
                "Coding workflow: Coder → Reviewer (milestone={} jobpack={})",
                active_work.milestone_id, active_work.jobpack_id
            );
            let workflow = CodingWorkflow::new(
                &self.ollama,
                &self.harnesses,
                &self.registry,
                &self.config,
                &self.session,
                &self.project_root,
                &self.lease_owner,
            );
            match workflow
                .run(&text, &active_work, &mut self.tool_runtime)
                .await?
            {
                CodingOutcome::ReviewPass { change_set_id } => {
                    println!(
                        "CODE_REVIEW_PASS jobpack={} change_set={}",
                        active_work.jobpack_id, change_set_id
                    );
                    let changed_paths = self
                        .tool_runtime
                        .review_evidence()
                        .into_iter()
                        .map(|item| item.path)
                        .collect::<Vec<_>>();
                    let verification = VerificationController::new(
                        &self.registry,
                        &self.project_root,
                        &self.lease_owner,
                    )
                    .verify(
                        active_work.graph_version,
                        &active_work.jobpack_id,
                        &change_set_id,
                        &changed_paths,
                    )?;
                    println!(
                        "VERIFICATION_RESULT jobpack={} change_set={} result={}",
                        active_work.jobpack_id,
                        change_set_id,
                        verification.as_str()
                    );
                    self.active_work = self.resolve_coder_work_after_testers().await?;
                    println!("Job Pack remains ACTIVE pending CR and later terminal gates.");
                }
                CodingOutcome::Paused {
                    change_set_id,
                    reason,
                } => {
                    println!(
                        "Coding workflow PAUSED jobpack={} change_set={:?}: {}",
                        active_work.jobpack_id, change_set_id, reason
                    );
                }
            }
            return Ok(());
        }

        if agent == AgentId::Tester {
            bail!("Tester is orchestration-owned and may run only from a declared Test Checkpoint");
        }

        let model = resolve_model_name(&self.ollama, &self.session, &self.config, agent).await?;

        let history = self.history.entry(agent).or_insert_with(|| {
            let mut system = self
                .harnesses
                .compose(agent)
                .unwrap_or_else(|error| format!("Harness load error: {error}"));
            system.push_str(&format!(
                "\n\nPROJECT ROOT: {}\nStay inside this project boundary.",
                self.project_root.display()
            ));
            vec![ChatMessage::system(system)]
        });

        history.push(ChatMessage::user(text));
        print!("{} [{}]: ", agent.display_name(), model);
        io::stdout().flush()?;

        let journal_before = self.tool_runtime.journal().len();
        let result = run_with_project_tools(
            &self.ollama,
            &model,
            agent,
            history,
            &mut self.tool_runtime,
            |token| {
                print!("{token}");
                let _ = io::stdout().flush();
            },
        )
        .await?;
        println!();

        if self.tool_runtime.journal().len() > journal_before {
            println!("Change set: {}", result.change_set_id);
        }
        Ok(())
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self
            .registry
            .release_lease(&self.project_root, &self.lease_owner);
    }
}

fn is_explicit_planning_workflow_request(text: &str) -> bool {
    let normalized = text.to_lowercase();

    const NEGATIONS: &[&str] = &[
        "do not create a plan",
        "don't create a plan",
        "do not create an implementation plan",
        "don't create an implementation plan",
        "không tạo plan",
        "đừng tạo plan",
        "không lập kế hoạch",
        "đừng lập kế hoạch",
    ];
    if NEGATIONS.iter().any(|phrase| normalized.contains(phrase)) {
        return false;
    }

    const EXPLICIT: &[&str] = &[
        "create an implementation plan",
        "create the implementation plan",
        "make an implementation plan",
        "produce an implementation plan",
        "prepare an implementation plan",
        "draft an implementation plan",
        "start an implementation plan",
        "revise the implementation plan",
        "revise an implementation plan",
        "update the implementation plan",
        "lập kế hoạch triển khai",
        "tạo kế hoạch triển khai",
        "lên kế hoạch triển khai",
        "hãy lên plan",
        "lên plan",
        "tạo plan",
        "lập plan",
    ];

    EXPLICIT.iter().any(|phrase| normalized.contains(phrase))
}

fn planning_start_tool() -> ToolDefinition {
    ToolDefinition::function(
        "start_planning_workflow",
        "Start the durable Planner -> Reviewer -> Local CR -> Job Builder workflow for an explicit implementation-planning request. Do not call this tool for greetings, status questions, explanations, or discussion.",
        serde_json::json!({
            "type": "object",
            "required": ["requirement"],
            "properties": {
                "requirement": {
                    "type": "string",
                    "minLength": 1,
                    "description": "The user's explicit implementation-planning requirement."
                }
            }
        }),
    )
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_owned())
}

fn parse_index(value: &str, len: usize) -> Result<usize> {
    let selected: usize = value
        .trim()
        .parse()
        .with_context(|| format!("expected a number between 1 and {len}"))?;
    if selected == 0 || selected > len {
        bail!("selection must be between 1 and {len}");
    }
    Ok(selected - 1)
}

fn apply_default_model_choice(
    config: &mut AppConfig,
    models: &[String],
    choice: &str,
) -> Result<()> {
    let choice = choice.trim();
    if choice.is_empty() {
        return Ok(());
    }
    if choice.eq_ignore_ascii_case("none") || choice.eq_ignore_ascii_case("clear") {
        config.set_default_model(None);
        return Ok(());
    }
    let model = models[parse_index(choice, models.len())?].clone();
    config.set_default_model(Some(model));
    Ok(())
}

fn print_models(models: &[String]) {
    println!("MODELS");
    for (index, model) in models.iter().enumerate() {
        println!("{}. {}", index + 1, model);
    }
}

#[cfg(test)]
mod planner_routing_tests {
    use super::{apply_default_model_choice, is_explicit_planning_workflow_request};
    use crate::config::AppConfig;

    #[test]
    fn explicit_implementation_plan_request_fast_paths_workflow() {
        assert!(is_explicit_planning_workflow_request(
            "Create an implementation plan for a documentation-only improvement."
        ));
        assert!(is_explicit_planning_workflow_request(
            "Hãy lên plan cho thay đổi này."
        ));
    }

    #[test]
    fn blank_default_choice_does_not_invent_a_model() {
        let mut config = AppConfig::default();
        let models = vec!["one".to_owned(), "two".to_owned()];

        apply_default_model_choice(&mut config, &models, "").unwrap();

        assert_eq!(config.default_model, None);
    }

    #[test]
    fn default_model_requires_explicit_selection_and_can_be_cleared() {
        let mut config = AppConfig::default();
        let models = vec!["one".to_owned(), "two".to_owned()];

        apply_default_model_choice(&mut config, &models, "2").unwrap();
        assert_eq!(config.default_model.as_deref(), Some("two"));

        apply_default_model_choice(&mut config, &models, "none").unwrap();
        assert_eq!(config.default_model, None);
    }

    #[test]
    fn conversation_and_negative_requests_do_not_fast_path_workflow() {
        assert!(!is_explicit_planning_workflow_request("hello?"));
        assert!(!is_explicit_planning_workflow_request(
            "Explain the current planning architecture."
        ));
        assert!(!is_explicit_planning_workflow_request(
            "Do not create an implementation plan; just discuss options."
        ));
    }
}
