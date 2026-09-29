use crate::{
    checkpoint::Checkpoint,
    plan::{PlanArtifact, PlanRevision},
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
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

        tx.execute(
            r#"
            INSERT INTO approved_plan (id, revision, plan_hash, execution_graph_version)
            VALUES (1, ?1, ?2, COALESCE((SELECT execution_graph_version FROM approved_plan WHERE id=1), 0))
            ON CONFLICT(id) DO UPDATE SET
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
