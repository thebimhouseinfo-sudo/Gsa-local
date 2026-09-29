use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionGraph {
    pub milestones: Vec<MilestoneSpec>,
    pub jobpacks: Vec<JobPackSpec>,
    pub todos: Vec<TodoSpec>,
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
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();

    for id in dependencies.keys().copied() {
        visit(id, &dependencies, &mut visiting, &mut visited)?;
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
        }
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
