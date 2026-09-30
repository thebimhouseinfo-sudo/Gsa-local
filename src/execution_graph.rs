use crate::plan::{EvidenceMode, PlanArtifact};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionGraph {
    pub milestones: Vec<MilestoneSpec>,
    pub jobpacks: Vec<JobPackSpec>,
    pub todos: Vec<TodoSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<TestCheckpointSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_requirements: Vec<EvidenceRequirementSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MilestoneSpec {
    pub id: String,
    pub title: String,
    pub order: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobPackSpec {
    pub id: String,
    pub milestone_id: String,
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub todo_ids: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub required_inputs: Vec<String>,
    pub expected_outputs: Vec<String>,
    pub acceptance: Vec<String>,
    pub verification_hints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoSpec {
    pub id: String,
    pub jobpack_id: String,
    pub title: String,
    pub checklist: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckpointBoundaryKind {
    AfterJobpackSet,
    BeforeJobpack,
    MilestoneGate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrerequisiteState {
    ReviewPass,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointPrerequisiteSpec {
    pub jobpack_id: String,
    pub state: PrerequisiteState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceOutputSpec {
    pub id: String,
    pub mode: EvidenceMode,
    pub description: String,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_need_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCheckpointSpec {
    pub id: String,
    pub milestone_id: String,
    pub boundary: CheckpointBoundaryKind,
    #[serde(default)]
    pub prerequisites: Vec<CheckpointPrerequisiteSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_jobpack_id: Option<String>,
    #[serde(default)]
    pub evidence_need_ids: Vec<String>,
    pub modes: Vec<EvidenceMode>,
    pub goal: String,
    pub criteria: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub experiment_dimensions: Vec<String>,
    pub evidence_outputs: Vec<EvidenceOutputSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRequirementSpec {
    pub consumer_jobpack_id: String,
    pub checkpoint_id: String,
    pub output_id: String,
    pub required: bool,
}

impl ExecutionGraph {
    pub fn validate(&self) -> Result<()> {
        if self.milestones.is_empty() {
            bail!("execution graph must contain at least one milestone");
        }
        if self.jobpacks.is_empty() {
            bail!("execution graph must contain at least one job pack");
        }
        if self.todos.is_empty() {
            bail!("execution graph must contain at least one todo");
        }

        validate_nonempty_unique(
            "milestone",
            self.milestones.iter().map(|item| (&item.id, &item.title)),
        )?;
        validate_nonempty_unique(
            "jobpack",
            self.jobpacks.iter().map(|item| (&item.id, &item.title)),
        )?;
        validate_nonempty_unique(
            "todo",
            self.todos.iter().map(|item| (&item.id, &item.title)),
        )?;

        let milestone_orders: HashMap<&str, u32> = self
            .milestones
            .iter()
            .map(|milestone| (milestone.id.as_str(), milestone.order))
            .collect();
        validate_milestone_orders(&self.milestones)?;

        let jobpacks: HashMap<&str, &JobPackSpec> = self
            .jobpacks
            .iter()
            .map(|jobpack| (jobpack.id.as_str(), jobpack))
            .collect();
        let todos: HashMap<&str, &TodoSpec> = self
            .todos
            .iter()
            .map(|todo| (todo.id.as_str(), todo))
            .collect();

        let mut packs_per_milestone: HashMap<&str, usize> = HashMap::new();
        for pack in &self.jobpacks {
            require_text("jobpack goal", &pack.goal)?;
            require_items("jobpack required_inputs", &pack.required_inputs)?;
            require_items("jobpack expected_outputs", &pack.expected_outputs)?;
            require_items("jobpack acceptance", &pack.acceptance)?;
            require_items("jobpack verification_hints", &pack.verification_hints)?;
            if pack.todo_ids.is_empty() {
                bail!("jobpack {} must reference at least one todo", pack.id);
            }
            if !milestone_orders.contains_key(pack.milestone_id.as_str()) {
                bail!(
                    "jobpack {} references missing milestone {}",
                    pack.id,
                    pack.milestone_id
                );
            }
            *packs_per_milestone
                .entry(pack.milestone_id.as_str())
                .or_default() += 1;

            let mut local_todos = HashSet::new();
            for todo_id in &pack.todo_ids {
                if !local_todos.insert(todo_id.as_str()) {
                    bail!("jobpack {} contains duplicate todo {}", pack.id, todo_id);
                }
                let Some(todo) = todos.get(todo_id.as_str()) else {
                    bail!("jobpack {} references missing todo {}", pack.id, todo_id);
                };
                if todo.jobpack_id != pack.id {
                    bail!(
                        "todo {} belongs to jobpack {}, not {}",
                        todo.id,
                        todo.jobpack_id,
                        pack.id
                    );
                }
            }

            let current_order = milestone_orders[pack.milestone_id.as_str()];
            let mut local_dependencies = HashSet::new();
            for dependency in &pack.depends_on {
                if dependency == &pack.id {
                    bail!("jobpack {} cannot depend on itself", pack.id);
                }
                if !local_dependencies.insert(dependency.as_str()) {
                    bail!(
                        "jobpack {} contains duplicate dependency {}",
                        pack.id,
                        dependency
                    );
                }
                let Some(dep_pack) = jobpacks.get(dependency.as_str()) else {
                    bail!(
                        "jobpack {} references missing dependency {}",
                        pack.id,
                        dependency
                    );
                };
                let dependency_order = milestone_orders[dep_pack.milestone_id.as_str()];
                if dependency_order > current_order {
                    bail!(
                        "jobpack {} in milestone order {} cannot depend on later jobpack {} in order {}",
                        pack.id,
                        current_order,
                        dependency,
                        dependency_order
                    );
                }
            }
        }

        for milestone in &self.milestones {
            if packs_per_milestone
                .get(milestone.id.as_str())
                .copied()
                .unwrap_or(0)
                == 0
            {
                bail!("milestone {} is empty", milestone.id);
            }
        }

        let mut referenced_todos = HashSet::new();
        for pack in &self.jobpacks {
            for todo_id in &pack.todo_ids {
                referenced_todos.insert(todo_id.as_str());
            }
        }
        for todo in &self.todos {
            require_items("todo checklist", &todo.checklist)?;
            if !jobpacks.contains_key(todo.jobpack_id.as_str()) {
                bail!(
                    "todo {} references missing jobpack {}",
                    todo.id,
                    todo.jobpack_id
                );
            }
            if !referenced_todos.contains(todo.id.as_str()) {
                bail!("todo {} is orphaned", todo.id);
            }
        }

        validate_dependency_cycles(&self.jobpacks)?;
        self.validate_checkpoints(&milestone_orders, &jobpacks)?;
        Ok(())
    }

    pub fn validate_against_plan(&self, plan: &PlanArtifact) -> Result<()> {
        self.validate()?;
        plan.validate()?;

        let needs = plan
            .evidence_needs
            .iter()
            .map(|need| (need.id.as_str(), need))
            .collect::<HashMap<_, _>>();
        let mut mapped_required = HashSet::new();

        for checkpoint in &self.checkpoints {
            for need_id in &checkpoint.evidence_need_ids {
                let Some(need) = needs.get(need_id.as_str()) else {
                    bail!(
                        "checkpoint {} references unknown evidence_need {}",
                        checkpoint.id,
                        need_id
                    );
                };
            }

            for output in &checkpoint.evidence_outputs {
                if let Some(need_id) = output.evidence_need_id.as_deref() {
                    let Some(need) = needs.get(need_id) else {
                        bail!(
                            "checkpoint {} output {} references unknown evidence_need {}",
                            checkpoint.id,
                            output.id,
                            need_id
                        );
                    };
                    if !checkpoint
                        .evidence_need_ids
                        .iter()
                        .any(|id| id == need_id)
                    {
                        bail!(
                            "checkpoint {} output {} maps evidence_need {} that is not declared by the checkpoint",
                            checkpoint.id,
                            output.id,
                            need_id
                        );
                    }
                    if !need.modes.contains(&output.mode) {
                        bail!(
                            "checkpoint {} output {} mode is incompatible with evidence_need {}",
                            checkpoint.id,
                            output.id,
                            need_id
                        );
                    }
                    if need.required && output.required {
                        mapped_required.insert(need.id.as_str());
                    }
                }
            }
        }

        for need in &plan.evidence_needs {
            if need.required && !mapped_required.contains(need.id.as_str()) {
                bail!(
                    "required evidence_need {} is not mapped to a required checkpoint output",
                    need.id
                );
            }
        }
        Ok(())
    }

    fn validate_checkpoints(
        &self,
        milestone_orders: &HashMap<&str, u32>,
        jobpacks: &HashMap<&str, &JobPackSpec>,
    ) -> Result<()> {
        let mut checkpoint_ids = HashSet::new();
        let mut checkpoint_map = HashMap::new();

        for checkpoint in &self.checkpoints {
            require_text("checkpoint id", &checkpoint.id)?;
            require_text("checkpoint goal", &checkpoint.goal)?;
            require_items("checkpoint criteria", &checkpoint.criteria)?;
            if !checkpoint_ids.insert(checkpoint.id.as_str()) {
                bail!("duplicate checkpoint id {}", checkpoint.id);
            }
            if !milestone_orders.contains_key(checkpoint.milestone_id.as_str()) {
                bail!(
                    "checkpoint {} references missing milestone {}",
                    checkpoint.id,
                    checkpoint.milestone_id
                );
            }
            if checkpoint.modes.is_empty() {
                bail!("checkpoint {} must declare at least one mode", checkpoint.id);
            }
            let mut local_modes = HashSet::new();
            for mode in &checkpoint.modes {
                if !local_modes.insert(*mode) {
                    bail!("checkpoint {} contains duplicate mode", checkpoint.id);
                }
            }

            let mut local_needs = HashSet::new();
            for need_id in &checkpoint.evidence_need_ids {
                require_text("checkpoint evidence_need_id", need_id)?;
                if !local_needs.insert(need_id.as_str()) {
                    bail!(
                        "checkpoint {} contains duplicate evidence_need_id {}",
                        checkpoint.id,
                        need_id
                    );
                }
            }

            let mut local_prereqs = HashSet::new();
            for prerequisite in &checkpoint.prerequisites {
                require_text("checkpoint prerequisite jobpack_id", &prerequisite.jobpack_id)?;
                if !local_prereqs.insert(prerequisite.jobpack_id.as_str()) {
                    bail!(
                        "checkpoint {} contains duplicate prerequisite {}",
                        checkpoint.id,
                        prerequisite.jobpack_id
                    );
                }
                let Some(pack) = jobpacks.get(prerequisite.jobpack_id.as_str()) else {
                    bail!(
                        "checkpoint {} references missing prerequisite jobpack {}",
                        checkpoint.id,
                        prerequisite.jobpack_id
                    );
                };
                if pack.milestone_id != checkpoint.milestone_id {
                    bail!(
                        "checkpoint {} prerequisite {} must belong to milestone {}",
                        checkpoint.id,
                        prerequisite.jobpack_id,
                        checkpoint.milestone_id
                    );
                }
            }

            match checkpoint.boundary {
                CheckpointBoundaryKind::AfterJobpackSet
                | CheckpointBoundaryKind::MilestoneGate => {
                    if checkpoint.prerequisites.is_empty() {
                        bail!(
                            "checkpoint {} boundary requires at least one prerequisite",
                            checkpoint.id
                        );
                    }
                    if checkpoint.before_jobpack_id.is_some() {
                        bail!(
                            "checkpoint {} boundary must not set before_jobpack_id",
                            checkpoint.id
                        );
                    }
                }
                CheckpointBoundaryKind::BeforeJobpack => {
                    let before = checkpoint.before_jobpack_id.as_deref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "checkpoint {} BEFORE_JOBPACK requires before_jobpack_id",
                            checkpoint.id
                        )
                    })?;
                    let Some(pack) = jobpacks.get(before) else {
                        bail!(
                            "checkpoint {} references missing before_jobpack {}",
                            checkpoint.id,
                            before
                        );
                    };
                    if pack.milestone_id != checkpoint.milestone_id {
                        bail!(
                            "checkpoint {} before_jobpack {} must belong to milestone {}",
                            checkpoint.id,
                            before,
                            checkpoint.milestone_id
                        );
                    }
                    if local_prereqs.contains(before) {
                        bail!(
                            "checkpoint {} cannot require the same jobpack it gates before",
                            checkpoint.id
                        );
                    }
                }
            }

            if checkpoint.evidence_outputs.is_empty() {
                bail!(
                    "checkpoint {} must declare at least one evidence output",
                    checkpoint.id
                );
            }
            let mut output_ids = HashSet::new();
            for output in &checkpoint.evidence_outputs {
                require_text("checkpoint evidence output id", &output.id)?;
                require_text("checkpoint evidence output description", &output.description)?;
                if !output_ids.insert(output.id.as_str()) {
                    bail!(
                        "checkpoint {} contains duplicate evidence output {}",
                        checkpoint.id,
                        output.id
                    );
                }
                if !checkpoint.modes.contains(&output.mode) {
                    bail!(
                        "checkpoint {} output {} uses a mode not declared by the checkpoint",
                        checkpoint.id,
                        output.id
                    );
                }
                if let Some(need_id) = output.evidence_need_id.as_deref() {
                    if !local_needs.contains(need_id) {
                        bail!(
                            "checkpoint {} output {} references undeclared evidence_need {}",
                            checkpoint.id,
                            output.id,
                            need_id
                        );
                    }
                }
            }

            checkpoint_map.insert(checkpoint.id.as_str(), checkpoint);
        }

        let mut requirement_keys = HashSet::new();
        let mut augmented_dependencies: HashMap<&str, Vec<&str>> = self
            .jobpacks
            .iter()
            .map(|pack| {
                (
                    pack.id.as_str(),
                    pack.depends_on.iter().map(String::as_str).collect(),
                )
            })
            .collect();

        for checkpoint in &self.checkpoints {
            if checkpoint.boundary == CheckpointBoundaryKind::BeforeJobpack {
                let before = checkpoint
                    .before_jobpack_id
                    .as_deref()
                    .expect("BEFORE_JOBPACK was validated to have before_jobpack_id");
                for prerequisite in &checkpoint.prerequisites {
                    augmented_dependencies
                        .entry(before)
                        .or_default()
                        .push(prerequisite.jobpack_id.as_str());
                }
            }
        }

        for requirement in &self.evidence_requirements {
            require_text(
                "evidence requirement consumer_jobpack_id",
                &requirement.consumer_jobpack_id,
            )?;
            require_text(
                "evidence requirement checkpoint_id",
                &requirement.checkpoint_id,
            )?;
            require_text("evidence requirement output_id", &requirement.output_id)?;
            if !jobpacks.contains_key(requirement.consumer_jobpack_id.as_str()) {
                bail!(
                    "evidence requirement references missing consumer jobpack {}",
                    requirement.consumer_jobpack_id
                );
            }
            let Some(checkpoint) = checkpoint_map.get(requirement.checkpoint_id.as_str()) else {
                bail!(
                    "evidence requirement references missing checkpoint {}",
                    requirement.checkpoint_id
                );
            };
            if !checkpoint
                .evidence_outputs
                .iter()
                .any(|output| output.id == requirement.output_id)
            {
                bail!(
                    "evidence requirement references missing output {}.{}",
                    requirement.checkpoint_id,
                    requirement.output_id
                );
            }
            let key = (
                requirement.consumer_jobpack_id.as_str(),
                requirement.checkpoint_id.as_str(),
                requirement.output_id.as_str(),
            );
            if !requirement_keys.insert(key) {
                bail!(
                    "duplicate evidence requirement for consumer {} and output {}.{}",
                    requirement.consumer_jobpack_id,
                    requirement.checkpoint_id,
                    requirement.output_id
                );
            }

            if checkpoint
                .prerequisites
                .iter()
                .any(|prerequisite| prerequisite.jobpack_id == requirement.consumer_jobpack_id)
            {
                bail!(
                    "evidence requirement would make consumer {} depend on its own checkpoint prerequisite",
                    requirement.consumer_jobpack_id
                );
            }

            let consumer = jobpacks[requirement.consumer_jobpack_id.as_str()];
            let consumer_order = milestone_orders[consumer.milestone_id.as_str()];
            let checkpoint_order = milestone_orders[checkpoint.milestone_id.as_str()];
            if consumer_order < checkpoint_order {
                bail!(
                    "consumer {} cannot require evidence from later checkpoint {}",
                    requirement.consumer_jobpack_id,
                    requirement.checkpoint_id
                );
            }
            for prerequisite in &checkpoint.prerequisites {
                augmented_dependencies
                    .entry(requirement.consumer_jobpack_id.as_str())
                    .or_default()
                    .push(prerequisite.jobpack_id.as_str());
            }
        }

        validate_dependency_map_cycles(&augmented_dependencies)?;
        Ok(())
    }
}

fn validate_nonempty_unique<'a>(
    kind: &str,
    items: impl Iterator<Item = (&'a String, &'a String)>,
) -> Result<()> {
    let mut ids = HashSet::new();
    for (id, title) in items {
        require_text(&format!("{kind} id"), id)?;
        require_text(&format!("{kind} title"), title)?;
        if !ids.insert(id.as_str()) {
            bail!("duplicate {kind} id {id}");
        }
    }
    Ok(())
}

fn validate_milestone_orders(milestones: &[MilestoneSpec]) -> Result<()> {
    let mut orders = HashSet::new();
    for milestone in milestones {
        if milestone.order == 0 {
            bail!("milestone {} order must start at 1", milestone.id);
        }
        if !orders.insert(milestone.order) {
            bail!("duplicate milestone order {}", milestone.order);
        }
    }

    for expected in 1..=milestones.len() as u32 {
        if !orders.contains(&expected) {
            bail!("milestone orders must be contiguous from 1; missing {expected}");
        }
    }
    Ok(())
}

fn validate_dependency_cycles(jobpacks: &[JobPackSpec]) -> Result<()> {
    let dependencies: HashMap<&str, Vec<&str>> = jobpacks
        .iter()
        .map(|pack| {
            (
                pack.id.as_str(),
                pack.depends_on.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    validate_dependency_map_cycles(&dependencies)
}

fn validate_dependency_map_cycles<'a>(
    dependencies: &HashMap<&'a str, Vec<&'a str>>,
) -> Result<()> {
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for id in dependencies.keys().copied() {
        visit(id, dependencies, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn visit<'a>(
    id: &'a str,
    dependencies: &HashMap<&'a str, Vec<&'a str>>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> Result<()> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id) {
        bail!("jobpack dependency cycle detected at {id}");
    }

    if let Some(items) = dependencies.get(id) {
        for dependency in items {
            visit(dependency, dependencies, visiting, visited)?;
        }
    }
    visiting.remove(id);
    visited.insert(id);
    Ok(())
}

fn require_text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{name} must not be empty");
    }
    Ok(())
}

fn require_items(name: &str, values: &[String]) -> Result<()> {
    if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
        bail!("{name} must contain non-empty items");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_graph() -> ExecutionGraph {
        ExecutionGraph {
            milestones: vec![
                MilestoneSpec {
                    id: "M1".into(),
                    title: "Foundation".into(),
                    order: 1,
                },
                MilestoneSpec {
                    id: "M2".into(),
                    title: "Workflow".into(),
                    order: 2,
                },
            ],
            jobpacks: vec![
                JobPackSpec {
                    id: "JP1".into(),
                    milestone_id: "M1".into(),
                    title: "Base".into(),
                    goal: "Build base".into(),
                    todo_ids: vec!["T1".into()],
                    depends_on: vec![],
                    required_inputs: vec!["approved inputs".into()],
                    expected_outputs: vec!["foundation output".into()],
                    acceptance: vec!["Base works".into()],
                    verification_hints: vec!["cargo test".into()],
                },
                JobPackSpec {
                    id: "JP2".into(),
                    milestone_id: "M2".into(),
                    title: "Flow".into(),
                    goal: "Build flow".into(),
                    todo_ids: vec!["T2".into()],
                    depends_on: vec!["JP1".into()],
                    required_inputs: vec!["foundation output".into()],
                    expected_outputs: vec!["workflow output".into()],
                    acceptance: vec!["Flow works".into()],
                    verification_hints: vec!["integration test".into()],
                },
            ],
            todos: vec![
                TodoSpec {
                    id: "T1".into(),
                    jobpack_id: "JP1".into(),
                    title: "Base todo".into(),
                    checklist: vec!["done".into()],
                },
                TodoSpec {
                    id: "T2".into(),
                    jobpack_id: "JP2".into(),
                    title: "Flow todo".into(),
                    checklist: vec!["done".into()],
                },
            ],
            checkpoints: vec![],
            evidence_requirements: vec![],
        }
    }

    #[test]
    fn checkpoint_validation_supports_multi_jobpack_prerequisites() {
        let mut graph = valid_graph();
        graph.jobpacks[1].milestone_id = "M1".into();
        graph.milestones.pop();
        graph.jobpacks[1].depends_on.clear();
        graph.checkpoints.push(TestCheckpointSpec {
            id: "CP1".into(),
            milestone_id: "M1".into(),
            boundary: CheckpointBoundaryKind::MilestoneGate,
            prerequisites: vec![
                CheckpointPrerequisiteSpec {
                    jobpack_id: "JP1".into(),
                    state: PrerequisiteState::ReviewPass,
                },
                CheckpointPrerequisiteSpec {
                    jobpack_id: "JP2".into(),
                    state: PrerequisiteState::ReviewPass,
                },
            ],
            before_jobpack_id: None,
            evidence_need_ids: vec![],
            modes: vec![EvidenceMode::Verify],
            goal: "Verify integrated milestone behavior".into(),
            criteria: vec!["Integrated behavior works".into()],
            required_capabilities: vec!["INTEGRATION".into()],
            experiment_dimensions: vec![],
            evidence_outputs: vec![EvidenceOutputSpec {
                id: "integration-verdict".into(),
                mode: EvidenceMode::Verify,
                description: "Observed integrated behavior verdict".into(),
                required: true,
                evidence_need_id: None,
            }],
        });
        graph.validate().unwrap();
    }

    #[test]
    fn required_plan_evidence_need_must_map_to_required_output() {
        let mut graph = valid_graph();
        graph.checkpoints.push(TestCheckpointSpec {
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
            criteria: vec!["Observation captured".into()],
            required_capabilities: vec![],
            experiment_dimensions: vec!["session boundary".into()],
            evidence_outputs: vec![EvidenceOutputSpec {
                id: "runtime-identity".into(),
                mode: EvidenceMode::Probe,
                description: "Observed runtime identity behavior".into(),
                required: true,
                evidence_need_id: Some("runtime-id".into()),
            }],
        });
        let mut plan = crate::plan::PlanArtifact {
            goal: "Use measured runtime identity".into(),
            current_architecture: "unknown identity lifetime".into(),
            required_changes: vec!["measure identity".into()],
            implementation_approach: vec!["probe before consumer".into()],
            dependencies: vec![],
            sequence: vec!["probe".into(), "consume".into()],
            risks: vec!["guessed id".into()],
            acceptance_direction: vec!["observed evidence only".into()],
            evidence_needs: vec![crate::plan::EvidenceNeed {
                id: "runtime-id".into(),
                question: "Which id is stable?".into(),
                purpose: "Choose a real binding key.".into(),
                required: true,
                consumer: "session controller".into(),
                modes: vec![EvidenceMode::Probe],
                intent: "Observe identity across boundaries.".into(),
            }],
        };
        graph.validate_against_plan(&plan).unwrap();
        graph.checkpoints[0].evidence_outputs[0].required = false;
        assert!(graph.validate_against_plan(&plan).is_err());
        plan.evidence_needs[0].required = false;
        graph.validate_against_plan(&plan).unwrap();
    }

    #[test]
    fn required_need_declaration_without_required_output_is_rejected() {
        let mut graph = valid_graph();
        graph.checkpoints.push(TestCheckpointSpec {
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
            criteria: vec!["Observation captured".into()],
            required_capabilities: vec![],
            experiment_dimensions: vec!["session boundary".into()],
            evidence_outputs: vec![EvidenceOutputSpec {
                id: "unrelated-output".into(),
                mode: EvidenceMode::Probe,
                description: "Probe output that does not satisfy the required need".into(),
                required: true,
                evidence_need_id: None,
            }],
        });
        let plan = crate::plan::PlanArtifact {
            goal: "Use measured runtime identity".into(),
            current_architecture: "unknown identity lifetime".into(),
            required_changes: vec!["measure identity".into()],
            implementation_approach: vec!["probe before consumer".into()],
            dependencies: vec![],
            sequence: vec!["probe".into(), "consume".into()],
            risks: vec!["guessed id".into()],
            acceptance_direction: vec!["observed evidence only".into()],
            evidence_needs: vec![crate::plan::EvidenceNeed {
                id: "runtime-id".into(),
                question: "Which id is stable?".into(),
                purpose: "Choose a real binding key.".into(),
                required: true,
                consumer: "session controller".into(),
                modes: vec![EvidenceMode::Probe],
                intent: "Observe identity across boundaries.".into(),
            }],
        };

        assert!(graph.validate_against_plan(&plan).is_err());
    }

    #[test]
    fn before_jobpack_checkpoint_rejects_implicit_dependency_cycle() {
        let mut graph = valid_graph();
        graph.jobpacks[1].milestone_id = "M1".into();
        graph.milestones.pop();
        graph.checkpoints.push(TestCheckpointSpec {
            id: "CP1".into(),
            milestone_id: "M1".into(),
            boundary: CheckpointBoundaryKind::BeforeJobpack,
            prerequisites: vec![CheckpointPrerequisiteSpec {
                jobpack_id: "JP2".into(),
                state: PrerequisiteState::ReviewPass,
            }],
            before_jobpack_id: Some("JP1".into()),
            evidence_need_ids: vec![],
            modes: vec![EvidenceMode::Verify],
            goal: "Gate JP1 on reviewed JP2".into(),
            criteria: vec!["Gate is topologically possible".into()],
            required_capabilities: vec![],
            experiment_dimensions: vec![],
            evidence_outputs: vec![EvidenceOutputSpec {
                id: "gate-verdict".into(),
                mode: EvidenceMode::Verify,
                description: "Gate result".into(),
                required: true,
                evidence_need_id: None,
            }],
        });

        assert!(graph.validate().is_err());
    }

    #[test]
    fn legacy_graph_json_defaults_checkpoint_fields() {
        let mut value = serde_json::to_value(valid_graph()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("checkpoints");
        object.remove("evidence_requirements");

        let graph: ExecutionGraph = serde_json::from_value(value).unwrap();
        assert!(graph.checkpoints.is_empty());
        assert!(graph.evidence_requirements.is_empty());
        graph.validate().unwrap();
    }

    #[test]
    fn valid_graph_passes() {
        valid_graph().validate().unwrap();
    }

    #[test]
    fn rejects_dependency_cycle() {
        let mut graph = valid_graph();
        graph.jobpacks[0].depends_on.push("JP2".into());
        assert!(graph.validate().is_err());
    }

    #[test]
    fn rejects_empty_milestone() {
        let mut graph = valid_graph();
        graph.jobpacks.retain(|pack| pack.milestone_id != "M2");
        graph.todos.retain(|todo| todo.jobpack_id != "JP2");
        assert!(graph.validate().is_err());
    }
}
