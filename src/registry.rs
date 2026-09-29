use crate::{
    checkpoint::Checkpoint,
    execution_graph::ExecutionGraph,
    plan::{PlanArtifact, PlanRevision},
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanBinding {
    pub revision: i64,
    pub hash: String,
    pub execution_graph_version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewActor {
    Reviewer,
    LocalCr,
}

impl ReviewActor {
    fn as_str(self) -> &'static str {
        match self {
            Self::Reviewer => "REVIEWER",
            Self::LocalCr => "LOCAL_CR",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewVerdict {
    Pass,
    Revise,
}

impl ReviewVerdict {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Revise => "REVISE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanWorkflowState {
    pub current_revision: Option<i64>,
    pub reviewer_attempts: u32,
    pub cr_attempts: u32,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWorkRecord {
    pub graph_version: i64,
    pub plan_revision: i64,
    pub plan_hash: String,
    pub milestone_id: String,
    pub milestone_title: String,
    pub milestone_status: String,
    pub jobpack_id: String,
    pub jobpack_title: String,
    pub jobpack_status: String,
    pub goal: String,
    pub required_inputs: Vec<String>,
    pub expected_outputs: Vec<String>,
    pub acceptance: Vec<String>,
    pub verification_hints: Vec<String>,
}

pub struct Registry {
    conn: Connection,
}

impl Registry {
    pub fn open(project_root: &Path) -> Result<Self> {
        let state_dir = project_root.join(".gsa").join("state");
        fs::create_dir_all(&state_dir)
            .with_context(|| format!("failed to create {}", state_dir.display()))?;
        Self::open_at(&state_dir.join("gsa.db"))
    }

    pub fn open_at(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open registry {}", path.display()))?;
        let registry = Self { conn };
        registry.init()?;
        Ok(registry)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            PRAGMA foreign_keys=ON;

            CREATE TABLE IF NOT EXISTS approved_plan (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                revision INTEGER NOT NULL,
                plan_hash TEXT NOT NULL,
                execution_graph_version INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS plan_revisions (
                revision INTEGER PRIMARY KEY,
                plan_hash TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS plan_verdicts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                actor TEXT NOT NULL,
                revision INTEGER NOT NULL,
                plan_hash TEXT NOT NULL,
                verdict TEXT NOT NULL,
                findings TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS plan_workflow_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                current_revision INTEGER,
                reviewer_attempts INTEGER NOT NULL DEFAULT 0,
                cr_attempts INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'IDLE'
            );

            CREATE TABLE IF NOT EXISTS execution_graph (
                version INTEGER PRIMARY KEY,
                plan_revision INTEGER NOT NULL,
                plan_hash TEXT NOT NULL,
                status TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS execution_milestones (
                graph_version INTEGER NOT NULL,
                milestone_id TEXT NOT NULL,
                title TEXT NOT NULL,
                position INTEGER NOT NULL,
                status TEXT NOT NULL,
                PRIMARY KEY (graph_version, milestone_id),
                FOREIGN KEY (graph_version) REFERENCES execution_graph(version) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS execution_jobpacks (
                graph_version INTEGER NOT NULL,
                jobpack_id TEXT NOT NULL,
                milestone_id TEXT NOT NULL,
                title TEXT NOT NULL,
                goal TEXT NOT NULL,
                required_inputs TEXT NOT NULL,
                expected_outputs TEXT NOT NULL,
                acceptance TEXT NOT NULL,
                verification_hints TEXT NOT NULL,
                status TEXT NOT NULL,
                PRIMARY KEY (graph_version, jobpack_id),
                FOREIGN KEY (graph_version, milestone_id)
                    REFERENCES execution_milestones(graph_version, milestone_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS execution_todos (
                graph_version INTEGER NOT NULL,
                todo_id TEXT NOT NULL,
                jobpack_id TEXT NOT NULL,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                PRIMARY KEY (graph_version, todo_id),
                FOREIGN KEY (graph_version, jobpack_id)
                    REFERENCES execution_jobpacks(graph_version, jobpack_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS execution_checklist_items (
                graph_version INTEGER NOT NULL,
                todo_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                item TEXT NOT NULL,
                checked INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (graph_version, todo_id, position),
                FOREIGN KEY (graph_version, todo_id)
                    REFERENCES execution_todos(graph_version, todo_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS execution_jobpack_dependencies (
                graph_version INTEGER NOT NULL,
                jobpack_id TEXT NOT NULL,
                depends_on_jobpack_id TEXT NOT NULL,
                PRIMARY KEY (graph_version, jobpack_id, depends_on_jobpack_id),
                FOREIGN KEY (graph_version, jobpack_id)
                    REFERENCES execution_jobpacks(graph_version, jobpack_id) ON DELETE CASCADE,
                FOREIGN KEY (graph_version, depends_on_jobpack_id)
                    REFERENCES execution_jobpacks(graph_version, jobpack_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS events (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS latest_checkpoint (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                sequence INTEGER NOT NULL,
                plan_revision INTEGER,
                milestone TEXT,
                jobpack TEXT,
                stage TEXT NOT NULL,
                jobpack_status TEXT
            );

            CREATE TABLE IF NOT EXISTS execution_lease (
                project_root TEXT PRIMARY KEY,
                owner TEXT NOT NULL,
                acquired_at INTEGER NOT NULL
            );
            "#,
        )?;
        self.migrate_plan_revision_hash_schema()?;
        self.migrate_execution_jobpack_contract_schema()?;
        Ok(())
    }

    fn migrate_execution_jobpack_contract_schema(&self) -> Result<()> {
        let mut statement = self.conn.prepare("PRAGMA table_info(execution_jobpacks)")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<std::result::Result<HashSet<_>, _>>()?;

        let missing_inputs = !columns.contains("required_inputs");
        let missing_outputs = !columns.contains("expected_outputs");
        if !missing_inputs && !missing_outputs {
            return Ok(());
        }
        drop(statement);

        let tx = self.conn.unchecked_transaction()?;
        if missing_inputs {
            tx.execute_batch(
                "ALTER TABLE execution_jobpacks ADD COLUMN required_inputs TEXT NOT NULL DEFAULT '[]';",
            )?;
        }
        if missing_outputs {
            tx.execute_batch(
                "ALTER TABLE execution_jobpacks ADD COLUMN expected_outputs TEXT NOT NULL DEFAULT '[]';",
            )?;
        }

        let affected_current: i64 = tx.query_row(
            r#"
            SELECT COUNT(*)
            FROM execution_graph
            WHERE status='CURRENT'
              AND version IN (SELECT DISTINCT graph_version FROM execution_jobpacks)
            "#,
            [],
            |row| row.get(0),
        )?;

        if affected_current > 0 {
            tx.execute(
                r#"
                UPDATE approved_plan
                SET execution_graph_version=0
                WHERE execution_graph_version IN (
                    SELECT version
                    FROM execution_graph
                    WHERE status='CURRENT'
                      AND version IN (
                          SELECT DISTINCT graph_version FROM execution_jobpacks
                      )
                )
                "#,
                [],
            )?;
            tx.execute(
                r#"
                UPDATE execution_graph
                SET status='SUPERSEDED'
                WHERE status='CURRENT'
                  AND version IN (
                      SELECT DISTINCT graph_version FROM execution_jobpacks
                  )
                "#,
                [],
            )?;

            tx.execute(
                "INSERT INTO events (kind, payload, created_at) VALUES ('LEGACY_EXECUTION_GRAPH_INVALIDATED', ?1, ?2)",
                params![
                    serde_json::json!({
                        "reason": "legacy jobpack rows lack required_inputs/expected_outputs"
                    })
                    .to_string(),
                    unix_seconds()?
                ],
            )?;
            let sequence = tx.last_insert_rowid();

            tx.execute(
                r#"
                UPDATE latest_checkpoint
                SET sequence=?1,
                    stage='plan_approved',
                    milestone=NULL,
                    jobpack=NULL,
                    jobpack_status=NULL
                WHERE id=1 AND stage='graph_registered'
                "#,
                params![sequence],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    fn migrate_plan_revision_hash_schema(&self) -> Result<()> {
        let sql: Option<String> = self
            .conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='plan_revisions'",
                [],
                |row| row.get(0),
            )
            .optional()?;

        let Some(sql) = sql else {
            return Ok(());
        };
        if !sql
            .to_ascii_uppercase()
            .contains("PLAN_HASH TEXT NOT NULL UNIQUE")
        {
            return Ok(());
        }

        self.conn.execute_batch(
            r#"
            BEGIN IMMEDIATE;
            ALTER TABLE plan_revisions RENAME TO plan_revisions_legacy;
            CREATE TABLE plan_revisions (
                revision INTEGER PRIMARY KEY,
                plan_hash TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            INSERT INTO plan_revisions (revision, plan_hash, content, created_at)
            SELECT revision, plan_hash, content, created_at
            FROM plan_revisions_legacy;
            DROP TABLE plan_revisions_legacy;
            COMMIT;
            "#,
        )?;
        Ok(())
    }

    pub fn begin_plan_workflow(&self) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO plan_workflow_state
                (id, current_revision, reviewer_attempts, cr_attempts, status)
            VALUES (1, NULL, 0, 0, 'PLANNING')
            ON CONFLICT(id) DO UPDATE SET
                current_revision=NULL,
                reviewer_attempts=0,
                cr_attempts=0,
                status='PLANNING'
            "#,
            [],
        )?;
        Ok(())
    }

    pub fn persist_plan_revision(&self, artifact: &PlanArtifact) -> Result<PlanRevision> {
        artifact.validate()?;
        let hash = artifact.hash()?;
        let content = serde_json::to_string(artifact)?;
        let tx = self.conn.unchecked_transaction()?;
        let revision: i64 = tx.query_row(
            "SELECT COALESCE(MAX(revision), 0) + 1 FROM plan_revisions",
            [],
            |row| row.get(0),
        )?;

        tx.execute(
            "INSERT INTO plan_revisions (revision, plan_hash, content, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![revision, hash, content, unix_seconds()?],
        )?;
        tx.execute(
            r#"
            INSERT INTO plan_workflow_state
                (id, current_revision, reviewer_attempts, cr_attempts, status)
            VALUES (1, ?1, 0, 0, 'REVIEWER')
            ON CONFLICT(id) DO UPDATE SET
                current_revision=excluded.current_revision,
                status='REVIEWER'
            "#,
            params![revision],
        )?;
        tx.commit()?;

        Ok(PlanRevision {
            revision,
            hash,
            artifact: artifact.clone(),
        })
    }

    pub fn current_plan_revision(&self) -> Result<Option<PlanRevision>> {
        self.conn
            .query_row(
                "SELECT revision, plan_hash, content FROM plan_revisions ORDER BY revision DESC LIMIT 1",
                [],
                |row| {
                    let revision: i64 = row.get(0)?;
                    let hash: String = row.get(1)?;
                    let content: String = row.get(2)?;
                    let artifact: PlanArtifact = serde_json::from_str(&content).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    Ok(PlanRevision {
                        revision,
                        hash,
                        artifact,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn record_plan_verdict(
        &self,
        actor: ReviewActor,
        revision: i64,
        hash: &str,
        verdict: ReviewVerdict,
        findings: &[String],
    ) -> Result<()> {
        let current = self
            .current_plan_revision()?
            .context("cannot record plan verdict without a current plan")?;
        if current.revision != revision || current.hash != hash {
            bail!(
                "stale plan verdict: target r{} {} is not current r{} {}",
                revision,
                hash,
                current.revision,
                current.hash
            );
        }

        self.conn.execute(
            r#"
            INSERT INTO plan_verdicts
                (actor, revision, plan_hash, verdict, findings, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                actor.as_str(),
                revision,
                hash,
                verdict.as_str(),
                serde_json::to_string(findings)?,
                unix_seconds()?
            ],
        )?;
        Ok(())
    }

    pub fn has_pass(&self, actor: ReviewActor, revision: i64, hash: &str) -> Result<bool> {
        Ok(self.latest_verdict(actor, revision, hash)?.as_deref() == Some("PASS"))
    }

    fn latest_verdict(
        &self,
        actor: ReviewActor,
        revision: i64,
        hash: &str,
    ) -> Result<Option<String>> {
        self.conn
            .query_row(
                r#"
                SELECT verdict FROM plan_verdicts
                WHERE actor=?1 AND revision=?2 AND plan_hash=?3
                ORDER BY id DESC LIMIT 1
                "#,
                params![actor.as_str(), revision, hash],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn workflow_state(&self) -> Result<PlanWorkflowState> {
        self.conn
            .query_row(
                r#"
                SELECT current_revision, reviewer_attempts, cr_attempts, status
                FROM plan_workflow_state WHERE id=1
                "#,
                [],
                |row| {
                    Ok(PlanWorkflowState {
                        current_revision: row.get(0)?,
                        reviewer_attempts: row.get::<_, i64>(1)? as u32,
                        cr_attempts: row.get::<_, i64>(2)? as u32,
                        status: row.get(3)?,
                    })
                },
            )
            .optional()?
            .context("plan workflow has not been started")
    }

    pub fn set_workflow_state(
        &self,
        current_revision: Option<i64>,
        reviewer_attempts: u32,
        cr_attempts: u32,
        status: &str,
    ) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO plan_workflow_state
                (id, current_revision, reviewer_attempts, cr_attempts, status)
            VALUES (1, ?1, ?2, ?3, ?4)
            ON CONFLICT(id) DO UPDATE SET
                current_revision=excluded.current_revision,
                reviewer_attempts=excluded.reviewer_attempts,
                cr_attempts=excluded.cr_attempts,
                status=excluded.status
            "#,
            params![
                current_revision,
                reviewer_attempts as i64,
                cr_attempts as i64,
                status
            ],
        )?;
        Ok(())
    }

    pub fn approve_current_plan(&self, revision: i64, hash: &str) -> Result<PlanBinding> {
        let tx = self.conn.unchecked_transaction()?;
        let current: Option<(i64, String)> = tx
            .query_row(
                "SELECT revision, plan_hash FROM plan_revisions ORDER BY revision DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((current_revision, current_hash)) = current else {
            bail!("cannot approve without a current plan");
        };
        if current_revision != revision || current_hash != hash {
            bail!("cannot approve stale plan revision/hash");
        }

        let previous_approved: Option<(i64, String)> = tx
            .query_row(
                "SELECT revision, plan_hash FROM approved_plan WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let binding_changed = previous_approved
            .as_ref()
            .map(|(old_revision, old_hash)| *old_revision != revision || old_hash != hash)
            .unwrap_or(false);

        for actor in [ReviewActor::Reviewer, ReviewActor::LocalCr] {
            let latest: Option<String> = tx
                .query_row(
                    r#"
                    SELECT verdict FROM plan_verdicts
                    WHERE actor=?1 AND revision=?2 AND plan_hash=?3
                    ORDER BY id DESC LIMIT 1
                    "#,
                    params![actor.as_str(), revision, hash],
                    |row| row.get(0),
                )
                .optional()?;
            if latest.as_deref() != Some("PASS") {
                bail!(
                    "cannot approve plan without latest {} verdict PASS",
                    actor.as_str()
                );
            }
        }

        if binding_changed {
            retire_active_jobpacks_for_current_graphs_tx(&tx)?;
            tx.execute(
                "UPDATE execution_graph SET status='SUPERSEDED' WHERE status='CURRENT'",
                [],
            )?;
        }

        tx.execute(
            r#"
            INSERT INTO approved_plan (id, revision, plan_hash, execution_graph_version)
            VALUES (1, ?1, ?2, 0)
            ON CONFLICT(id) DO UPDATE SET
                execution_graph_version=CASE
                    WHEN approved_plan.revision=excluded.revision
                     AND approved_plan.plan_hash=excluded.plan_hash
                    THEN approved_plan.execution_graph_version
                    ELSE 0
                END,
                revision=excluded.revision,
                plan_hash=excluded.plan_hash
            "#,
            params![revision, hash],
        )?;
        tx.execute(
            "INSERT INTO events (kind, payload, created_at) VALUES ('PLAN_APPROVED', ?1, ?2)",
            params![
                serde_json::json!({"revision": revision, "hash": hash}).to_string(),
                unix_seconds()?
            ],
        )?;
        let sequence = tx.last_insert_rowid();
        tx.execute(
            r#"
            INSERT INTO latest_checkpoint
                (id, sequence, plan_revision, milestone, jobpack, stage, jobpack_status)
            VALUES (1, ?1, ?2, NULL, NULL, 'plan_approved', NULL)
            ON CONFLICT(id) DO UPDATE SET
                sequence=excluded.sequence,
                plan_revision=excluded.plan_revision,
                milestone=NULL,
                jobpack=NULL,
                stage='plan_approved',
                jobpack_status=NULL
            "#,
            params![sequence, revision],
        )?;
        tx.execute(
            "UPDATE plan_workflow_state SET status='APPROVED' WHERE id=1",
            [],
        )?;
        tx.commit()?;

        Ok(PlanBinding {
            revision,
            hash: hash.to_owned(),
            execution_graph_version: self
                .plan_binding()?
                .map(|binding| binding.execution_graph_version)
                .unwrap_or(0),
        })
    }

    pub fn register_execution_graph(
        &self,
        plan_revision: i64,
        plan_hash: &str,
        graph: &ExecutionGraph,
    ) -> Result<i64> {
        graph.validate()?;
        let tx = self.conn.unchecked_transaction()?;

        let approved: Option<(i64, String)> = tx
            .query_row(
                "SELECT revision, plan_hash FROM approved_plan WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((approved_revision, approved_hash)) = approved else {
            bail!("cannot register execution graph without an approved plan");
        };
        if approved_revision != plan_revision || approved_hash != plan_hash {
            bail!("stale execution graph registration: approved plan binding changed");
        }

        let current_plan: Option<(i64, String)> = tx
            .query_row(
                "SELECT revision, plan_hash FROM plan_revisions ORDER BY revision DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if current_plan.as_ref() != Some(&(plan_revision, plan_hash.to_owned())) {
            bail!("approved plan is no longer the current plan revision");
        }

        let existing_current: Option<(i64, i64, String)> = tx
            .query_row(
                r#"
                SELECT version, plan_revision, plan_hash
                FROM execution_graph
                WHERE status='CURRENT'
                ORDER BY version DESC LIMIT 1
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((_, existing_revision, existing_hash)) = &existing_current {
            if *existing_revision == plan_revision && existing_hash == plan_hash {
                bail!("execution graph is already registered for this approved plan");
            }
        }

        let version: i64 = tx.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM execution_graph",
            [],
            |row| row.get(0),
        )?;

        retire_active_jobpacks_for_current_graphs_tx(&tx)?;
        tx.execute(
            "UPDATE execution_graph SET status='SUPERSEDED' WHERE status='CURRENT'",
            [],
        )?;
        tx.execute(
            "INSERT INTO execution_graph (version, plan_revision, plan_hash, status) VALUES (?1, ?2, ?3, 'CURRENT')",
            params![version, plan_revision, plan_hash],
        )?;

        for milestone in &graph.milestones {
            tx.execute(
                r#"
                INSERT INTO execution_milestones
                    (graph_version, milestone_id, title, position, status)
                VALUES (?1, ?2, ?3, ?4, 'LOCKED')
                "#,
                params![
                    version,
                    milestone.id,
                    milestone.title,
                    milestone.order as i64
                ],
            )?;
        }

        for pack in &graph.jobpacks {
            tx.execute(
                r#"
                INSERT INTO execution_jobpacks
                    (graph_version, jobpack_id, milestone_id, title, goal,
                     required_inputs, expected_outputs, acceptance, verification_hints, status)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'PENDING')
                "#,
                params![
                    version,
                    pack.id,
                    pack.milestone_id,
                    pack.title,
                    pack.goal,
                    serde_json::to_string(&pack.required_inputs)?,
                    serde_json::to_string(&pack.expected_outputs)?,
                    serde_json::to_string(&pack.acceptance)?,
                    serde_json::to_string(&pack.verification_hints)?
                ],
            )?;
        }

        for todo in &graph.todos {
            tx.execute(
                r#"
                INSERT INTO execution_todos
                    (graph_version, todo_id, jobpack_id, title, status)
                VALUES (?1, ?2, ?3, ?4, 'PENDING')
                "#,
                params![version, todo.id, todo.jobpack_id, todo.title],
            )?;
            for (position, item) in todo.checklist.iter().enumerate() {
                tx.execute(
                    r#"
                    INSERT INTO execution_checklist_items
                        (graph_version, todo_id, position, item, checked)
                    VALUES (?1, ?2, ?3, ?4, 0)
                    "#,
                    params![version, todo.id, position as i64 + 1, item],
                )?;
            }
        }

        for pack in &graph.jobpacks {
            for dependency in &pack.depends_on {
                tx.execute(
                    r#"
                    INSERT INTO execution_jobpack_dependencies
                        (graph_version, jobpack_id, depends_on_jobpack_id)
                    VALUES (?1, ?2, ?3)
                    "#,
                    params![version, pack.id, dependency],
                )?;
            }
        }

        let updated = tx.execute(
            r#"
            UPDATE approved_plan
            SET execution_graph_version=?1
            WHERE id=1 AND revision=?2 AND plan_hash=?3
            "#,
            params![version, plan_revision, plan_hash],
        )?;
        if updated != 1 {
            bail!("approved plan binding changed during graph registration");
        }

        tx.execute(
            "INSERT INTO events (kind, payload, created_at) VALUES ('EXECUTION_GRAPH_REGISTERED', ?1, ?2)",
            params![
                serde_json::json!({
                    "version": version,
                    "revision": plan_revision,
                    "hash": plan_hash
                })
                .to_string(),
                unix_seconds()?
            ],
        )?;
        let sequence = tx.last_insert_rowid();
        tx.execute(
            r#"
            INSERT INTO latest_checkpoint
                (id, sequence, plan_revision, milestone, jobpack, stage, jobpack_status)
            VALUES (1, ?1, ?2, NULL, NULL, 'graph_registered', NULL)
            ON CONFLICT(id) DO UPDATE SET
                sequence=excluded.sequence,
                plan_revision=excluded.plan_revision,
                milestone=NULL,
                jobpack=NULL,
                stage='graph_registered',
                jobpack_status=NULL
            "#,
            params![sequence, plan_revision],
        )?;
        tx.commit()?;
        Ok(version)
    }

    pub fn current_execution_graph_version(&self) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT version FROM execution_graph WHERE status='CURRENT' ORDER BY version DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn execution_graph_status(&self, version: i64) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT status FROM execution_graph WHERE version=?1",
                params![version],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn execution_graph_counts(&self, version: i64) -> Result<(i64, i64, i64, i64)> {
        let milestone_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM execution_milestones WHERE graph_version=?1",
            params![version],
            |row| row.get(0),
        )?;
        let jobpack_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM execution_jobpacks WHERE graph_version=?1",
            params![version],
            |row| row.get(0),
        )?;
        let todo_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM execution_todos WHERE graph_version=?1",
            params![version],
            |row| row.get(0),
        )?;
        let checklist_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM execution_checklist_items WHERE graph_version=?1",
            params![version],
            |row| row.get(0),
        )?;
        Ok((milestone_count, jobpack_count, todo_count, checklist_count))
    }

    pub fn execution_jobpack_contract(
        &self,
        version: i64,
        jobpack_id: &str,
    ) -> Result<Option<(Vec<String>, Vec<String>)>> {
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                r#"
                SELECT required_inputs, expected_outputs
                FROM execution_jobpacks
                WHERE graph_version=?1 AND jobpack_id=?2
                "#,
                params![version, jobpack_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(inputs, outputs)| {
            Ok((
                serde_json::from_str::<Vec<String>>(&inputs)?,
                serde_json::from_str::<Vec<String>>(&outputs)?,
            ))
        })
        .transpose()
    }

    pub fn has_active_jobpack(&self, version: i64) -> Result<bool> {
        let active: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM execution_jobpacks WHERE graph_version=?1 AND status='ACTIVE'",
            params![version],
            |row| row.get(0),
        )?;
        Ok(active > 0)
    }

    pub fn milestone_status(&self, version: i64, milestone_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                r#"
                SELECT status FROM execution_milestones
                WHERE graph_version=?1 AND milestone_id=?2
                "#,
                params![version, milestone_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn jobpack_status(&self, version: i64, jobpack_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                r#"
                SELECT status FROM execution_jobpacks
                WHERE graph_version=?1 AND jobpack_id=?2
                "#,
                params![version, jobpack_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn active_jobpack_count(&self, version: i64) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM execution_jobpacks WHERE graph_version=?1 AND status='ACTIVE'",
                params![version],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn project_active_jobpack_count(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM execution_jobpacks WHERE status='ACTIVE'",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn resolve_or_activate_work(
        &self,
        project_root: &Path,
        owner: &str,
    ) -> Result<Option<ActiveWorkRecord>> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        let Some((graph_version, plan_revision, plan_hash)) = current_graph_binding_tx(&tx)? else {
            tx.commit()?;
            return Ok(None);
        };

        if let Some(work) = active_work_tx(&tx, graph_version)? {
            ensure_active_checkpoint_tx(&tx, &work)?;
            tx.commit()?;
            return Ok(Some(work));
        }

        let milestone = current_milestone_tx(&tx, graph_version)?;
        match milestone {
            Some((_, _, ref status, _)) if status == "VERIFY" => {
                ensure_milestone_checkpoint_tx(
                    &tx,
                    plan_revision,
                    milestone.as_ref().map(|item| item.0.as_str()),
                    "milestone_verify",
                )?;
                tx.commit()?;
                return Ok(None);
            }
            Some((_, _, ref status, _)) if status == "ACTIVE" => {}
            Some((id, _, status, _)) => {
                bail!("invalid current milestone state {id}: {status}");
            }
            None => {
                let next: Option<(String, String, String, i64)> = tx
                    .query_row(
                        r#"
                        SELECT milestone_id, title, status, position
                        FROM execution_milestones
                        WHERE graph_version=?1 AND status!='COMPLETE'
                        ORDER BY position ASC LIMIT 1
                        "#,
                        params![graph_version],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .optional()?;
                let Some((milestone_id, _, status, position)) = next else {
                    ensure_milestone_checkpoint_tx(
                        &tx,
                        plan_revision,
                        None,
                        "execution_graph_complete",
                    )?;
                    tx.commit()?;
                    return Ok(None);
                };
                if status != "LOCKED" {
                    bail!(
                        "cannot activate milestone {milestone_id} from state {status}; expected LOCKED"
                    );
                }
                let unfinished_before: i64 = tx.query_row(
                    r#"
                    SELECT COUNT(*) FROM execution_milestones
                    WHERE graph_version=?1 AND position<?2 AND status!='COMPLETE'
                    "#,
                    params![graph_version, position],
                    |row| row.get(0),
                )?;
                if unfinished_before != 0 {
                    bail!(
                        "cannot jump to milestone {milestone_id}; earlier milestone is incomplete"
                    );
                }
                tx.execute(
                    r#"
                    UPDATE execution_milestones
                    SET status='ACTIVE'
                    WHERE graph_version=?1 AND milestone_id=?2 AND status='LOCKED'
                    "#,
                    params![graph_version, milestone_id],
                )?;
                append_event_tx(
                    &tx,
                    "MILESTONE_ACTIVE",
                    &serde_json::json!({
                        "graph_version": graph_version,
                        "milestone": milestone_id
                    }),
                )?;
            }
        }

        let work = activate_eligible_jobpack_tx(&tx, graph_version, plan_revision, &plan_hash)?;
        tx.commit()?;
        Ok(work)
    }

    pub fn complete_active_jobpack(
        &self,
        project_root: &Path,
        owner: &str,
    ) -> Result<Option<ActiveWorkRecord>> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        let Some((graph_version, plan_revision, plan_hash)) = current_graph_binding_tx(&tx)? else {
            bail!("cannot complete Job Pack without a current execution graph");
        };
        let work = active_work_tx(&tx, graph_version)?
            .context("there is no ACTIVE Job Pack to complete")?;

        let updated = tx.execute(
            r#"
            UPDATE execution_jobpacks
            SET status='DONE'
            WHERE graph_version=?1 AND jobpack_id=?2 AND status='ACTIVE'
            "#,
            params![graph_version, work.jobpack_id],
        )?;
        if updated != 1 {
            bail!("ACTIVE Job Pack changed before completion");
        }
        append_event_tx(
            &tx,
            "JOBPACK_DONE",
            &serde_json::json!({
                "graph_version": graph_version,
                "milestone": work.milestone_id,
                "jobpack": work.jobpack_id
            }),
        )?;

        let unfinished: i64 = tx.query_row(
            r#"
            SELECT COUNT(*) FROM execution_jobpacks
            WHERE graph_version=?1 AND milestone_id=?2 AND status!='DONE'
            "#,
            params![graph_version, work.milestone_id],
            |row| row.get(0),
        )?;

        if unfinished == 0 {
            tx.execute(
                r#"
                UPDATE execution_milestones
                SET status='VERIFY'
                WHERE graph_version=?1 AND milestone_id=?2 AND status='ACTIVE'
                "#,
                params![graph_version, work.milestone_id],
            )?;
            let sequence = append_event_tx(
                &tx,
                "MILESTONE_VERIFY",
                &serde_json::json!({
                    "graph_version": graph_version,
                    "milestone": work.milestone_id
                }),
            )?;
            write_checkpoint_tx(
                &tx,
                sequence,
                plan_revision,
                Some(&work.milestone_id),
                None,
                "milestone_verify",
                None,
            )?;
            tx.commit()?;
            return Ok(None);
        }

        let next = activate_eligible_jobpack_tx(&tx, graph_version, plan_revision, &plan_hash)?
            .context("unfinished Job Packs remain but none is dependency-eligible")?;
        tx.commit()?;
        Ok(Some(next))
    }

    pub fn complete_verified_milestone(
        &self,
        project_root: &Path,
        owner: &str,
        milestone_id: &str,
    ) -> Result<Option<ActiveWorkRecord>> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        let Some((graph_version, plan_revision, plan_hash)) = current_graph_binding_tx(&tx)? else {
            bail!("cannot complete Milestone without a current execution graph");
        };

        let row: Option<(String, i64)> = tx
            .query_row(
                r#"
                SELECT status, position
                FROM execution_milestones
                WHERE graph_version=?1 AND milestone_id=?2
                "#,
                params![graph_version, milestone_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((status, position)) = row else {
            bail!("unknown milestone {milestone_id}");
        };
        if status != "VERIFY" {
            bail!("milestone {milestone_id} must be VERIFY before COMPLETE; found {status}");
        }

        let active_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM execution_jobpacks WHERE graph_version=?1 AND status='ACTIVE'",
            params![graph_version],
            |row| row.get(0),
        )?;
        if active_count != 0 {
            bail!("cannot complete milestone while a Job Pack is ACTIVE");
        }
        let unfinished: i64 = tx.query_row(
            r#"
            SELECT COUNT(*) FROM execution_jobpacks
            WHERE graph_version=?1 AND milestone_id=?2 AND status!='DONE'
            "#,
            params![graph_version, milestone_id],
            |row| row.get(0),
        )?;
        if unfinished != 0 {
            bail!("cannot complete milestone while Job Packs are unfinished");
        }

        tx.execute(
            r#"
            UPDATE execution_milestones
            SET status='COMPLETE'
            WHERE graph_version=?1 AND milestone_id=?2 AND status='VERIFY'
            "#,
            params![graph_version, milestone_id],
        )?;
        let complete_sequence = append_event_tx(
            &tx,
            "MILESTONE_COMPLETE",
            &serde_json::json!({
                "graph_version": graph_version,
                "milestone": milestone_id
            }),
        )?;

        let next: Option<(String, String, i64)> = tx
            .query_row(
                r#"
                SELECT milestone_id, status, position
                FROM execution_milestones
                WHERE graph_version=?1 AND position>?2
                ORDER BY position ASC LIMIT 1
                "#,
                params![graph_version, position],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let Some((next_id, next_status, next_position)) = next else {
            write_checkpoint_tx(
                &tx,
                complete_sequence,
                plan_revision,
                Some(milestone_id),
                None,
                "milestone_complete",
                None,
            )?;
            tx.commit()?;
            return Ok(None);
        };
        if next_position != position + 1 {
            bail!("milestone ordering is not contiguous after {milestone_id}");
        }
        if next_status != "LOCKED" {
            bail!("next milestone {next_id} must be LOCKED; found {next_status}");
        }

        let earlier_incomplete: i64 = tx.query_row(
            r#"
            SELECT COUNT(*) FROM execution_milestones
            WHERE graph_version=?1 AND position<?2 AND status!='COMPLETE'
            "#,
            params![graph_version, next_position],
            |row| row.get(0),
        )?;
        if earlier_incomplete != 0 {
            bail!("cannot activate {next_id}; an earlier milestone is incomplete");
        }

        tx.execute(
            r#"
            UPDATE execution_milestones
            SET status='ACTIVE'
            WHERE graph_version=?1 AND milestone_id=?2 AND status='LOCKED'
            "#,
            params![graph_version, next_id],
        )?;
        append_event_tx(
            &tx,
            "MILESTONE_ACTIVE",
            &serde_json::json!({
                "graph_version": graph_version,
                "milestone": next_id
            }),
        )?;

        let work = activate_eligible_jobpack_tx(&tx, graph_version, plan_revision, &plan_hash)?
            .context("new ACTIVE milestone has no dependency-eligible Job Pack")?;
        tx.commit()?;
        Ok(Some(work))
    }

    #[cfg(test)]
    pub fn set_plan_binding(
        &self,
        revision: i64,
        hash: &str,
        expected_current_revision: Option<i64>,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT revision FROM approved_plan WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(expected) = expected_current_revision {
            if current != Some(expected) {
                bail!(
                    "stale plan binding update: expected current revision {}, found {:?}",
                    expected,
                    current
                );
            }
        }

        tx.execute(
            r#"
            INSERT INTO approved_plan (id, revision, plan_hash, execution_graph_version)
            VALUES (1, ?1, ?2, COALESCE((SELECT execution_graph_version FROM approved_plan WHERE id = 1), 0))
            ON CONFLICT(id) DO UPDATE SET revision=excluded.revision, plan_hash=excluded.plan_hash
            "#,
            params![revision, hash],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn plan_binding(&self) -> Result<Option<PlanBinding>> {
        let binding = self
            .conn
            .query_row(
                "SELECT revision, plan_hash, execution_graph_version FROM approved_plan WHERE id = 1",
                [],
                |row| {
                    Ok(PlanBinding {
                        revision: row.get(0)?,
                        hash: row.get(1)?,
                        execution_graph_version: row.get(2)?,
                    })
                },
            )
            .optional()?;

        let Some(binding) = binding else {
            return Ok(None);
        };
        if let Some(current) = self.current_plan_revision()? {
            if current.revision != binding.revision || current.hash != binding.hash {
                return Ok(None);
            }
        }
        Ok(Some(binding))
    }

    pub fn acquire_lease(
        &self,
        project_root: &Path,
        owner: &str,
        stale_after: Duration,
    ) -> Result<()> {
        let project_root = canonical_or_original(project_root);
        let project_root = project_root.to_string_lossy().into_owned();
        let now = unix_seconds()?;
        let tx = self.conn.unchecked_transaction()?;

        let existing: Option<(String, i64)> = tx
            .query_row(
                "SELECT owner, acquired_at FROM execution_lease WHERE project_root = ?1",
                params![project_root],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        match existing {
            None => {
                tx.execute(
                    "INSERT INTO execution_lease (project_root, owner, acquired_at) VALUES (?1, ?2, ?3)",
                    params![project_root, owner, now],
                )?;
            }
            Some((existing_owner, _)) if existing_owner == owner => {
                tx.execute(
                    "UPDATE execution_lease SET acquired_at = ?3 WHERE project_root = ?1 AND owner = ?2",
                    params![project_root, owner, now],
                )?;
            }
            Some((existing_owner, acquired_at))
                if now.saturating_sub(acquired_at) > stale_after.as_secs() as i64 =>
            {
                if owner_process_alive(&existing_owner) {
                    bail!("project execution lease is stale by time but owner {existing_owner} is still alive");
                }
                tx.execute(
                    "DELETE FROM execution_lease WHERE project_root = ?1",
                    params![project_root],
                )?;
                tx.execute(
                    "INSERT INTO execution_lease (project_root, owner, acquired_at) VALUES (?1, ?2, ?3)",
                    params![project_root, owner, now],
                )?;
            }
            Some((existing_owner, _)) => {
                bail!("project execution is already leased by {existing_owner}");
            }
        }

        tx.commit()?;
        Ok(())
    }

    pub fn release_lease(&self, project_root: &Path, owner: &str) -> Result<()> {
        let project_root = canonical_or_original(project_root);
        let project_root = project_root.to_string_lossy().into_owned();
        self.conn.execute(
            "DELETE FROM execution_lease WHERE project_root = ?1 AND owner = ?2",
            params![project_root, owner],
        )?;
        Ok(())
    }

    pub fn transition(
        &self,
        expected_sequence: i64,
        kind: &str,
        payload: &str,
        mut checkpoint: Checkpoint,
    ) -> Result<Checkpoint> {
        let tx = self.conn.unchecked_transaction()?;
        let current_sequence: Option<i64> = tx
            .query_row(
                "SELECT sequence FROM latest_checkpoint WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let actual = current_sequence.unwrap_or(0);
        if actual != expected_sequence {
            bail!(
                "stale transition: expected checkpoint sequence {}, found {}",
                expected_sequence,
                actual
            );
        }

        tx.execute(
            "INSERT INTO events (kind, payload, created_at) VALUES (?1, ?2, ?3)",
            params![kind, payload, unix_seconds()?],
        )?;
        let sequence = tx.last_insert_rowid();
        checkpoint.sequence = sequence;

        tx.execute(
            r#"
            INSERT INTO latest_checkpoint
                (id, sequence, plan_revision, milestone, jobpack, stage, jobpack_status)
            VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                sequence=excluded.sequence,
                plan_revision=excluded.plan_revision,
                milestone=excluded.milestone,
                jobpack=excluded.jobpack,
                stage=excluded.stage,
                jobpack_status=excluded.jobpack_status
            "#,
            params![
                checkpoint.sequence,
                checkpoint.plan_revision,
                checkpoint.milestone,
                checkpoint.jobpack,
                checkpoint.stage,
                checkpoint.jobpack_status
            ],
        )?;
        tx.commit()?;
        Ok(checkpoint)
    }

    pub fn latest_checkpoint(&self) -> Result<Option<Checkpoint>> {
        self.conn
            .query_row(
                r#"
                SELECT sequence, plan_revision, milestone, jobpack, stage, jobpack_status
                FROM latest_checkpoint WHERE id = 1
                "#,
                [],
                |row| {
                    Ok(Checkpoint {
                        sequence: row.get(0)?,
                        plan_revision: row.get(1)?,
                        milestone: row.get(2)?,
                        jobpack: row.get(3)?,
                        stage: row.get(4)?,
                        jobpack_status: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }
}

fn retire_active_jobpacks_for_current_graphs_tx(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        r#"
        UPDATE execution_jobpacks
        SET status='BLOCKED'
        WHERE status='ACTIVE'
          AND graph_version IN (
              SELECT version FROM execution_graph WHERE status='CURRENT'
          )
        "#,
        [],
    )?;
    Ok(())
}

fn assert_lease_owner_tx(tx: &Transaction<'_>, project_root: &Path, owner: &str) -> Result<()> {
    let project_root = canonical_or_original(project_root)
        .to_string_lossy()
        .into_owned();
    let stored: Option<String> = tx
        .query_row(
            "SELECT owner FROM execution_lease WHERE project_root=?1",
            params![project_root],
            |row| row.get(0),
        )
        .optional()?;
    match stored {
        Some(stored) if stored == owner => Ok(()),
        Some(stored) => bail!("execution lease is owned by {stored}, not {owner}"),
        None => bail!("project execution lease is not held"),
    }
}

fn current_graph_binding_tx(tx: &Transaction<'_>) -> Result<Option<(i64, i64, String)>> {
    let row: Option<(i64, i64, String, String, i64, String)> = tx
        .query_row(
            r#"
            SELECT ap.execution_graph_version, ap.revision, ap.plan_hash,
                   eg.status, pr.revision, pr.plan_hash
            FROM approved_plan ap
            JOIN execution_graph eg ON eg.version=ap.execution_graph_version
            JOIN plan_revisions pr ON pr.revision=(
                SELECT MAX(revision) FROM plan_revisions
            )
            WHERE ap.id=1 AND ap.execution_graph_version>0
            "#,
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?;

    let Some((version, revision, hash, graph_status, current_revision, current_hash)) = row else {
        return Ok(None);
    };
    if graph_status != "CURRENT" {
        bail!("approved execution graph {version} is not CURRENT");
    }
    if revision != current_revision || hash != current_hash {
        bail!("execution graph is bound to a stale approved plan");
    }
    Ok(Some((version, revision, hash)))
}

fn current_milestone_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
) -> Result<Option<(String, String, String, i64)>> {
    let count: i64 = tx.query_row(
        r#"
        SELECT COUNT(*) FROM execution_milestones
        WHERE graph_version=?1 AND status IN ('ACTIVE', 'VERIFY')
        "#,
        params![graph_version],
        |row| row.get(0),
    )?;
    if count > 1 {
        bail!("execution graph has more than one current Milestone");
    }
    tx.query_row(
        r#"
        SELECT milestone_id, title, status, position
        FROM execution_milestones
        WHERE graph_version=?1 AND status IN ('ACTIVE', 'VERIFY')
        ORDER BY position ASC LIMIT 1
        "#,
        params![graph_version],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .optional()
    .map_err(Into::into)
}

fn active_work_tx(tx: &Transaction<'_>, graph_version: i64) -> Result<Option<ActiveWorkRecord>> {
    let active_count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM execution_jobpacks WHERE graph_version=?1 AND status='ACTIVE'",
        params![graph_version],
        |row| row.get(0),
    )?;
    if active_count > 1 {
        bail!("execution graph has more than one ACTIVE Job Pack");
    }
    if active_count == 0 {
        return Ok(None);
    }

    let row: (
        i64,
        i64,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ) = tx.query_row(
        r#"
        SELECT eg.version, eg.plan_revision, eg.plan_hash,
               m.milestone_id, m.title, m.status,
               jp.jobpack_id, jp.title, jp.status, jp.goal,
               jp.required_inputs, jp.expected_outputs,
               jp.acceptance, jp.verification_hints
        FROM execution_jobpacks jp
        JOIN execution_milestones m
          ON m.graph_version=jp.graph_version AND m.milestone_id=jp.milestone_id
        JOIN execution_graph eg ON eg.version=jp.graph_version
        WHERE jp.graph_version=?1 AND jp.status='ACTIVE'
        LIMIT 1
        "#,
        params![graph_version],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
                row.get(13)?,
            ))
        },
    )?;
    if row.5 != "ACTIVE" {
        bail!("ACTIVE Job Pack {} is outside an ACTIVE Milestone", row.6);
    }

    Ok(Some(ActiveWorkRecord {
        graph_version: row.0,
        plan_revision: row.1,
        plan_hash: row.2,
        milestone_id: row.3,
        milestone_title: row.4,
        milestone_status: row.5,
        jobpack_id: row.6,
        jobpack_title: row.7,
        jobpack_status: row.8,
        goal: row.9,
        required_inputs: serde_json::from_str(&row.10)?,
        expected_outputs: serde_json::from_str(&row.11)?,
        acceptance: serde_json::from_str(&row.12)?,
        verification_hints: serde_json::from_str(&row.13)?,
    }))
}

fn activate_eligible_jobpack_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    plan_revision: i64,
    _plan_hash: &str,
) -> Result<Option<ActiveWorkRecord>> {
    if active_work_tx(tx, graph_version)?.is_some() {
        bail!("cannot activate another Job Pack while one is ACTIVE");
    }
    let Some((milestone_id, _, status, _)) = current_milestone_tx(tx, graph_version)? else {
        return Ok(None);
    };
    if status != "ACTIVE" {
        return Ok(None);
    }

    let candidate: Option<String> = tx
        .query_row(
            r#"
            SELECT jp.jobpack_id
            FROM execution_jobpacks jp
            WHERE jp.graph_version=?1
              AND jp.milestone_id=?2
              AND jp.status IN ('PENDING', 'READY')
              AND NOT EXISTS (
                  SELECT 1
                  FROM execution_jobpack_dependencies d
                  JOIN execution_jobpacks dep
                    ON dep.graph_version=d.graph_version
                   AND dep.jobpack_id=d.depends_on_jobpack_id
                  WHERE d.graph_version=jp.graph_version
                    AND d.jobpack_id=jp.jobpack_id
                    AND dep.status!='DONE'
              )
            ORDER BY jp.jobpack_id ASC
            LIMIT 1
            "#,
            params![graph_version, milestone_id],
            |row| row.get(0),
        )
        .optional()?;

    let Some(jobpack_id) = candidate else {
        return Ok(None);
    };
    let updated = tx.execute(
        r#"
        UPDATE execution_jobpacks
        SET status='ACTIVE'
        WHERE graph_version=?1 AND jobpack_id=?2
          AND status IN ('PENDING', 'READY')
        "#,
        params![graph_version, jobpack_id],
    )?;
    if updated != 1 {
        bail!("Job Pack eligibility changed before activation");
    }
    let sequence = append_event_tx(
        tx,
        "JOBPACK_ACTIVE",
        &serde_json::json!({
            "graph_version": graph_version,
            "milestone": milestone_id,
            "jobpack": jobpack_id
        }),
    )?;
    write_checkpoint_tx(
        tx,
        sequence,
        plan_revision,
        Some(&milestone_id),
        Some(&jobpack_id),
        "jobpack_active",
        Some("ACTIVE"),
    )?;
    active_work_tx(tx, graph_version)
}

fn append_event_tx(tx: &Transaction<'_>, kind: &str, payload: &serde_json::Value) -> Result<i64> {
    tx.execute(
        "INSERT INTO events (kind, payload, created_at) VALUES (?1, ?2, ?3)",
        params![kind, payload.to_string(), unix_seconds()?],
    )?;
    Ok(tx.last_insert_rowid())
}

fn write_checkpoint_tx(
    tx: &Transaction<'_>,
    sequence: i64,
    plan_revision: i64,
    milestone: Option<&str>,
    jobpack: Option<&str>,
    stage: &str,
    jobpack_status: Option<&str>,
) -> Result<()> {
    tx.execute(
        r#"
        INSERT INTO latest_checkpoint
            (id, sequence, plan_revision, milestone, jobpack, stage, jobpack_status)
        VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(id) DO UPDATE SET
            sequence=excluded.sequence,
            plan_revision=excluded.plan_revision,
            milestone=excluded.milestone,
            jobpack=excluded.jobpack,
            stage=excluded.stage,
            jobpack_status=excluded.jobpack_status
        "#,
        params![
            sequence,
            plan_revision,
            milestone,
            jobpack,
            stage,
            jobpack_status
        ],
    )?;
    Ok(())
}

fn ensure_active_checkpoint_tx(tx: &Transaction<'_>, work: &ActiveWorkRecord) -> Result<()> {
    let checkpoint: Option<(
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
    )> = tx
        .query_row(
            r#"
            SELECT sequence, plan_revision, milestone, jobpack, stage, jobpack_status
            FROM latest_checkpoint WHERE id=1
            "#,
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?;
    let max_sequence: i64 =
        tx.query_row("SELECT COALESCE(MAX(sequence),0) FROM events", [], |row| {
            row.get(0)
        })?;

    let valid = checkpoint.as_ref().is_some_and(|cp| {
        cp.0 == max_sequence
            && cp.1 == Some(work.plan_revision)
            && cp.2.as_deref() == Some(work.milestone_id.as_str())
            && cp.3.as_deref() == Some(work.jobpack_id.as_str())
            && cp.4 == "jobpack_active"
            && cp.5.as_deref() == Some("ACTIVE")
    });
    if valid {
        return Ok(());
    }

    let sequence = append_event_tx(
        tx,
        "CHECKPOINT_REPAIRED",
        &serde_json::json!({
            "graph_version": work.graph_version,
            "milestone": work.milestone_id,
            "jobpack": work.jobpack_id,
            "stage": "jobpack_active"
        }),
    )?;
    write_checkpoint_tx(
        tx,
        sequence,
        work.plan_revision,
        Some(&work.milestone_id),
        Some(&work.jobpack_id),
        "jobpack_active",
        Some("ACTIVE"),
    )
}

fn ensure_milestone_checkpoint_tx(
    tx: &Transaction<'_>,
    plan_revision: i64,
    milestone: Option<&str>,
    stage: &str,
) -> Result<()> {
    let current: Option<(
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
    )> = tx
        .query_row(
            r#"
            SELECT sequence, plan_revision, milestone, jobpack, stage, jobpack_status
            FROM latest_checkpoint WHERE id=1
            "#,
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?;
    let max_sequence: i64 =
        tx.query_row("SELECT COALESCE(MAX(sequence),0) FROM events", [], |row| {
            row.get(0)
        })?;
    let valid = current.as_ref().is_some_and(|cp| {
        cp.0 == max_sequence
            && cp.1 == Some(plan_revision)
            && cp.2.as_deref() == milestone
            && cp.3.is_none()
            && cp.4 == stage
            && cp.5.is_none()
    });
    if valid {
        return Ok(());
    }

    let sequence = append_event_tx(
        tx,
        "CHECKPOINT_REPAIRED",
        &serde_json::json!({
            "milestone": milestone,
            "stage": stage
        }),
    )?;
    write_checkpoint_tx(tx, sequence, plan_revision, milestone, None, stage, None)
}

#[cfg(unix)]
fn owner_process_alive(owner: &str) -> bool {
    let Some(pid) = owner
        .strip_prefix("pid:")
        .and_then(|value| value.parse::<i32>().ok())
    else {
        return false;
    };

    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return true;
    }

    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn owner_process_alive(_owner: &str) -> bool {
    false
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn unix_seconds() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX epoch")?
        .as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::Checkpoint;
    use tempfile::tempdir;

    fn registry() -> (tempfile::TempDir, Registry) {
        let dir = tempdir().unwrap();
        let registry = Registry::open_at(&dir.path().join("state.db")).unwrap();
        (dir, registry)
    }

    #[test]
    fn lease_is_single_owner_until_release() {
        let (dir, registry) = registry();
        registry
            .acquire_lease(dir.path(), "owner-a", Duration::from_secs(3600))
            .unwrap();
        assert!(registry
            .acquire_lease(dir.path(), "owner-b", Duration::from_secs(3600))
            .is_err());

        registry.release_lease(dir.path(), "owner-a").unwrap();
        registry
            .acquire_lease(dir.path(), "owner-b", Duration::from_secs(3600))
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn stale_lease_does_not_displace_live_owner_process() {
        let (dir, registry) = registry();
        let owner = format!("pid:{}", std::process::id());
        registry
            .acquire_lease(dir.path(), &owner, Duration::from_secs(3600))
            .unwrap();

        registry
            .conn
            .execute("UPDATE execution_lease SET acquired_at = 0", [])
            .unwrap();

        let result = registry.acquire_lease(dir.path(), "pid:999999", Duration::from_secs(0));
        assert!(result.is_err());
    }

    #[test]
    fn legacy_jobpack_contract_migration_invalidates_current_graph() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.db");
        let legacy = Connection::open(&path).unwrap();
        legacy
            .execute_batch(
                r#"
                CREATE TABLE approved_plan (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    revision INTEGER NOT NULL,
                    plan_hash TEXT NOT NULL,
                    execution_graph_version INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO approved_plan
                    (id, revision, plan_hash, execution_graph_version)
                VALUES (1, 1, 'legacy-hash', 7);

                CREATE TABLE execution_graph (
                    version INTEGER PRIMARY KEY,
                    plan_revision INTEGER NOT NULL,
                    plan_hash TEXT NOT NULL,
                    status TEXT NOT NULL
                );
                INSERT INTO execution_graph
                    (version, plan_revision, plan_hash, status)
                VALUES (7, 1, 'legacy-hash', 'CURRENT');

                CREATE TABLE execution_jobpacks (
                    graph_version INTEGER NOT NULL,
                    jobpack_id TEXT NOT NULL,
                    milestone_id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    goal TEXT NOT NULL,
                    acceptance TEXT NOT NULL,
                    verification_hints TEXT NOT NULL,
                    status TEXT NOT NULL,
                    PRIMARY KEY (graph_version, jobpack_id)
                );
                INSERT INTO execution_jobpacks
                    (graph_version, jobpack_id, milestone_id, title, goal,
                     acceptance, verification_hints, status)
                VALUES
                    (7, 'JP1', 'M1', 'Legacy pack', 'Legacy goal',
                     '["accept"]', '["cargo test"]', 'PENDING');

                CREATE TABLE events (
                    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                    kind TEXT NOT NULL,
                    payload TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );
                INSERT INTO events (sequence, kind, payload, created_at)
                VALUES (42, 'EXECUTION_GRAPH_REGISTERED', '{}', 1);

                CREATE TABLE latest_checkpoint (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    sequence INTEGER NOT NULL,
                    plan_revision INTEGER,
                    milestone TEXT,
                    jobpack TEXT,
                    stage TEXT NOT NULL,
                    jobpack_status TEXT
                );
                INSERT INTO latest_checkpoint
                    (id, sequence, plan_revision, milestone, jobpack, stage, jobpack_status)
                VALUES (1, 42, 1, NULL, NULL, 'graph_registered', NULL);
                "#,
            )
            .unwrap();
        drop(legacy);

        let registry = Registry::open_at(&path).unwrap();
        let mut statement = registry
            .conn
            .prepare("PRAGMA table_info(execution_jobpacks)")
            .unwrap();
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<std::result::Result<HashSet<_>, _>>()
            .unwrap();

        assert!(columns.contains("required_inputs"));
        assert!(columns.contains("expected_outputs"));
        assert_eq!(
            registry.execution_graph_status(7).unwrap().as_deref(),
            Some("SUPERSEDED")
        );
        assert_eq!(
            registry
                .plan_binding()
                .unwrap()
                .unwrap()
                .execution_graph_version,
            0
        );

        let checkpoint = registry.latest_checkpoint().unwrap().unwrap();
        assert_eq!(checkpoint.stage, "plan_approved");
        assert_eq!(checkpoint.sequence, 43);
        assert_eq!(checkpoint.milestone, None);
        assert_eq!(checkpoint.jobpack, None);
        assert_eq!(checkpoint.jobpack_status, None);
    }

    #[test]
    fn duplicate_plan_hashes_are_allowed_across_revisions() {
        let (_dir, registry) = registry();
        registry.begin_plan_workflow().unwrap();
        let artifact = crate::plan::PlanArtifact {
            goal: "same".into(),
            current_architecture: "current".into(),
            required_changes: vec!["change".into()],
            implementation_approach: vec!["approach".into()],
            dependencies: vec![],
            sequence: vec!["step".into()],
            risks: vec!["risk".into()],
            acceptance_direction: vec!["acceptance".into()],
        };

        let first = registry.persist_plan_revision(&artifact).unwrap();
        let second = registry.persist_plan_revision(&artifact).unwrap();
        assert_eq!(first.hash, second.hash);
        assert_eq!(second.revision, first.revision + 1);
    }

    #[test]
    fn latest_revise_invalidates_older_pass() {
        let (_dir, registry) = registry();
        registry.begin_plan_workflow().unwrap();
        let artifact = crate::plan::PlanArtifact {
            goal: "approval".into(),
            current_architecture: "current".into(),
            required_changes: vec!["change".into()],
            implementation_approach: vec!["approach".into()],
            dependencies: vec![],
            sequence: vec!["step".into()],
            risks: vec!["risk".into()],
            acceptance_direction: vec!["acceptance".into()],
        };
        let plan = registry.persist_plan_revision(&artifact).unwrap();

        registry
            .record_plan_verdict(
                ReviewActor::Reviewer,
                plan.revision,
                &plan.hash,
                ReviewVerdict::Pass,
                &[],
            )
            .unwrap();
        registry
            .record_plan_verdict(
                ReviewActor::Reviewer,
                plan.revision,
                &plan.hash,
                ReviewVerdict::Revise,
                &["new finding".into()],
            )
            .unwrap();
        registry
            .record_plan_verdict(
                ReviewActor::LocalCr,
                plan.revision,
                &plan.hash,
                ReviewVerdict::Pass,
                &[],
            )
            .unwrap();

        assert!(!registry
            .has_pass(ReviewActor::Reviewer, plan.revision, &plan.hash)
            .unwrap());
        assert!(registry
            .approve_current_plan(plan.revision, &plan.hash)
            .is_err());
    }

    #[test]
    fn stale_transition_is_rejected() {
        let (_dir, registry) = registry();
        registry.set_plan_binding(1, "hash-1", None).unwrap();

        let mut checkpoint = Checkpoint::new("coder");
        checkpoint.plan_revision = Some(1);
        let written = registry.transition(0, "start", "{}", checkpoint).unwrap();
        assert_eq!(written.sequence, 1);

        let next = Checkpoint::new("reviewer");
        assert!(registry.transition(0, "stale", "{}", next).is_err());
        assert_eq!(
            registry.latest_checkpoint().unwrap().unwrap().stage,
            "coder"
        );
    }

    #[test]
    fn plan_binding_compare_and_set_rejects_stale_revision() {
        let (_dir, registry) = registry();
        registry.set_plan_binding(1, "one", None).unwrap();
        registry.set_plan_binding(2, "two", Some(1)).unwrap();
        assert!(registry.set_plan_binding(3, "three", Some(1)).is_err());
        assert_eq!(registry.plan_binding().unwrap().unwrap().revision, 2);
    }
}
