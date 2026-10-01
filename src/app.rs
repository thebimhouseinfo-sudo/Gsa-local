use crate::{
    agent_runtime::run_with_project_tools,
    cli::{self, InputLine, SlashCommand},
    config::AppConfig,
    controller::{ActiveWork, MilestoneController, NextWork},
    harness::{AgentId, HarnessRegistry},
    ollama::{ChatMessage, OllamaClient},
    registry::{Registry, TesterCheckpointDisposition},
    session::Session,
    tester_execution::{tester_capability_catalog, TesterWorkflow},
    tools::ProjectToolRuntime,
    verification::{discover_profile, VerificationController},
    workflow::{CodingOutcome, CodingWorkflow, PlanningOutcome, PlanningWorkflow},
};
use anyhow::{bail, Context, Result};
use std::{
    collections::HashMap,
    io::{self, Write},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

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
        let ollama = OllamaClient::new(config.ollama_base_url.clone());
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

        if self.config.default_model.is_none() {
            self.config.default_model = Some(models[0].clone());
        }

        for agent in AgentId::user_selectable() {
            let current = self.config.model_for(*agent).unwrap_or("<none>");
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
        loop {
            let profile = discover_profile(&self.project_root)?;
            let available_capabilities = tester_capability_catalog(&profile);
            let next =
                MilestoneController::new(&self.registry, &self.project_root, &self.lease_owner)
                    .resolve_next(&available_capabilities)?;

            match next {
                None => return Ok(None),
                Some(NextWork::Coder(work)) => return Ok(Some(work)),
                Some(NextWork::Tester(checkpoint_work)) => {
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
                    println!(
                        "TEST_CHECKPOINT_DUE checkpoint={} attempt={}",
                        checkpoint_work.checkpoint.id, attempt_id
                    );
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
                            &checkpoint_work.checkpoint.goal,
                            checkpoint_work.graph_version,
                            &checkpoint_work.checkpoint,
                            target,
                            attempt_id,
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

    async fn dispatch_user_text(&mut self, text: String) -> Result<()> {
        let agent = self.session.active_agent;

        if agent == AgentId::Planner {
            println!("Planning workflow: Planner → Reviewer → Local CR");
            let workflow = PlanningWorkflow::new(
                &self.ollama,
                &self.harnesses,
                &self.registry,
                &self.config,
                &self.session,
                &self.project_root,
            );
            match workflow.run(&text).await? {
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
            return Ok(());
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

        let model = match self.session.resolved_model(&self.config, agent) {
            Some(model) => model.to_owned(),
            None => self
                .ollama
                .list_models()
                .await?
                .into_iter()
                .next()
                .context("no Ollama model is configured or installed")?,
        };

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

fn print_models(models: &[String]) {
    println!("MODELS");
    for (index, model) in models.iter().enumerate() {
        println!("{}. {}", index + 1, model);
    }
}
