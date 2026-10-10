use crate::{
    checkpoint::{Checkpoint, RecoveryClassification, ResumeAction, ResumeDecision},
    execution_graph::{
        CheckpointBoundaryKind, EvidenceRequirementSpec, ExecutionGraph, PrerequisiteState,
        TestCheckpointSpec,
    },
    plan::{PlanArtifact, PlanRevision},
    tester_evidence::{
        AdapterObservationField, ApplicabilityContext, ApplicabilityDecision, EvidenceProvenance,
        ObservedValue, ReplaySafety, TesterAttemptEvidence, TesterClassification,
        TesterEvidenceOutputRecord, TesterEvidenceRef, TesterModeOutcome, TesterPrerequisiteTarget,
        TesterTargetBinding, VerificationObservationField,
    },
    tester_execution::{TesterExecutionObservation, TesterExecutionStepRequest},
    tester_workspace::TesterWorkspaceRuntime,
    tools::MutationRecord,
    verification::{CommandEvidence, VerificationEvidence, VerificationResult},
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeCrBoundaryKey {
    pub graph_version: i64,
    pub jobpack_id: String,
    pub change_set_id: String,
    pub boundary_id: String,
    pub evidence_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeCrReviewRecord {
    pub key: CodeCrBoundaryKey,
    pub verdict: String,
    pub findings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeCrTesterEvidenceRef {
    pub checkpoint_id: String,
    pub attempt_id: String,
    pub target_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeCrBoundaryWork {
    pub key: CodeCrBoundaryKey,
    pub boundary_ids: Vec<String>,
    pub verification_run_id: i64,
    pub tester_evidence: Vec<CodeCrTesterEvidenceRef>,
    pub terminal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_review: Option<CodeCrReviewRecord>,
}

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

/// Additive successor to the legacy plan_workflow_state singleton.
/// An ambiguous legacy row is never silently overwritten or treated as a
/// fresh planning session; it must be explicitly recovered by Human.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningRunState {
    pub requirement: Option<String>,
    pub stage: String,
    pub current_revision: Option<i64>,
    pub reviewer_attempts: u32,
    pub cr_attempts: u32,
    pub legacy_snapshot_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChecklistClaim {
    pub todo_id: String,
    pub position: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeChecklistState {
    pub position: i64,
    pub item: String,
    pub checked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeTodoState {
    pub todo_id: String,
    pub title: String,
    pub status: String,
    pub checklist: Vec<CodeChecklistState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeWorkflowState {
    pub graph_version: i64,
    pub jobpack_id: String,
    pub change_set_id: Option<String>,
    pub coder_attempts: u32,
    pub reviewer_attempts: u32,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedCodeCheckpoint {
    pub change_set_id: String,
    pub summary: String,
    pub completed_checklist: Vec<ChecklistClaim>,
    pub goal_recheck: Vec<String>,
    pub mutation_journal: Vec<MutationRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTesterEvidence {
    pub checkpoint_id: String,
    pub output_id: String,
    pub attempt_id: String,
    pub value: ObservedValue,
    pub evidence_refs: Vec<TesterEvidenceRef>,
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
    pub tester_evidence: Vec<ResolvedTesterEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TesterCheckpointDisposition {
    Due,
    ProductFailure,
    Blocked,
    NeedsHuman,
    IntegrationNotReady,
    SpecGap,
}

impl TesterCheckpointDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Due => "DUE",
            Self::ProductFailure => "PRODUCT_FAILURE",
            Self::Blocked => "BLOCKED",
            Self::NeedsHuman => "NEEDS_HUMAN",
            Self::IntegrationNotReady => "INTEGRATION_NOT_READY",
            Self::SpecGap => "SPEC_GAP",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TesterCheckpointWorkRecord {
    pub graph_version: i64,
    pub plan_revision: i64,
    pub checkpoint: TestCheckpointSpec,
    pub target: Option<TesterTargetBinding>,
    pub target_fingerprint: Option<String>,
    pub disposition: TesterCheckpointDisposition,
    pub reason: Option<String>,
    pub next_attempt_id: Option<String>,
    pub retest_context: Option<TesterRetestContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterRetestContext {
    pub failed_attempt_id: String,
    pub failed_target_fingerprint: String,
    pub failure_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterResumeState {
    pub graph_version: i64,
    pub checkpoint_id: String,
    pub attempt_id: String,
    pub target_fingerprint: String,
    pub execution_id: Option<String>,
    pub stage: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterExecutionStepRecord {
    pub execution_id: String,
    pub target_fingerprint: String,
    pub request: TesterExecutionStepRequest,
    pub status: String,
    pub observation: Option<TesterExecutionObservation>,
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

            CREATE TABLE IF NOT EXISTS planning_job_builder_gaps (
                revision INTEGER PRIMARY KEY,
                plan_hash TEXT NOT NULL,
                findings_json TEXT NOT NULL,
                FOREIGN KEY (revision) REFERENCES plan_revisions(revision)
            );

            CREATE TABLE IF NOT EXISTS planning_source_evidence (
                revision INTEGER PRIMARY KEY,
                plan_hash TEXT NOT NULL,
                evidence_json TEXT NOT NULL,
                FOREIGN KEY (revision) REFERENCES plan_revisions(revision)
            );

            CREATE TABLE IF NOT EXISTS planning_run_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                requirement TEXT,
                stage TEXT NOT NULL CHECK (stage IN (
                    'LEGACY_RECOVERY_REQUIRED', 'PLANNER', 'REVIEWER', 'LOCAL_CR',
                    'JOB_BUILDER', 'PAUSED', 'REGISTERED'
                )),
                current_revision INTEGER,
                reviewer_attempts INTEGER NOT NULL DEFAULT 0,
                cr_attempts INTEGER NOT NULL DEFAULT 0,
                legacy_snapshot_json TEXT
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

            CREATE TABLE IF NOT EXISTS execution_test_checkpoints (
                graph_version INTEGER NOT NULL,
                checkpoint_id TEXT NOT NULL,
                milestone_id TEXT NOT NULL,
                boundary TEXT NOT NULL,
                before_jobpack_id TEXT,
                definition_json TEXT NOT NULL,
                PRIMARY KEY (graph_version, checkpoint_id),
                FOREIGN KEY (graph_version, milestone_id)
                    REFERENCES execution_milestones(graph_version, milestone_id) ON DELETE CASCADE,
                FOREIGN KEY (graph_version, before_jobpack_id)
                    REFERENCES execution_jobpacks(graph_version, jobpack_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS execution_checkpoint_prerequisites (
                graph_version INTEGER NOT NULL,
                checkpoint_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                jobpack_id TEXT NOT NULL,
                required_state TEXT NOT NULL,
                PRIMARY KEY (graph_version, checkpoint_id, position),
                UNIQUE (graph_version, checkpoint_id, jobpack_id),
                FOREIGN KEY (graph_version, checkpoint_id)
                    REFERENCES execution_test_checkpoints(graph_version, checkpoint_id) ON DELETE CASCADE,
                FOREIGN KEY (graph_version, jobpack_id)
                    REFERENCES execution_jobpacks(graph_version, jobpack_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS execution_evidence_outputs (
                graph_version INTEGER NOT NULL,
                checkpoint_id TEXT NOT NULL,
                output_id TEXT NOT NULL,
                mode TEXT NOT NULL,
                required INTEGER NOT NULL,
                evidence_need_id TEXT,
                definition_json TEXT NOT NULL,
                PRIMARY KEY (graph_version, checkpoint_id, output_id),
                FOREIGN KEY (graph_version, checkpoint_id)
                    REFERENCES execution_test_checkpoints(graph_version, checkpoint_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS execution_evidence_requirements (
                graph_version INTEGER NOT NULL,
                consumer_jobpack_id TEXT NOT NULL,
                checkpoint_id TEXT NOT NULL,
                output_id TEXT NOT NULL,
                required INTEGER NOT NULL,
                definition_json TEXT NOT NULL,
                PRIMARY KEY (graph_version, consumer_jobpack_id, checkpoint_id, output_id),
                FOREIGN KEY (graph_version, consumer_jobpack_id)
                    REFERENCES execution_jobpacks(graph_version, jobpack_id) ON DELETE CASCADE,
                FOREIGN KEY (graph_version, checkpoint_id, output_id)
                    REFERENCES execution_evidence_outputs(graph_version, checkpoint_id, output_id)
                    ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS code_workflow_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                graph_version INTEGER NOT NULL,
                jobpack_id TEXT NOT NULL,
                change_set_id TEXT,
                coder_attempts INTEGER NOT NULL DEFAULT 0,
                reviewer_attempts INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS code_checkpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                graph_version INTEGER NOT NULL,
                jobpack_id TEXT NOT NULL,
                change_set_id TEXT NOT NULL,
                summary TEXT NOT NULL,
                checklist_claims TEXT NOT NULL,
                goal_recheck TEXT NOT NULL,
                mutation_journal TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS code_reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                graph_version INTEGER NOT NULL,
                jobpack_id TEXT NOT NULL,
                change_set_id TEXT NOT NULL,
                verdict TEXT NOT NULL,
                findings TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS code_cr_reviews (
                graph_version INTEGER NOT NULL,
                jobpack_id TEXT NOT NULL,
                change_set_id TEXT NOT NULL,
                boundary_id TEXT NOT NULL,
                evidence_fingerprint TEXT NOT NULL,
                verdict TEXT NOT NULL,
                findings TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (
                    graph_version, jobpack_id, change_set_id,
                    boundary_id, evidence_fingerprint
                )
            );

            CREATE TABLE IF NOT EXISTS verification_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                graph_version INTEGER NOT NULL,
                jobpack_id TEXT NOT NULL,
                change_set_id TEXT NOT NULL,
                result TEXT NOT NULL,
                test_surface_changed INTEGER NOT NULL,
                profile_json TEXT NOT NULL,
                commands_json TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tester_evidence_attempts (
                graph_version INTEGER NOT NULL,
                checkpoint_id TEXT NOT NULL,
                attempt_id TEXT NOT NULL,
                target_fingerprint TEXT NOT NULL,
                attempt_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (graph_version, checkpoint_id, attempt_id),
                FOREIGN KEY (graph_version, checkpoint_id)
                    REFERENCES execution_test_checkpoints(graph_version, checkpoint_id)
                    ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS tester_evidence_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                graph_version INTEGER NOT NULL,
                checkpoint_id TEXT NOT NULL,
                attempt_id TEXT NOT NULL,
                output_id TEXT NOT NULL,
                mode TEXT NOT NULL,
                provenance TEXT NOT NULL,
                record_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                UNIQUE (graph_version, checkpoint_id, attempt_id, output_id),
                FOREIGN KEY (graph_version, checkpoint_id, attempt_id)
                    REFERENCES tester_evidence_attempts(graph_version, checkpoint_id, attempt_id)
                    ON DELETE CASCADE,
                FOREIGN KEY (graph_version, checkpoint_id, output_id)
                    REFERENCES execution_evidence_outputs(graph_version, checkpoint_id, output_id)
                    ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS tester_execution_steps (
                graph_version INTEGER NOT NULL,
                checkpoint_id TEXT NOT NULL,
                attempt_id TEXT NOT NULL,
                execution_id TEXT NOT NULL,
                step_id TEXT NOT NULL,
                adapter_id TEXT NOT NULL,
                replay_safety TEXT NOT NULL,
                fence_key TEXT NOT NULL,
                target_fingerprint TEXT NOT NULL,
                request_json TEXT NOT NULL,
                status TEXT NOT NULL,
                observation_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (graph_version, checkpoint_id, attempt_id, execution_id),
                UNIQUE (graph_version, checkpoint_id, attempt_id, step_id),
                UNIQUE (graph_version, checkpoint_id, attempt_id, fence_key),
                FOREIGN KEY (graph_version, checkpoint_id)
                    REFERENCES execution_test_checkpoints(graph_version, checkpoint_id)
                    ON DELETE CASCADE
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
        self.migrate_planning_run_state()?;
        Ok(())
    }

    /// Idempotent, append-only classification of the legacy planning singleton.
    /// A fully approved row is recognized only with matching durable bindings;
    /// any unknown or incomplete state is blocked without altering its history.
    fn migrate_planning_run_state(&self) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let existing: Option<i64> = tx
            .query_row("SELECT id FROM planning_run_state WHERE id=1", [], |row| {
                row.get(0)
            })
            .optional()?;
        if existing.is_some() {
            tx.commit()?;
            return Ok(());
        }

        let legacy: Option<(Option<i64>, i64, i64, String)> = tx
            .query_row(
                "SELECT current_revision, reviewer_attempts, cr_attempts, status FROM plan_workflow_state WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        if let Some((revision, reviewer_attempts, cr_attempts, status)) = legacy {
            let safely_completed = if status == "APPROVED" {
                if let Some(revision) = revision {
                    let matches: i64 = tx.query_row(
                        "SELECT COUNT(*) FROM approved_plan ap
                         JOIN plan_revisions pr
                           ON pr.revision=ap.revision AND pr.plan_hash=ap.plan_hash
                         JOIN execution_graph eg
                           ON eg.version=ap.execution_graph_version
                          AND eg.plan_revision=ap.revision
                          AND eg.plan_hash=ap.plan_hash AND eg.status='CURRENT'
                         WHERE ap.id=1 AND ap.revision=?1
                           AND (SELECT verdict FROM plan_verdicts
                                WHERE actor='REVIEWER' AND revision=ap.revision
                                  AND plan_hash=ap.plan_hash
                                ORDER BY id DESC LIMIT 1)='PASS'
                           AND (SELECT verdict FROM plan_verdicts
                                WHERE actor='LOCAL_CR' AND revision=ap.revision
                                  AND plan_hash=ap.plan_hash
                                ORDER BY id DESC LIMIT 1)='PASS'",
                        params![revision],
                        |row| row.get(0),
                    )?;
                    matches == 1
                } else {
                    false
                }
            } else {
                status == "IDLE" && revision.is_none() && reviewer_attempts == 0 && cr_attempts == 0
            };

            if !safely_completed {
                let legacy_snapshot = serde_json::json!({
                    "status": status,
                    "current_revision": revision,
                    "reviewer_attempts": reviewer_attempts,
                    "cr_attempts": cr_attempts
                });
                tx.execute(
                    "INSERT INTO planning_run_state
                     (id, requirement, stage, current_revision, reviewer_attempts,
                      cr_attempts, legacy_snapshot_json)
                     VALUES (1, NULL, 'LEGACY_RECOVERY_REQUIRED', NULL, 0, 0, ?1)",
                    params![legacy_snapshot.to_string()],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn planning_run_state(&self) -> Result<Option<PlanningRunState>> {
        self.conn
            .query_row(
                "SELECT requirement, stage, current_revision, reviewer_attempts,
                        cr_attempts, legacy_snapshot_json
                 FROM planning_run_state WHERE id=1",
                [],
                |row| {
                    Ok(PlanningRunState {
                        requirement: row.get(0)?,
                        stage: row.get(1)?,
                        current_revision: row.get(2)?,
                        reviewer_attempts: row.get::<_, i64>(3)? as u32,
                        cr_attempts: row.get::<_, i64>(4)? as u32,
                        legacy_snapshot_json: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Start a new planning run or recover its exact durable identity without
    /// overwriting an in-flight revision or an ambiguous legacy singleton.
    /// Resuming the returned stage is the planning runner's responsibility.
    pub fn begin_or_resume_planning_run(&self, requirement: &str) -> Result<PlanningRunState> {
        if requirement.trim().is_empty() {
            bail!("planning requires an explicit nonempty requirement");
        }
        if let Some(state) = self.planning_run_state()? {
            if state.stage == "LEGACY_RECOVERY_REQUIRED" {
                bail!("LEGACY_RECOVERY_REQUIRED: affirmative Human recovery is required");
            }
            if state.stage == "REGISTERED" {
                bail!("PLANNING_ALREADY_REGISTERED: start a new reviewed planning intent");
            }
            if state.requirement.as_deref() != Some(requirement) {
                bail!(
                    "PLANNING_REQUIREMENT_CONFLICT: active run belongs to a different requirement"
                );
            }
            return Ok(state);
        }

        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO planning_run_state
             (id, requirement, stage, current_revision, reviewer_attempts, cr_attempts)
             VALUES (1, ?1, 'PLANNER', NULL, 0, 0)",
            params![requirement],
        )?;
        tx.execute(
            "INSERT INTO plan_workflow_state
             (id, current_revision, reviewer_attempts, cr_attempts, status)
             VALUES (1, NULL, 0, 0, 'PLANNING')
             ON CONFLICT(id) DO UPDATE SET
                 current_revision=NULL, reviewer_attempts=0,
                 cr_attempts=0, status='PLANNING'",
            [],
        )?;
        tx.commit()?;
        self.planning_run_state()?
            .context("new planning run state missing after commit")
    }

    /// Apply a separately obtained, affirmative Human recovery decision.
    /// Merely receiving a planning-like message must never call this method.
    /// Decline/cancel and empty requirements do not change any durable state.
    pub fn confirm_legacy_planning_recovery(
        &self,
        new_requirement: &str,
        human_confirmed: bool,
    ) -> Result<PlanningRunState> {
        if !human_confirmed {
            bail!("LEGACY_RECOVERY_REQUIRED: explicit Human confirmation is required");
        }
        if new_requirement.trim().is_empty() {
            bail!("legacy planning recovery requires an exact nonempty new requirement");
        }

        let tx = self.conn.unchecked_transaction()?;
        let legacy_snapshot: String = tx
            .query_row(
                "SELECT legacy_snapshot_json FROM planning_run_state
                 WHERE id=1 AND stage='LEGACY_RECOVERY_REQUIRED'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .context("no blocked legacy planning state to recover")?;
        tx.execute(
            "INSERT INTO events (kind, payload, created_at)
             VALUES ('LEGACY_PLANNING_ABANDONED', ?1, ?2)",
            params![
                serde_json::json!({
                    "legacy": serde_json::from_str::<serde_json::Value>(&legacy_snapshot)?,
                    "new_requirement": new_requirement
                })
                .to_string(),
                unix_seconds()?
            ],
        )?;
        let updated = tx.execute(
            "UPDATE planning_run_state
             SET requirement=?1, stage='PLANNER', current_revision=NULL,
                 reviewer_attempts=0, cr_attempts=0
             WHERE id=1 AND stage='LEGACY_RECOVERY_REQUIRED'",
            params![new_requirement],
        )?;
        if updated != 1 {
            bail!("legacy planning recovery lost its singleton state");
        }
        tx.execute(
            "INSERT INTO plan_workflow_state
             (id, current_revision, reviewer_attempts, cr_attempts, status)
             VALUES (1, NULL, 0, 0, 'PLANNING')
             ON CONFLICT(id) DO UPDATE SET
                current_revision=NULL, reviewer_attempts=0,
                cr_attempts=0, status='PLANNING'",
            [],
        )?;
        tx.commit()?;
        self.planning_run_state()?
            .context("committed planning run state was not found")
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
        if let Some(state) = self.planning_run_state()? {
            bail!(
                "PLANNING_RUN_STATE_ACTIVE: {} requires the durable planning runner; refusing to reset legacy planning history",
                state.stage
            );
        }
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
        // A completed Planner submission advances the durable cursor in the
        // same transaction as its immutable plan revision. Legacy-only test
        // databases have no durable row and retain their original behavior.
        tx.execute(
            "UPDATE planning_run_state
             SET current_revision=?1, stage='REVIEWER'
             WHERE id=1 AND stage='PLANNER' AND requirement IS NOT NULL",
            params![revision],
        )?;
        tx.commit()?;

        Ok(PlanRevision {
            revision,
            hash,
            artifact: artifact.clone(),
        })
    }

    pub fn plan_for_current_execution_graph(&self, graph_version: i64) -> Result<PlanRevision> {
        let row: Option<(i64, String, String, i64, i64, String, String, String)> = self
            .conn
            .query_row(
                r#"
                SELECT eg.plan_revision, eg.plan_hash, eg.status,
                       ap.execution_graph_version, ap.revision, ap.plan_hash,
                       pr.plan_hash, pr.content
                FROM execution_graph eg
                JOIN approved_plan ap ON ap.id=1
                JOIN plan_revisions pr ON pr.revision=eg.plan_revision
                WHERE eg.version=?1
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
                    ))
                },
            )
            .optional()?;
        let Some((
            plan_revision,
            plan_hash,
            graph_status,
            approved_graph,
            approved_revision,
            approved_hash,
            stored_hash,
            content,
        )) = row
        else {
            bail!(
                "execution graph {} is not bound to a persisted plan",
                graph_version
            );
        };

        if graph_status != "CURRENT" {
            bail!("Tester graph {} is not CURRENT", graph_version);
        }
        if approved_graph != graph_version
            || approved_revision != plan_revision
            || approved_hash != plan_hash
        {
            bail!("Tester graph is not the currently approved execution-graph binding");
        }
        if stored_hash != plan_hash {
            bail!("execution graph plan hash does not match persisted plan revision");
        }

        Ok(PlanRevision {
            revision: plan_revision,
            hash: plan_hash,
            artifact: serde_json::from_str(&content)?,
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

        let tx = self.conn.unchecked_transaction()?;
        let durable: Option<(String, Option<i64>)> = tx
            .query_row(
                "SELECT stage, current_revision FROM planning_run_state WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((stage, bound_revision)) = durable {
            let expected = match actor {
                ReviewActor::Reviewer => "REVIEWER",
                ReviewActor::LocalCr => "LOCAL_CR",
            };
            if stage != expected || bound_revision != Some(revision) {
                bail!("PLANNING_VERDICT_STAGE_CONFLICT: durable stage/revision mismatch");
            }
            let next = match (actor, verdict) {
                (ReviewActor::Reviewer, ReviewVerdict::Pass) => "LOCAL_CR",
                (ReviewActor::Reviewer, ReviewVerdict::Revise) => "PLANNER",
                (ReviewActor::LocalCr, ReviewVerdict::Pass) => "JOB_BUILDER",
                (ReviewActor::LocalCr, ReviewVerdict::Revise) => "PLANNER",
            };
            tx.execute(
                "UPDATE planning_run_state SET stage=?1 WHERE id=1 AND stage=?2 AND current_revision=?3",
                params![next, expected, revision],
            )?;
        }
        tx.execute(
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
        tx.commit()?;
        Ok(())
    }

    /// Checkpoint source evidence for a particular immutable plan revision.
    /// Never overwrite different evidence for an already checkpointed revision.
    pub fn checkpoint_planning_source_evidence(
        &self,
        revision: i64,
        hash: &str,
        evidence_json: &str,
    ) -> Result<()> {
        let current = self
            .current_plan_revision()?
            .context("PLANNING_SOURCE_EVIDENCE_NO_PLAN")?;
        if current.revision != revision || current.hash != hash {
            bail!("PLANNING_SOURCE_EVIDENCE_STALE_PLAN");
        }
        let _: Vec<serde_json::Value> = serde_json::from_str(evidence_json)?;
        self.conn.execute(
            "INSERT INTO planning_source_evidence (revision, plan_hash, evidence_json)
             VALUES (?1, ?2, ?3) ON CONFLICT(revision) DO NOTHING",
            params![revision, hash, evidence_json],
        )?;
        let stored: (String, String) = self.conn.query_row(
            "SELECT plan_hash, evidence_json FROM planning_source_evidence WHERE revision=?1",
            params![revision],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if stored.0 != hash || stored.1 != evidence_json {
            bail!("PLANNING_SOURCE_EVIDENCE_CONFLICT: immutable checkpoint mismatch");
        }
        Ok(())
    }

    pub fn planning_source_evidence(&self, revision: i64, hash: &str) -> Result<String> {
        self.conn
            .query_row(
                "SELECT evidence_json FROM planning_source_evidence
             WHERE revision=?1 AND plan_hash=?2",
                params![revision, hash],
                |row| row.get(0),
            )
            .optional()?
            .context("PLANNING_SOURCE_EVIDENCE_MISSING: cannot resume without source evidence")
    }

    /// Persist Job Builder PLAN_GAP and route back to Planner in one commit.
    pub fn record_job_builder_plan_gap(
        &self,
        revision: i64,
        hash: &str,
        findings: &[String],
    ) -> Result<()> {
        if findings.is_empty() {
            bail!("PLANNING_JOB_BUILDER_GAP_EMPTY");
        }
        let current = self
            .current_plan_revision()?
            .context("PLANNING_JOB_BUILDER_GAP_PLAN_MISSING")?;
        if current.revision != revision || current.hash != hash {
            bail!("PLANNING_JOB_BUILDER_GAP_STALE_PLAN");
        }
        if !self.has_pass(ReviewActor::Reviewer, revision, hash)?
            || !self.has_pass(ReviewActor::LocalCr, revision, hash)?
        {
            bail!("PLANNING_JOB_BUILDER_GAP_PASS_BINDING_MISSING");
        }
        let tx = self.conn.unchecked_transaction()?;
        let stage: Option<(String, Option<i64>)> = tx
            .query_row(
                "SELECT stage, current_revision FROM planning_run_state WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if stage != Some(("JOB_BUILDER".to_string(), Some(revision))) {
            bail!("PLANNING_JOB_BUILDER_GAP_STAGE_CONFLICT");
        }
        let encoded = serde_json::to_string(findings)?;
        tx.execute(
            "INSERT INTO planning_job_builder_gaps (revision, plan_hash, findings_json)
             VALUES (?1, ?2, ?3) ON CONFLICT(revision) DO NOTHING",
            params![revision, hash, encoded],
        )?;
        let saved: (String, String) = tx.query_row(
            "SELECT plan_hash, findings_json FROM planning_job_builder_gaps WHERE revision=?1",
            params![revision],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if saved.0 != hash || saved.1 != encoded {
            bail!("PLANNING_JOB_BUILDER_GAP_CONFLICT");
        }
        tx.execute(
            "UPDATE planning_run_state SET stage='PLANNER'
             WHERE id=1 AND stage='JOB_BUILDER' AND current_revision=?1",
            params![revision],
        )?;
        tx.execute(
            "UPDATE plan_workflow_state SET status='PLAN_GAP' WHERE id=1 AND current_revision=?1",
            params![revision],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Recover the exact findings from the last persisted negative planning
    /// verdict for a revision. Never synthesize findings after a restart.
    pub fn pending_planner_revision_findings(
        &self,
        revision: i64,
        hash: &str,
    ) -> Result<Vec<String>> {
        let persisted: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT verdict, findings FROM plan_verdicts
             WHERE revision=?1 AND plan_hash=?2
               AND actor IN ('REVIEWER', 'LOCAL_CR')
             ORDER BY id DESC LIMIT 1",
                params![revision, hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (verdict, findings) = persisted
            .context("PLANNING_REVISION_EVIDENCE_MISSING: no persisted review findings")?;
        let findings = if verdict == "REVISE" {
            findings
        } else if verdict == "PASS" {
            self.conn
                .query_row(
                    "SELECT findings_json FROM planning_job_builder_gaps
                 WHERE revision=?1 AND plan_hash=?2",
                    params![revision, hash],
                    |row| row.get(0),
                )
                .optional()?
                .context("PLANNING_JOB_BUILDER_GAP_EVIDENCE_MISSING")?
        } else {
            bail!("PLANNING_REVISION_EVIDENCE_CONFLICT: unsupported latest verdict");
        };
        let findings: Vec<String> = serde_json::from_str(&findings)?;
        if findings.is_empty() {
            bail!("PLANNING_REVISION_EVIDENCE_EMPTY: cannot replan without findings");
        }
        Ok(findings)
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
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
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
        let durable: Option<Option<i64>> = tx
            .query_row(
                "SELECT current_revision FROM planning_run_state WHERE id=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(bound_revision) = durable {
            if bound_revision != current_revision {
                bail!("PLANNING_CURSOR_REVISION_CONFLICT: cannot change durable revision through legacy state");
            }
            let next_stage = match status {
                "REVIEWER" => "REVIEWER",
                "LOCAL_CR" => "LOCAL_CR",
                "JOB_BUILDER" => "JOB_BUILDER",
                "PLANNER" | "PLAN_GAP" => "PLANNER",
                "PAUSED" => "PAUSED",
                _ => bail!("PLANNING_CURSOR_STAGE_UNSUPPORTED: {status}"),
            };
            tx.execute(
                "UPDATE planning_run_state
                 SET stage=?1, reviewer_attempts=?2, cr_attempts=?3
                 WHERE id=1 AND current_revision IS ?4",
                params![
                    next_stage,
                    reviewer_attempts as i64,
                    cr_attempts as i64,
                    current_revision
                ],
            )?;
        }
        tx.commit()?;
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

        let current_plan: Option<(i64, String, String)> = tx
            .query_row(
                "SELECT revision, plan_hash, content FROM plan_revisions ORDER BY revision DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((current_revision, current_hash, current_content)) = current_plan else {
            bail!("approved plan is no longer the current plan revision");
        };
        if current_revision != plan_revision || current_hash != plan_hash {
            bail!("approved plan is no longer the current plan revision");
        }
        let current_artifact: PlanArtifact = serde_json::from_str(&current_content)?;
        graph.validate_against_plan(&current_artifact)?;

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

        for checkpoint in &graph.checkpoints {
            tx.execute(
                r#"
                INSERT INTO execution_test_checkpoints
                    (graph_version, checkpoint_id, milestone_id, boundary,
                     before_jobpack_id, definition_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    version,
                    checkpoint.id,
                    checkpoint.milestone_id,
                    serde_json::to_value(checkpoint.boundary)?
                        .as_str()
                        .context("checkpoint boundary did not serialize as string")?,
                    checkpoint.before_jobpack_id,
                    serde_json::to_string(checkpoint)?
                ],
            )?;

            for (position, prerequisite) in checkpoint.prerequisites.iter().enumerate() {
                tx.execute(
                    r#"
                    INSERT INTO execution_checkpoint_prerequisites
                        (graph_version, checkpoint_id, position, jobpack_id, required_state)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    "#,
                    params![
                        version,
                        checkpoint.id,
                        position as i64 + 1,
                        prerequisite.jobpack_id,
                        serde_json::to_value(prerequisite.state)?
                            .as_str()
                            .context("prerequisite state did not serialize as string")?
                    ],
                )?;
            }

            for output in &checkpoint.evidence_outputs {
                tx.execute(
                    r#"
                    INSERT INTO execution_evidence_outputs
                        (graph_version, checkpoint_id, output_id, mode,
                         required, evidence_need_id, definition_json)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    "#,
                    params![
                        version,
                        checkpoint.id,
                        output.id,
                        serde_json::to_value(output.mode)?
                            .as_str()
                            .context("evidence mode did not serialize as string")?,
                        if output.required { 1 } else { 0 },
                        output.evidence_need_id,
                        serde_json::to_string(output)?
                    ],
                )?;
            }
        }

        for requirement in &graph.evidence_requirements {
            tx.execute(
                r#"
                INSERT INTO execution_evidence_requirements
                    (graph_version, consumer_jobpack_id, checkpoint_id,
                     output_id, required, definition_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    version,
                    requirement.consumer_jobpack_id,
                    requirement.checkpoint_id,
                    requirement.output_id,
                    if requirement.required { 1 } else { 0 },
                    serde_json::to_string(requirement)?
                ],
            )?;
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
        let durable: Option<(String, Option<i64>)> = tx
            .query_row(
                "SELECT stage, current_revision FROM planning_run_state WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((stage, revision)) = durable {
            if stage != "JOB_BUILDER" || revision != Some(plan_revision) {
                bail!("PLANNING_GRAPH_REGISTRATION_STAGE_CONFLICT");
            }
            tx.execute(
                "UPDATE planning_run_state SET stage='REGISTERED'
                 WHERE id=1 AND stage='JOB_BUILDER' AND current_revision=?1",
                params![plan_revision],
            )?;
        }
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

    pub fn execution_checkpoint_counts(&self, version: i64) -> Result<(i64, i64, i64, i64)> {
        let checkpoints = self.conn.query_row(
            "SELECT COUNT(*) FROM execution_test_checkpoints WHERE graph_version=?1",
            params![version],
            |row| row.get(0),
        )?;
        let prerequisites = self.conn.query_row(
            "SELECT COUNT(*) FROM execution_checkpoint_prerequisites WHERE graph_version=?1",
            params![version],
            |row| row.get(0),
        )?;
        let outputs = self.conn.query_row(
            "SELECT COUNT(*) FROM execution_evidence_outputs WHERE graph_version=?1",
            params![version],
            |row| row.get(0),
        )?;
        let requirements = self.conn.query_row(
            "SELECT COUNT(*) FROM execution_evidence_requirements WHERE graph_version=?1",
            params![version],
            |row| row.get(0),
        )?;
        Ok((checkpoints, prerequisites, outputs, requirements))
    }

    pub fn execution_test_checkpoints(&self, version: i64) -> Result<Vec<TestCheckpointSpec>> {
        let mut statement = self.conn.prepare(
            r#"
            SELECT definition_json
            FROM execution_test_checkpoints
            WHERE graph_version=?1
            ORDER BY checkpoint_id
            "#,
        )?;
        let rows = statement.query_map(params![version], |row| row.get::<_, String>(0))?;
        let mut checkpoints = Vec::new();
        for row in rows {
            checkpoints.push(serde_json::from_str::<TestCheckpointSpec>(&row?)?);
        }
        Ok(checkpoints)
    }

    pub fn execution_evidence_requirements(
        &self,
        version: i64,
    ) -> Result<Vec<EvidenceRequirementSpec>> {
        let mut statement = self.conn.prepare(
            r#"
            SELECT definition_json
            FROM execution_evidence_requirements
            WHERE graph_version=?1
            ORDER BY consumer_jobpack_id, checkpoint_id, output_id
            "#,
        )?;
        let rows = statement.query_map(params![version], |row| row.get::<_, String>(0))?;
        let mut requirements = Vec::new();
        for row in rows {
            requirements.push(serde_json::from_str::<EvidenceRequirementSpec>(&row?)?);
        }
        Ok(requirements)
    }

    pub fn current_tester_evidence_catalog(&self) -> Result<Vec<ResolvedTesterEvidence>> {
        let tx = self.conn.unchecked_transaction()?;
        let Some((graph_version, _, _)) = current_graph_binding_tx(&tx)? else {
            tx.commit()?;
            return Ok(vec![]);
        };
        let rows = {
            let mut statement = tx.prepare(
                r#"
                SELECT r.checkpoint_id, r.output_id, r.attempt_id, r.record_json
                FROM tester_evidence_records r
                JOIN (
                    SELECT checkpoint_id, output_id, MAX(id) AS latest_id
                    FROM tester_evidence_records
                    WHERE graph_version=?1
                    GROUP BY checkpoint_id, output_id
                ) latest ON latest.latest_id=r.id
                ORDER BY r.checkpoint_id, r.output_id
                "#,
            )?;
            let values = statement
                .query_map(params![graph_version], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            values
        };

        let mut catalog = Vec::new();
        for (checkpoint_id, output_id, attempt_id, record_json) in rows {
            let record: TesterEvidenceOutputRecord = serde_json::from_str(&record_json)?;
            if record.provenance != EvidenceProvenance::Observed {
                continue;
            }
            let Some(attempt) =
                tester_attempt_evidence_tx(&tx, graph_version, &checkpoint_id, &attempt_id)?
            else {
                continue;
            };
            if validate_tester_target_tx(&tx, graph_version, &attempt.target).is_err() {
                continue;
            }
            if !tester_output_mode_satisfied(&attempt, record.mode) {
                continue;
            }
            let context = applicability_context_for_attempt(&attempt);
            if record.applicability.evaluate(&context)? != ApplicabilityDecision::Compatible {
                continue;
            }
            let Some(value) = record.value.clone() else {
                continue;
            };
            catalog.push(ResolvedTesterEvidence {
                checkpoint_id,
                output_id,
                attempt_id,
                value,
                evidence_refs: record.evidence_refs.clone(),
            });
        }
        tx.commit()?;
        Ok(catalog)
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

    pub fn begin_code_workflow(
        &self,
        project_root: &Path,
        owner: &str,
        graph_version: i64,
        jobpack_id: &str,
    ) -> Result<CodeWorkflowState> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        let (_plan_revision, _milestone_id) =
            assert_active_jobpack_binding_tx(&tx, graph_version, jobpack_id)?;

        let preserve_reviewed_progress = {
            let state: Option<(Option<String>, String)> = tx
                .query_row(
                    r#"
                    SELECT change_set_id, status
                    FROM code_workflow_state
                    WHERE id=1 AND graph_version=?1 AND jobpack_id=?2
                    "#,
                    params![graph_version, jobpack_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((Some(change_set_id), status)) = state {
                if status == "REVIEW_PASS" {
                    if let Some(cr_work) = code_cr_boundary_work_tx(&tx, graph_version)? {
                        let exact_pass = !cr_work.terminal
                            && cr_work.key.jobpack_id == jobpack_id
                            && cr_work.key.change_set_id == change_set_id
                            && cr_work
                                .existing_review
                                .as_ref()
                                .is_some_and(|review| review.verdict == "PASS");
                        if exact_pass {
                            assert_code_change_set_current_tx(
                                &tx,
                                project_root,
                                graph_version,
                                jobpack_id,
                                &change_set_id,
                            )?;
                        }
                        exact_pass
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        };

        if !preserve_reviewed_progress {
            tx.execute(
                r#"
                UPDATE execution_checklist_items
                SET checked=0
                WHERE graph_version=?1
                  AND todo_id IN (
                      SELECT todo_id FROM execution_todos
                      WHERE graph_version=?1 AND jobpack_id=?2
                  )
                "#,
                params![graph_version, jobpack_id],
            )?;
            tx.execute(
                r#"
                UPDATE execution_todos
                SET status='PENDING'
                WHERE graph_version=?1 AND jobpack_id=?2
                "#,
                params![graph_version, jobpack_id],
            )?;
        }

        tx.execute(
            r#"
            INSERT INTO code_workflow_state
                (id, graph_version, jobpack_id, change_set_id,
                 coder_attempts, reviewer_attempts, status)
            VALUES (1, ?1, ?2, NULL, 0, 0, 'CODER')
            ON CONFLICT(id) DO UPDATE SET
                graph_version=excluded.graph_version,
                jobpack_id=excluded.jobpack_id,
                change_set_id=NULL,
                coder_attempts=0,
                reviewer_attempts=0,
                status='CODER'
            "#,
            params![graph_version, jobpack_id],
        )?;
        append_event_tx(
            &tx,
            if preserve_reviewed_progress {
                "CODE_WORKFLOW_CONTINUED_AFTER_CR"
            } else {
                "CODE_WORKFLOW_STARTED"
            },
            &serde_json::json!({
                "graph_version": graph_version,
                "jobpack": jobpack_id
            }),
        )?;
        tx.commit()?;
        Ok(CodeWorkflowState {
            graph_version,
            jobpack_id: jobpack_id.to_owned(),
            change_set_id: None,
            coder_attempts: 0,
            reviewer_attempts: 0,
            status: "CODER".into(),
        })
    }

    pub fn code_workflow_state(&self) -> Result<Option<CodeWorkflowState>> {
        self.conn
            .query_row(
                r#"
                SELECT graph_version, jobpack_id, change_set_id,
                       coder_attempts, reviewer_attempts, status
                FROM code_workflow_state WHERE id=1
                "#,
                [],
                |row| {
                    Ok(CodeWorkflowState {
                        graph_version: row.get(0)?,
                        jobpack_id: row.get(1)?,
                        change_set_id: row.get(2)?,
                        coder_attempts: row.get::<_, i64>(3)? as u32,
                        reviewer_attempts: row.get::<_, i64>(4)? as u32,
                        status: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn persisted_code_checkpoint(
        &self,
        graph_version: i64,
        jobpack_id: &str,
        change_set_id: &str,
    ) -> Result<Option<PersistedCodeCheckpoint>> {
        let raw: Option<(String, String, String, String)> = self
            .conn
            .query_row(
                r#"
                SELECT summary, checklist_claims, goal_recheck, mutation_journal
                FROM code_checkpoints
                WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
                ORDER BY id DESC LIMIT 1
                "#,
                params![graph_version, jobpack_id, change_set_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        raw.map(|(summary, checklist, goal_recheck, journal)| {
            Ok(PersistedCodeCheckpoint {
                change_set_id: change_set_id.to_owned(),
                summary,
                completed_checklist: serde_json::from_str(&checklist)?,
                goal_recheck: serde_json::from_str(&goal_recheck)?,
                mutation_journal: serde_json::from_str(&journal)?,
            })
        })
        .transpose()
    }

    pub fn latest_code_review_findings(
        &self,
        graph_version: i64,
        jobpack_id: &str,
        change_set_id: &str,
    ) -> Result<Vec<String>> {
        let findings: Option<String> = self
            .conn
            .query_row(
                r#"
                SELECT findings
                FROM code_reviews
                WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
                ORDER BY id DESC LIMIT 1
                "#,
                params![graph_version, jobpack_id, change_set_id],
                |row| row.get(0),
            )
            .optional()?;
        findings
            .map(|value| serde_json::from_str(&value).map_err(Into::into))
            .transpose()
            .map(|value| value.unwrap_or_default())
    }

    pub fn enter_code_internal_fix(
        &self,
        project_root: &Path,
        owner: &str,
        graph_version: i64,
        jobpack_id: &str,
        change_set_id: &str,
        reason: &str,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        assert_active_jobpack_binding_tx(&tx, graph_version, jobpack_id)?;
        let updated = tx.execute(
            r#"
            UPDATE code_workflow_state
            SET status='INTERNAL_FIX'
            WHERE id=1 AND graph_version=?1 AND jobpack_id=?2
              AND change_set_id=?3 AND status='REVIEW_PASS'
            "#,
            params![graph_version, jobpack_id, change_set_id],
        )?;
        if updated != 1 {
            bail!("Local CR repair requires exact REVIEW_PASS code workflow state");
        }
        append_event_tx(
            &tx,
            "CODE_INTERNAL_FIX_REQUIRED",
            &serde_json::json!({
                "graph_version": graph_version,
                "jobpack": jobpack_id,
                "change_set_id": change_set_id,
                "reason": reason
            }),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn current_active_work(&self) -> Result<Option<ActiveWorkRecord>> {
        let tx = self.conn.unchecked_transaction()?;
        let Some((graph_version, _, _)) = current_graph_binding_tx(&tx)? else {
            tx.commit()?;
            return Ok(None);
        };
        let work = active_work_tx(&tx, graph_version)?;
        tx.commit()?;
        Ok(work)
    }

    pub fn resolve_resume_decision(
        &self,
        project_root: &Path,
        available_capabilities: &[String],
    ) -> Result<ResumeDecision> {
        if let Some(planning) = self.planning_run_state()? {
            let needs_human = planning.stage == "LEGACY_RECOVERY_REQUIRED";
            // The successor planning router is implemented in the next UAR-3
            // checkpoint. Until then, NEVER resume a stale CURRENT graph while
            // a new planning run is nonterminal.
            return Ok(ResumeDecision {
                classification: if needs_human {
                    RecoveryClassification::NeedsHuman
                } else {
                    RecoveryClassification::Blocked
                },
                action: ResumeAction::BlockedNeedsHuman,
                graph_version: None,
                milestone_id: None,
                jobpack_id: None,
                change_set_id: None,
                checkpoint_id: None,
                attempt_id: None,
                reason: if needs_human {
                    "LEGACY_RECOVERY_REQUIRED: Human must affirmatively abandon old planning before starting a new requirement".into()
                } else {
                    format!(
                        "planning stage {} requires the durable planning resume router",
                        planning.stage
                    )
                },
            });
        }
        let Some((graph_version, _, _)) = ({
            let tx = self.conn.unchecked_transaction()?;
            let binding = current_graph_binding_tx(&tx)?;
            tx.commit()?;
            binding
        }) else {
            return Ok(ResumeDecision {
                classification: RecoveryClassification::DurableExact,
                action: ResumeAction::ExecutionComplete,
                graph_version: None,
                milestone_id: None,
                jobpack_id: None,
                change_set_id: None,
                checkpoint_id: None,
                attempt_id: None,
                reason: "no CURRENT execution graph".into(),
            });
        };

        let active_work = self.current_active_work()?;
        let persisted_code_state = self.code_workflow_state()?;
        if let Some(state) = persisted_code_state.as_ref() {
            if state.graph_version == graph_version
                && active_work
                    .as_ref()
                    .is_some_and(|work| work.jobpack_id == state.jobpack_id)
                && state.status != "CODER"
            {
                if let Some(change_set_id) = state.change_set_id.as_deref() {
                    let tx = self.conn.unchecked_transaction()?;
                    let (classification, reason) = reconcile_code_change_set_source_tx(
                        &tx,
                        project_root,
                        graph_version,
                        &state.jobpack_id,
                        change_set_id,
                    )?;
                    tx.commit()?;
                    if classification != RecoveryClassification::DurableExact {
                        return Ok(ResumeDecision {
                            classification,
                            action: ResumeAction::BlockedNeedsHuman,
                            graph_version: Some(graph_version),
                            milestone_id: active_work
                                .as_ref()
                                .map(|work| work.milestone_id.clone()),
                            jobpack_id: Some(state.jobpack_id.clone()),
                            change_set_id: Some(change_set_id.to_owned()),
                            checkpoint_id: None,
                            attempt_id: None,
                            reason,
                        });
                    }
                }
            }
        }

        if let Some(state) = self.latest_tester_resume_state()? {
            if state.graph_version == graph_version && state.stage != "ATTEMPT_RECORDED" {
                if state.stage == "RECOVERY_REQUIRED" {
                    return Ok(ResumeDecision {
                        classification: RecoveryClassification::NeedsHuman,
                        action: ResumeAction::BlockedNeedsHuman,
                        graph_version: Some(graph_version),
                        milestone_id: None,
                        jobpack_id: None,
                        change_set_id: None,
                        checkpoint_id: Some(state.checkpoint_id),
                        attempt_id: Some(state.attempt_id),
                        reason: "Tester has unresolved NON_IDEMPOTENT execution recovery".into(),
                    });
                }
                if state.stage == "EXECUTION_PREPARED" {
                    let steps = self.tester_execution_steps(
                        graph_version,
                        &state.checkpoint_id,
                        &state.attempt_id,
                    )?;
                    if steps.iter().any(|step| {
                        step.status == "PREPARED"
                            && step.request.replay_safety == ReplaySafety::NonIdempotent
                    }) {
                        return Ok(ResumeDecision {
                            classification: RecoveryClassification::NeedsHuman,
                            action: ResumeAction::BlockedNeedsHuman,
                            graph_version: Some(graph_version),
                            milestone_id: None,
                            jobpack_id: None,
                            change_set_id: None,
                            checkpoint_id: Some(state.checkpoint_id),
                            attempt_id: Some(state.attempt_id),
                            reason: "uncertain NON_IDEMPOTENT Tester execution cannot auto-replay"
                                .into(),
                        });
                    }
                }
                return Ok(ResumeDecision {
                    classification: RecoveryClassification::DurableExact,
                    action: ResumeAction::ResumeTesterAttempt,
                    graph_version: Some(graph_version),
                    milestone_id: None,
                    jobpack_id: None,
                    change_set_id: None,
                    checkpoint_id: Some(state.checkpoint_id),
                    attempt_id: Some(state.attempt_id),
                    reason: format!("resume persisted Tester stage {}", state.stage),
                });
            }
        }

        if let Some(checkpoint) = self.resolve_tester_checkpoint(available_capabilities)? {
            let action = match checkpoint.disposition {
                TesterCheckpointDisposition::Due => ResumeAction::ResumeTesterAttempt,
                TesterCheckpointDisposition::ProductFailure => ResumeAction::ResumeCoder,
                TesterCheckpointDisposition::NeedsHuman
                | TesterCheckpointDisposition::Blocked
                | TesterCheckpointDisposition::IntegrationNotReady
                | TesterCheckpointDisposition::SpecGap => ResumeAction::BlockedNeedsHuman,
            };
            let classification = if action == ResumeAction::BlockedNeedsHuman {
                RecoveryClassification::NeedsHuman
            } else {
                RecoveryClassification::DurableExact
            };
            return Ok(ResumeDecision {
                classification,
                action,
                graph_version: Some(graph_version),
                milestone_id: Some(checkpoint.checkpoint.milestone_id.clone()),
                jobpack_id: checkpoint
                    .target
                    .as_ref()
                    .and_then(|target| target.prerequisites.first())
                    .map(|target| target.jobpack_id.clone()),
                change_set_id: checkpoint
                    .target
                    .as_ref()
                    .and_then(|target| target.prerequisites.first())
                    .and_then(|target| target.change_set_id.clone()),
                checkpoint_id: Some(checkpoint.checkpoint.id.clone()),
                attempt_id: checkpoint.next_attempt_id.clone(),
                reason: checkpoint.reason.clone().unwrap_or_else(|| {
                    format!("Tester checkpoint {} is due", checkpoint.checkpoint.id)
                }),
            });
        }

        if let Some(state) = persisted_code_state {
            if state.graph_version == graph_version
                && active_work
                    .as_ref()
                    .is_some_and(|work| work.jobpack_id == state.jobpack_id)
            {
                let milestone_id = active_work.as_ref().map(|work| work.milestone_id.clone());
                let action = match state.status.as_str() {
                    "CODER" => ResumeAction::ResumeCoder,
                    "REVIEWER" => ResumeAction::ResumeReviewer,
                    "INTERNAL_FIX" => ResumeAction::ResumeInternalFix,
                    "PAUSED" => ResumeAction::BlockedNeedsHuman,
                    "REVIEW_PASS" => {
                        let change_set_id = state
                            .change_set_id
                            .as_deref()
                            .context("REVIEW_PASS resume requires exact change set")?;
                        if self
                            .latest_verification_result(
                                graph_version,
                                &state.jobpack_id,
                                change_set_id,
                            )?
                            .as_deref()
                            != Some("TEST_PASS")
                        {
                            ResumeAction::RunRequiredVerification
                        } else if self.resolve_code_cr_boundary()?.is_some() {
                            ResumeAction::ResumeLocalCr
                        } else {
                            ResumeAction::ResumeCoder
                        }
                    }
                    other => {
                        return Ok(ResumeDecision {
                            classification: RecoveryClassification::Blocked,
                            action: ResumeAction::BlockedNeedsHuman,
                            graph_version: Some(graph_version),
                            milestone_id,
                            jobpack_id: Some(state.jobpack_id),
                            change_set_id: state.change_set_id,
                            checkpoint_id: None,
                            attempt_id: None,
                            reason: format!("unsupported persisted code workflow state {other}"),
                        });
                    }
                };
                let classification = if action == ResumeAction::BlockedNeedsHuman {
                    RecoveryClassification::Blocked
                } else {
                    RecoveryClassification::DurableExact
                };
                return Ok(ResumeDecision {
                    classification,
                    action,
                    graph_version: Some(graph_version),
                    milestone_id,
                    jobpack_id: Some(state.jobpack_id),
                    change_set_id: state.change_set_id,
                    checkpoint_id: None,
                    attempt_id: None,
                    reason: format!("resume persisted code workflow state {}", state.status),
                });
            }
        }

        let tx = self.conn.unchecked_transaction()?;
        let milestone = current_milestone_tx(&tx, graph_version)?;
        if let Some((milestone_id, _, status, _)) = milestone {
            let action = if status == "VERIFY" {
                ResumeAction::CompleteMilestone
            } else {
                ResumeAction::ResumeCoder
            };
            tx.commit()?;
            return Ok(ResumeDecision {
                classification: RecoveryClassification::DurableExact,
                action,
                graph_version: Some(graph_version),
                milestone_id: Some(milestone_id),
                jobpack_id: None,
                change_set_id: None,
                checkpoint_id: None,
                attempt_id: None,
                reason: format!("resume milestone state {status}"),
            });
        }

        let next: Option<(String, i64)> = tx
            .query_row(
                r#"
                SELECT milestone_id, position
                FROM execution_milestones
                WHERE graph_version=?1 AND status!='COMPLETE'
                ORDER BY position ASC LIMIT 1
                "#,
                params![graph_version],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let decision = if let Some((milestone_id, position)) = next {
            let completed_before: i64 = tx.query_row(
                r#"
                SELECT COUNT(*) FROM execution_milestones
                WHERE graph_version=?1 AND position<?2 AND status='COMPLETE'
                "#,
                params![graph_version, position],
                |row| row.get(0),
            )?;
            if completed_before == 0 {
                ResumeDecision {
                    classification: RecoveryClassification::DurableExact,
                    action: ResumeAction::ActivateInitialMilestone,
                    graph_version: Some(graph_version),
                    milestone_id: Some(milestone_id),
                    jobpack_id: None,
                    change_set_id: None,
                    checkpoint_id: None,
                    attempt_id: None,
                    reason: "initial milestone is ready to start".into(),
                }
            } else {
                ResumeDecision {
                    classification: RecoveryClassification::DurableExact,
                    action: ResumeAction::WaitExplicitNextMilestoneStart,
                    graph_version: Some(graph_version),
                    milestone_id: Some(milestone_id),
                    jobpack_id: None,
                    change_set_id: None,
                    checkpoint_id: None,
                    attempt_id: None,
                    reason: "next milestone requires explicit start".into(),
                }
            }
        } else {
            ResumeDecision {
                classification: RecoveryClassification::DurableExact,
                action: ResumeAction::ExecutionComplete,
                graph_version: Some(graph_version),
                milestone_id: None,
                jobpack_id: None,
                change_set_id: None,
                checkpoint_id: None,
                attempt_id: None,
                reason: "execution graph has no remaining milestone".into(),
            }
        };
        tx.commit()?;
        Ok(decision)
    }

    pub fn jobpack_code_tasks(
        &self,
        graph_version: i64,
        jobpack_id: &str,
    ) -> Result<Vec<CodeTodoState>> {
        let mut statement = self.conn.prepare(
            r#"
            SELECT todo_id, title, status
            FROM execution_todos
            WHERE graph_version=?1 AND jobpack_id=?2
            ORDER BY todo_id ASC
            "#,
        )?;
        let todos = statement
            .query_map(params![graph_version, jobpack_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut result = Vec::with_capacity(todos.len());
        for (todo_id, title, status) in todos {
            let mut checklist_stmt = self.conn.prepare(
                r#"
                SELECT position, item, checked
                FROM execution_checklist_items
                WHERE graph_version=?1 AND todo_id=?2
                ORDER BY position ASC
                "#,
            )?;
            let checklist = checklist_stmt
                .query_map(params![graph_version, todo_id], |row| {
                    Ok(CodeChecklistState {
                        position: row.get(0)?,
                        item: row.get(1)?,
                        checked: row.get::<_, i64>(2)? != 0,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            result.push(CodeTodoState {
                todo_id,
                title,
                status,
                checklist,
            });
        }
        Ok(result)
    }

    pub fn record_code_checkpoint(
        &self,
        project_root: &Path,
        owner: &str,
        graph_version: i64,
        jobpack_id: &str,
        change_set_id: &str,
        summary: &str,
        claims: &[ChecklistClaim],
        goal_recheck: &[String],
        mutation_journal: &serde_json::Value,
        coder_attempts: u32,
        reviewer_attempts: u32,
    ) -> Result<()> {
        if change_set_id.trim().is_empty() {
            bail!("code checkpoint change_set_id cannot be empty");
        }
        let journal = mutation_journal
            .as_array()
            .context("mutation journal must be an array")?;
        if journal.is_empty() {
            bail!("code checkpoint must contain at least one runtime-observed mutation");
        }

        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        assert_active_jobpack_binding_tx(&tx, graph_version, jobpack_id)?;
        validate_checklist_claims_tx(&tx, graph_version, jobpack_id, claims)?;

        let workflow_state: Option<(i64, i64, String)> = tx
            .query_row(
                r#"
                SELECT coder_attempts, reviewer_attempts, status
                FROM code_workflow_state
                WHERE id=1 AND graph_version=?1 AND jobpack_id=?2
                "#,
                params![graph_version, jobpack_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((state_coder_attempts, state_reviewer_attempts, state_status)) = workflow_state
        else {
            bail!("code workflow state is missing for active Job Pack");
        };
        let checkpoint_transition_ok = match state_status.as_str() {
            "CODER" => {
                state_coder_attempts == 0
                    && state_reviewer_attempts == 0
                    && coder_attempts == 1
                    && reviewer_attempts == 0
            }
            "INTERNAL_FIX" => {
                coder_attempts as i64 == state_coder_attempts + 1
                    && reviewer_attempts as i64 == state_reviewer_attempts
            }
            _ => false,
        };
        if !checkpoint_transition_ok {
            bail!(
                "invalid code checkpoint transition from {} ({}/{}) to ({}/{})",
                state_status,
                state_coder_attempts,
                state_reviewer_attempts,
                coder_attempts,
                reviewer_attempts
            );
        }

        tx.execute(
            r#"
            INSERT INTO code_checkpoints
                (graph_version, jobpack_id, change_set_id, summary,
                 checklist_claims, goal_recheck, mutation_journal, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                graph_version,
                jobpack_id,
                change_set_id,
                summary,
                serde_json::to_string(claims)?,
                serde_json::to_string(goal_recheck)?,
                mutation_journal.to_string(),
                unix_seconds()?
            ],
        )?;
        tx.execute(
            r#"
            INSERT INTO code_workflow_state
                (id, graph_version, jobpack_id, change_set_id,
                 coder_attempts, reviewer_attempts, status)
            VALUES (1, ?1, ?2, ?3, ?4, ?5, 'REVIEWER')
            ON CONFLICT(id) DO UPDATE SET
                graph_version=excluded.graph_version,
                jobpack_id=excluded.jobpack_id,
                change_set_id=excluded.change_set_id,
                coder_attempts=excluded.coder_attempts,
                reviewer_attempts=excluded.reviewer_attempts,
                status='REVIEWER'
            "#,
            params![
                graph_version,
                jobpack_id,
                change_set_id,
                coder_attempts as i64,
                reviewer_attempts as i64
            ],
        )?;
        append_event_tx(
            &tx,
            "CODE_CHECKPOINT_SUBMITTED",
            &serde_json::json!({
                "graph_version": graph_version,
                "jobpack": jobpack_id,
                "change_set_id": change_set_id,
                "coder_attempts": coder_attempts
            }),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn record_code_review(
        &self,
        project_root: &Path,
        owner: &str,
        graph_version: i64,
        jobpack_id: &str,
        change_set_id: &str,
        verdict: ReviewVerdict,
        findings: &[String],
        coder_attempts: u32,
        reviewer_attempts: u32,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        let (plan_revision, milestone_id) =
            assert_active_jobpack_binding_tx(&tx, graph_version, jobpack_id)?;

        let state: Option<(String, i64, i64, String)> = tx
            .query_row(
                r#"
                SELECT change_set_id, coder_attempts, reviewer_attempts, status
                FROM code_workflow_state
                WHERE id=1 AND graph_version=?1 AND jobpack_id=?2
                "#,
                params![graph_version, jobpack_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((state_change_set, state_coder_attempts, state_reviewer_attempts, state_status)) =
            state
        else {
            bail!("code workflow state is missing for active Job Pack");
        };
        if state_change_set != change_set_id
            || state_status != "REVIEWER"
            || coder_attempts as i64 != state_coder_attempts
            || reviewer_attempts as i64 != state_reviewer_attempts + 1
        {
            bail!(
                "stale code review target/attempt: expected REVIEWER {} on change set {} after attempts {}/{}, received {}/{}",
                state_status,
                state_change_set,
                state_coder_attempts,
                state_reviewer_attempts,
                coder_attempts,
                reviewer_attempts
            );
        }

        let claims_json: Option<String> = tx
            .query_row(
                r#"
                SELECT checklist_claims
                FROM code_checkpoints
                WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
                ORDER BY id DESC LIMIT 1
                "#,
                params![graph_version, jobpack_id, change_set_id],
                |row| row.get(0),
            )
            .optional()?;
        let claims_json = claims_json.context("review target has no code checkpoint")?;
        let claims: Vec<ChecklistClaim> = serde_json::from_str(&claims_json)?;
        validate_checklist_claims_tx(&tx, graph_version, jobpack_id, &claims)?;

        tx.execute(
            r#"
            INSERT INTO code_reviews
                (graph_version, jobpack_id, change_set_id, verdict, findings, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                graph_version,
                jobpack_id,
                change_set_id,
                verdict.as_str(),
                serde_json::to_string(findings)?,
                unix_seconds()?
            ],
        )?;

        if verdict == ReviewVerdict::Revise {
            tx.execute(
                r#"
                UPDATE code_workflow_state
                SET coder_attempts=?1,
                    reviewer_attempts=?2,
                    status='INTERNAL_FIX'
                WHERE id=1
                "#,
                params![coder_attempts as i64, reviewer_attempts as i64],
            )?;
            append_event_tx(
                &tx,
                "CODE_REVIEW_CHANGES_REQUIRED",
                &serde_json::json!({
                    "graph_version": graph_version,
                    "jobpack": jobpack_id,
                    "change_set_id": change_set_id,
                    "findings": findings
                }),
            )?;
            tx.commit()?;
            return Ok(());
        }

        apply_checklist_claims_tx(&tx, graph_version, jobpack_id, &claims)?;
        tx.execute(
            r#"
            UPDATE code_workflow_state
            SET coder_attempts=?1,
                reviewer_attempts=?2,
                status='REVIEW_PASS'
            WHERE id=1
            "#,
            params![coder_attempts as i64, reviewer_attempts as i64],
        )?;
        let sequence = append_event_tx(
            &tx,
            "CODE_REVIEW_PASS",
            &serde_json::json!({
                "graph_version": graph_version,
                "jobpack": jobpack_id,
                "change_set_id": change_set_id
            }),
        )?;
        write_checkpoint_tx(
            &tx,
            sequence,
            plan_revision,
            Some(&milestone_id),
            Some(jobpack_id),
            "code_review_pass",
            Some("ACTIVE"),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn pause_code_workflow(
        &self,
        project_root: &Path,
        owner: &str,
        graph_version: i64,
        jobpack_id: &str,
        coder_attempts: u32,
        reviewer_attempts: u32,
        reason: &str,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        assert_active_jobpack_binding_tx(&tx, graph_version, jobpack_id)?;
        tx.execute(
            r#"
            UPDATE code_workflow_state
            SET coder_attempts=?1,
                reviewer_attempts=?2,
                status='PAUSED'
            WHERE id=1 AND graph_version=?3 AND jobpack_id=?4
            "#,
            params![
                coder_attempts as i64,
                reviewer_attempts as i64,
                graph_version,
                jobpack_id
            ],
        )?;
        append_event_tx(
            &tx,
            "CODE_WORKFLOW_PAUSED",
            &serde_json::json!({
                "graph_version": graph_version,
                "jobpack": jobpack_id,
                "reason": reason
            }),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn latest_code_review_verdict(
        &self,
        graph_version: i64,
        jobpack_id: &str,
        change_set_id: &str,
    ) -> Result<Option<String>> {
        self.conn
            .query_row(
                r#"
                SELECT verdict FROM code_reviews
                WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
                ORDER BY id DESC LIMIT 1
                "#,
                params![graph_version, jobpack_id, change_set_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn record_code_cr_review(
        &self,
        project_root: &Path,
        owner: &str,
        key: &CodeCrBoundaryKey,
        verdict: ReviewVerdict,
        findings: &[String],
    ) -> Result<()> {
        if key.jobpack_id.trim().is_empty()
            || key.change_set_id.trim().is_empty()
            || key.boundary_id.trim().is_empty()
            || key.evidence_fingerprint.trim().is_empty()
        {
            bail!("code CR boundary key fields must be non-empty");
        }

        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        assert_active_jobpack_binding_tx(&tx, key.graph_version, &key.jobpack_id)?;

        let workflow_state: Option<(Option<String>, String)> = tx
            .query_row(
                r#"
                SELECT change_set_id, status
                FROM code_workflow_state
                WHERE id=1 AND graph_version=?1 AND jobpack_id=?2
                "#,
                params![key.graph_version, key.jobpack_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((state_change_set, state_status)) = workflow_state else {
            bail!("code CR target has no code workflow state");
        };
        if state_status != "REVIEW_PASS"
            || state_change_set.as_deref() != Some(key.change_set_id.as_str())
        {
            bail!(
                "code CR requires exact REVIEW_PASS on change set {}; found status={} change_set={:?}",
                key.change_set_id,
                state_status,
                state_change_set
            );
        }
        assert_code_change_set_current_tx(
            &tx,
            project_root,
            key.graph_version,
            &key.jobpack_id,
            &key.change_set_id,
        )?;

        let existing: Option<(String, String)> = tx
            .query_row(
                r#"
                SELECT verdict, findings
                FROM code_cr_reviews
                WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
                  AND boundary_id=?4 AND evidence_fingerprint=?5
                "#,
                params![
                    key.graph_version,
                    key.jobpack_id,
                    key.change_set_id,
                    key.boundary_id,
                    key.evidence_fingerprint
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let findings_json = serde_json::to_string(findings)?;
        if let Some((existing_verdict, existing_findings)) = existing {
            if existing_verdict == verdict.as_str() && existing_findings == findings_json {
                tx.commit()?;
                return Ok(());
            }
            bail!("conflicting code CR result already exists for exact boundary key");
        }

        tx.execute(
            r#"
            INSERT INTO code_cr_reviews
                (graph_version, jobpack_id, change_set_id, boundary_id,
                 evidence_fingerprint, verdict, findings, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                key.graph_version,
                key.jobpack_id,
                key.change_set_id,
                key.boundary_id,
                key.evidence_fingerprint,
                verdict.as_str(),
                findings_json,
                unix_seconds()?
            ],
        )?;
        append_event_tx(
            &tx,
            if verdict == ReviewVerdict::Pass {
                "CODE_CR_PASS"
            } else {
                "CODE_CR_REVISE"
            },
            &serde_json::json!({
                "graph_version": key.graph_version,
                "jobpack": key.jobpack_id,
                "change_set_id": key.change_set_id,
                "boundary_id": key.boundary_id,
                "evidence_fingerprint": key.evidence_fingerprint,
                "findings": findings
            }),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn code_cr_review(&self, key: &CodeCrBoundaryKey) -> Result<Option<CodeCrReviewRecord>> {
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                r#"
                SELECT verdict, findings
                FROM code_cr_reviews
                WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
                  AND boundary_id=?4 AND evidence_fingerprint=?5
                "#,
                params![
                    key.graph_version,
                    key.jobpack_id,
                    key.change_set_id,
                    key.boundary_id,
                    key.evidence_fingerprint
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(verdict, findings)| {
            Ok::<CodeCrReviewRecord, anyhow::Error>(CodeCrReviewRecord {
                key: key.clone(),
                verdict,
                findings: serde_json::from_str(&findings)?,
            })
        })
        .transpose()
    }

    pub fn resolve_code_cr_boundary(&self) -> Result<Option<CodeCrBoundaryWork>> {
        let tx = self.conn.unchecked_transaction()?;
        let Some((graph_version, _, _)) = current_graph_binding_tx(&tx)? else {
            tx.commit()?;
            return Ok(None);
        };
        let work = code_cr_boundary_work_tx(&tx, graph_version)?;
        tx.commit()?;
        Ok(work)
    }

    pub fn resolve_tester_checkpoint(
        &self,
        available_capabilities: &[String],
    ) -> Result<Option<TesterCheckpointWorkRecord>> {
        let tx = self.conn.unchecked_transaction()?;
        let Some((graph_version, plan_revision, _plan_hash)) = current_graph_binding_tx(&tx)?
        else {
            tx.commit()?;
            return Ok(None);
        };

        let checkpoints = {
            let mut statement = tx.prepare(
                r#"
                SELECT c.definition_json
                FROM execution_test_checkpoints c
                JOIN execution_milestones m
                  ON m.graph_version=c.graph_version
                 AND m.milestone_id=c.milestone_id
                WHERE c.graph_version=?1
                ORDER BY m.position ASC, c.checkpoint_id ASC
                "#,
            )?;
            let rows = statement
                .query_map(params![graph_version], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        let available = available_capabilities
            .iter()
            .map(|item| item.trim().to_ascii_uppercase())
            .collect::<HashSet<_>>();

        for checkpoint_json in checkpoints {
            let checkpoint: TestCheckpointSpec = serde_json::from_str(&checkpoint_json)?;
            let target = build_tester_target_tx(&tx, graph_version, &checkpoint)?;
            let boundary =
                tester_boundary_state_tx(&tx, graph_version, &checkpoint, target.is_some())?;
            if matches!(boundary, TesterBoundaryState::NotReached) {
                continue;
            }

            let Some(target) = target else {
                let reason = match boundary {
                    TesterBoundaryState::Crossed(reason) => reason,
                    TesterBoundaryState::Reached => {
                        format!(
                            "checkpoint {} boundary is reached but prerequisites are not ready",
                            checkpoint.id
                        )
                    }
                    TesterBoundaryState::NotReached => unreachable!(),
                };
                tx.commit()?;
                return Ok(Some(TesterCheckpointWorkRecord {
                    graph_version,
                    plan_revision,
                    checkpoint,
                    target: None,
                    target_fingerprint: None,
                    disposition: TesterCheckpointDisposition::Blocked,
                    reason: Some(reason),
                    next_attempt_id: None,
                    retest_context: None,
                }));
            };

            let target_fingerprint = target.fingerprint(graph_version, &checkpoint.id)?;
            if let Some(attempt) = latest_tester_attempt_for_target_tx(
                &tx,
                graph_version,
                &checkpoint.id,
                &target_fingerprint,
            )? {
                attempt.validate_against_checkpoint(&checkpoint)?;
                validate_tester_target_tx(&tx, graph_version, &attempt.target)?;
                if tester_attempt_satisfied(&attempt) {
                    continue;
                }
                let (disposition, reason) = tester_attempt_blocking_state(&attempt);
                let retest_context =
                    tester_product_failure_context(&attempt, &target_fingerprint, &reason);
                tx.commit()?;
                return Ok(Some(TesterCheckpointWorkRecord {
                    graph_version,
                    plan_revision,
                    checkpoint,
                    target: Some(target),
                    target_fingerprint: Some(target_fingerprint),
                    disposition,
                    reason: Some(reason),
                    next_attempt_id: None,
                    retest_context,
                }));
            }

            if let TesterBoundaryState::Crossed(reason) = boundary {
                tx.commit()?;
                return Ok(Some(TesterCheckpointWorkRecord {
                    graph_version,
                    plan_revision,
                    checkpoint,
                    target: Some(target),
                    target_fingerprint: Some(target_fingerprint),
                    disposition: TesterCheckpointDisposition::Blocked,
                    reason: Some(reason),
                    next_attempt_id: None,
                    retest_context: None,
                }));
            }

            let missing = checkpoint
                .required_capabilities
                .iter()
                .filter(|required| !available.contains(&required.trim().to_ascii_uppercase()))
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                tx.commit()?;
                return Ok(Some(TesterCheckpointWorkRecord {
                    graph_version,
                    plan_revision,
                    checkpoint,
                    target: Some(target),
                    target_fingerprint: Some(target_fingerprint),
                    disposition: TesterCheckpointDisposition::IntegrationNotReady,
                    reason: Some(format!(
                        "checkpoint requires unavailable capabilities: {}",
                        missing.join(", ")
                    )),
                    next_attempt_id: None,
                    retest_context: None,
                }));
            }

            let retest_context = latest_product_failure_context_for_previous_target_tx(
                &tx,
                graph_version,
                &checkpoint.id,
                &target_fingerprint,
            )?;
            let attempt_count: i64 = tx.query_row(
                r#"
                SELECT COUNT(*) FROM tester_evidence_attempts
                WHERE graph_version=?1 AND checkpoint_id=?2
                "#,
                params![graph_version, checkpoint.id],
                |row| row.get(0),
            )?;
            let next_attempt_id = format!("attempt-{:04}", attempt_count + 1);
            ensure_tester_resume_checkpoint_tx(
                &tx,
                graph_version,
                plan_revision,
                &checkpoint,
                &next_attempt_id,
                &target_fingerprint,
            )?;
            tx.commit()?;
            return Ok(Some(TesterCheckpointWorkRecord {
                graph_version,
                plan_revision,
                checkpoint,
                target: Some(target),
                target_fingerprint: Some(target_fingerprint),
                disposition: TesterCheckpointDisposition::Due,
                reason: None,
                next_attempt_id: Some(next_attempt_id),
                retest_context,
            }));
        }

        tx.commit()?;
        Ok(None)
    }

    pub fn record_tester_attempt_evidence(
        &self,
        project_root: &Path,
        owner: &str,
        attempt: &TesterAttemptEvidence,
    ) -> Result<String> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;

        let graph_status: Option<String> = tx
            .query_row(
                "SELECT status FROM execution_graph WHERE version=?1",
                params![attempt.graph_version],
                |row| row.get(0),
            )
            .optional()?;
        if graph_status.as_deref() != Some("CURRENT") {
            bail!(
                "Tester evidence requires CURRENT execution graph {}; found {:?}",
                attempt.graph_version,
                graph_status
            );
        }

        let checkpoint_json: Option<String> = tx
            .query_row(
                r#"
                SELECT definition_json
                FROM execution_test_checkpoints
                WHERE graph_version=?1 AND checkpoint_id=?2
                "#,
                params![attempt.graph_version, attempt.checkpoint_id],
                |row| row.get(0),
            )
            .optional()?;
        let checkpoint_json = checkpoint_json.context("Tester checkpoint definition is missing")?;
        let checkpoint: TestCheckpointSpec = serde_json::from_str(&checkpoint_json)?;
        attempt.validate_against_checkpoint(&checkpoint)?;
        validate_tester_target_tx(&tx, attempt.graph_version, &attempt.target)?;

        validate_tester_evidence_refs(&tx, project_root, attempt)?;

        let target_fingerprint = attempt.target_fingerprint()?;
        tx.execute(
            r#"
            INSERT INTO tester_evidence_attempts
                (graph_version, checkpoint_id, attempt_id, target_fingerprint,
                 attempt_json, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                attempt.graph_version,
                attempt.checkpoint_id,
                attempt.attempt_id,
                target_fingerprint,
                serde_json::to_string(attempt)?,
                unix_seconds()?
            ],
        )?;

        for output in &attempt.outputs {
            tx.execute(
                r#"
                INSERT INTO tester_evidence_records
                    (graph_version, checkpoint_id, attempt_id, output_id,
                     mode, provenance, record_json, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
                params![
                    attempt.graph_version,
                    attempt.checkpoint_id,
                    attempt.attempt_id,
                    output.output_id,
                    serde_json::to_value(output.mode)?
                        .as_str()
                        .context("Tester evidence mode did not serialize as string")?,
                    serde_json::to_value(output.provenance)?
                        .as_str()
                        .context("Tester evidence provenance did not serialize as string")?,
                    serde_json::to_string(output)?,
                    unix_seconds()?
                ],
            )?;
        }

        let sequence = append_event_tx(
            &tx,
            "TESTER_EVIDENCE_RECORDED",
            &serde_json::json!({
                "graph_version": attempt.graph_version,
                "checkpoint_id": attempt.checkpoint_id,
                "attempt_id": attempt.attempt_id,
                "target_fingerprint": target_fingerprint,
                "modes": attempt.mode_results,
                "outputs": attempt.outputs.iter().map(|output| &output.output_id).collect::<Vec<_>>()
            }),
        )?;
        write_tester_resume_checkpoint_tx(
            &tx,
            sequence,
            attempt.graph_version,
            plan_revision_for_graph_tx(&tx, attempt.graph_version)?,
            &checkpoint.milestone_id,
            &attempt.checkpoint_id,
            &attempt.attempt_id,
            &target_fingerprint,
            None,
            "ATTEMPT_RECORDED",
        )?;
        tx.commit()?;
        Ok(target_fingerprint)
    }

    pub fn tester_attempt_evidence(
        &self,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
    ) -> Result<Option<TesterAttemptEvidence>> {
        let json: Option<String> = self
            .conn
            .query_row(
                r#"
                SELECT attempt_json
                FROM tester_evidence_attempts
                WHERE graph_version=?1 AND checkpoint_id=?2 AND attempt_id=?3
                "#,
                params![graph_version, checkpoint_id, attempt_id],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|value| serde_json::from_str(&value).map_err(Into::into))
            .transpose()
    }

    pub fn tester_target_fingerprint(
        &self,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
    ) -> Result<Option<String>> {
        self.conn
            .query_row(
                r#"
                SELECT target_fingerprint
                FROM tester_evidence_attempts
                WHERE graph_version=?1 AND checkpoint_id=?2 AND attempt_id=?3
                "#,
                params![graph_version, checkpoint_id, attempt_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn tester_evidence_output(
        &self,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
        output_id: &str,
    ) -> Result<Option<TesterEvidenceOutputRecord>> {
        let json: Option<String> = self
            .conn
            .query_row(
                r#"
                SELECT record_json
                FROM tester_evidence_records
                WHERE graph_version=?1 AND checkpoint_id=?2
                  AND attempt_id=?3 AND output_id=?4
                "#,
                params![graph_version, checkpoint_id, attempt_id, output_id],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|value| serde_json::from_str(&value).map_err(Into::into))
            .transpose()
    }

    pub fn latest_tester_evidence_output(
        &self,
        graph_version: i64,
        checkpoint_id: &str,
        output_id: &str,
    ) -> Result<Option<(String, TesterEvidenceOutputRecord)>> {
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                r#"
                SELECT attempt_id, record_json
                FROM tester_evidence_records
                WHERE graph_version=?1 AND checkpoint_id=?2 AND output_id=?3
                ORDER BY id DESC LIMIT 1
                "#,
                params![graph_version, checkpoint_id, output_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(attempt_id, json)| {
            Ok((
                attempt_id,
                serde_json::from_str::<TesterEvidenceOutputRecord>(&json)?,
            ))
        })
        .transpose()
    }

    pub fn prepare_tester_execution_step(
        &self,
        project_root: &Path,
        owner: &str,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
        target: &TesterTargetBinding,
        execution_id: &str,
        step_id: &str,
        adapter_id: &str,
        replay_safety: ReplaySafety,
        fence_key: &str,
        request_json: &serde_json::Value,
    ) -> Result<()> {
        for (name, value) in [
            ("checkpoint_id", checkpoint_id),
            ("attempt_id", attempt_id),
            ("execution_id", execution_id),
            ("step_id", step_id),
            ("adapter_id", adapter_id),
            ("fence_key", fence_key),
        ] {
            if value.trim().is_empty() {
                bail!("Tester execution {name} must not be empty");
            }
        }
        if execution_id != fence_key {
            bail!("Tester execution_id must equal the deterministic fence_key");
        }

        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        let graph_status: Option<String> = tx
            .query_row(
                "SELECT status FROM execution_graph WHERE version=?1",
                params![graph_version],
                |row| row.get(0),
            )
            .optional()?;
        if graph_status.as_deref() != Some("CURRENT") {
            bail!("Tester execution requires CURRENT execution graph");
        }

        let checkpoint_json: String = tx
            .query_row(
                r#"
                SELECT definition_json
                FROM execution_test_checkpoints
                WHERE graph_version=?1 AND checkpoint_id=?2
                "#,
                params![graph_version, checkpoint_id],
                |row| row.get(0),
            )
            .optional()?
            .context("Tester execution checkpoint definition is missing")?;
        let checkpoint: TestCheckpointSpec = serde_json::from_str(&checkpoint_json)?;
        target.validate_against_checkpoint(&checkpoint)?;
        validate_tester_target_tx(&tx, graph_version, target)?;
        let target_fingerprint = target.fingerprint(graph_version, checkpoint_id)?;
        let parsed_request: TesterExecutionStepRequest =
            serde_json::from_value(request_json.clone())
                .context("invalid persisted Tester execution request")?;
        let expected_fence = parsed_request.fence_key(
            graph_version,
            checkpoint_id,
            attempt_id,
            &target_fingerprint,
        )?;
        if parsed_request.step_id != step_id
            || parsed_request.adapter.adapter_id() != adapter_id
            || parsed_request.replay_safety != replay_safety
            || expected_fence != fence_key
            || execution_id != expected_fence
        {
            bail!("Tester execution request metadata does not match deterministic fence");
        }

        tx.execute(
            r#"
            INSERT INTO tester_execution_steps
                (graph_version, checkpoint_id, attempt_id, execution_id,
                 step_id, adapter_id, replay_safety, fence_key,
                 target_fingerprint, request_json, status,
                 observation_json, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                    'PREPARED', NULL, ?11, ?11)
            "#,
            params![
                graph_version,
                checkpoint_id,
                attempt_id,
                execution_id,
                step_id,
                adapter_id,
                replay_safety.as_str(),
                fence_key,
                target_fingerprint,
                request_json.to_string(),
                unix_seconds()?
            ],
        )
        .context("Tester execution step/fence already exists or is invalid")?;

        let sequence = append_event_tx(
            &tx,
            "TESTER_EXECUTION_PREPARED",
            &serde_json::json!({
                "graph_version": graph_version,
                "checkpoint_id": checkpoint_id,
                "attempt_id": attempt_id,
                "execution_id": execution_id,
                "step_id": step_id,
                "adapter_id": adapter_id,
                "replay_safety": replay_safety.as_str(),
                "fence_key": fence_key,
                "target_fingerprint": target_fingerprint
            }),
        )?;
        let (plan_revision, milestone_id) =
            tester_checkpoint_context_tx(&tx, graph_version, checkpoint_id)?;
        write_tester_resume_checkpoint_tx(
            &tx,
            sequence,
            graph_version,
            plan_revision,
            &milestone_id,
            checkpoint_id,
            attempt_id,
            &target_fingerprint,
            Some(execution_id),
            "EXECUTION_PREPARED",
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn complete_tester_execution_step(
        &self,
        project_root: &Path,
        owner: &str,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
        observation: &TesterExecutionObservation,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;

        let row: Option<(String, String, String, String, String)> = tx
            .query_row(
                r#"
                SELECT step_id, adapter_id, replay_safety, fence_key, status
                FROM tester_execution_steps
                WHERE graph_version=?1 AND checkpoint_id=?2
                  AND attempt_id=?3 AND execution_id=?4
                "#,
                params![
                    graph_version,
                    checkpoint_id,
                    attempt_id,
                    observation.execution_id
                ],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((step_id, adapter_id, replay_safety, fence_key, status)) = row else {
            bail!("Tester execution step is not PREPARED");
        };
        if status != "PREPARED"
            || step_id != observation.step_id
            || adapter_id != observation.adapter_id
            || replay_safety != observation.replay_safety.as_str()
            || fence_key != observation.fence_key
            || observation.execution_id != observation.fence_key
        {
            bail!("Tester execution completion does not match the PREPARED fence");
        }

        let updated = tx.execute(
            r#"
            UPDATE tester_execution_steps
            SET status=?5, observation_json=?6, updated_at=?7
            WHERE graph_version=?1 AND checkpoint_id=?2
              AND attempt_id=?3 AND execution_id=?4 AND status='PREPARED'
            "#,
            params![
                graph_version,
                checkpoint_id,
                attempt_id,
                observation.execution_id,
                observation.status.as_str(),
                serde_json::to_string(observation)?,
                unix_seconds()?
            ],
        )?;
        if updated != 1 {
            bail!("Tester execution step changed before completion");
        }

        let sequence = append_event_tx(
            &tx,
            "TESTER_EXECUTION_COMPLETED",
            &serde_json::json!({
                "graph_version": graph_version,
                "checkpoint_id": checkpoint_id,
                "attempt_id": attempt_id,
                "execution_id": observation.execution_id,
                "step_id": observation.step_id,
                "adapter_id": observation.adapter_id,
                "replay_safety": observation.replay_safety.as_str(),
                "status": observation.status.as_str()
            }),
        )?;
        let target_fingerprint: String = tx.query_row(
            r#"
            SELECT target_fingerprint
            FROM tester_execution_steps
            WHERE graph_version=?1 AND checkpoint_id=?2
              AND attempt_id=?3 AND execution_id=?4
            "#,
            params![
                graph_version,
                checkpoint_id,
                attempt_id,
                observation.execution_id
            ],
            |row| row.get(0),
        )?;
        let (plan_revision, milestone_id) =
            tester_checkpoint_context_tx(&tx, graph_version, checkpoint_id)?;
        write_tester_resume_checkpoint_tx(
            &tx,
            sequence,
            graph_version,
            plan_revision,
            &milestone_id,
            checkpoint_id,
            attempt_id,
            &target_fingerprint,
            Some(&observation.execution_id),
            "EXECUTION_COMPLETED",
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn tester_execution_observation(
        &self,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
        execution_id: &str,
    ) -> Result<Option<(String, TesterExecutionObservation)>> {
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                r#"
                SELECT target_fingerprint, observation_json
                FROM tester_execution_steps
                WHERE graph_version=?1 AND checkpoint_id=?2
                  AND attempt_id=?3 AND execution_id=?4
                  AND status IN ('COMPLETED','FAILED','BLOCKED')
                "#,
                params![graph_version, checkpoint_id, attempt_id, execution_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(target_fingerprint, json)| {
            Ok((
                target_fingerprint,
                serde_json::from_str::<TesterExecutionObservation>(&json)?,
            ))
        })
        .transpose()
    }

    pub fn tester_execution_steps(
        &self,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
    ) -> Result<Vec<TesterExecutionStepRecord>> {
        let mut statement = self.conn.prepare(
            r#"
            SELECT execution_id, step_id, adapter_id, replay_safety, fence_key,
                   target_fingerprint, request_json, status, observation_json
            FROM tester_execution_steps
            WHERE graph_version=?1 AND checkpoint_id=?2 AND attempt_id=?3
            ORDER BY created_at ASC, step_id ASC
            "#,
        )?;
        let rows =
            statement.query_map(params![graph_version, checkpoint_id, attempt_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            })?;

        let mut records = Vec::new();
        for row in rows {
            let (
                execution_id,
                step_id,
                adapter_id,
                replay_safety,
                fence_key,
                target_fingerprint,
                request_json,
                status,
                observation_json,
            ) = row?;
            let request: TesterExecutionStepRequest = serde_json::from_str(&request_json)
                .context("invalid persisted Tester execution request")?;
            if request.step_id != step_id
                || request.adapter.adapter_id() != adapter_id
                || request.replay_safety.as_str() != replay_safety
                || execution_id != fence_key
            {
                bail!("persisted Tester execution metadata is inconsistent");
            }
            let expected_fence = request.fence_key(
                graph_version,
                checkpoint_id,
                attempt_id,
                &target_fingerprint,
            )?;
            if expected_fence != fence_key {
                bail!("persisted Tester execution fence is stale");
            }
            let observation = observation_json
                .map(|json| {
                    serde_json::from_str::<TesterExecutionObservation>(&json)
                        .context("invalid persisted Tester execution observation")
                })
                .transpose()?;
            records.push(TesterExecutionStepRecord {
                execution_id,
                target_fingerprint,
                request,
                status,
                observation,
            });
        }
        Ok(records)
    }

    pub fn mark_tester_recovery_required(
        &self,
        project_root: &Path,
        owner: &str,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
        execution_id: &str,
        reason: &str,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        let row: Option<(String, String)> = tx
            .query_row(
                r#"
                SELECT replay_safety, target_fingerprint
                FROM tester_execution_steps
                WHERE graph_version=?1 AND checkpoint_id=?2
                  AND attempt_id=?3 AND execution_id=?4 AND status='PREPARED'
                "#,
                params![graph_version, checkpoint_id, attempt_id, execution_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((replay_safety, target_fingerprint)) = row else {
            bail!("Tester recovery target is not an unresolved PREPARED execution");
        };
        if replay_safety != ReplaySafety::NonIdempotent.as_str() {
            bail!("Tester recovery-required state is reserved for NON_IDEMPOTENT execution");
        }
        let sequence = append_event_tx(
            &tx,
            "TESTER_RECOVERY_REQUIRED",
            &serde_json::json!({
                "graph_version": graph_version,
                "checkpoint_id": checkpoint_id,
                "attempt_id": attempt_id,
                "execution_id": execution_id,
                "reason": reason
            }),
        )?;
        let (plan_revision, milestone_id) =
            tester_checkpoint_context_tx(&tx, graph_version, checkpoint_id)?;
        write_tester_resume_checkpoint_tx(
            &tx,
            sequence,
            graph_version,
            plan_revision,
            &milestone_id,
            checkpoint_id,
            attempt_id,
            &target_fingerprint,
            Some(execution_id),
            "RECOVERY_REQUIRED",
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn has_unresolved_tester_execution_steps(
        &self,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
    ) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            r#"
            SELECT COUNT(*)
            FROM tester_execution_steps
            WHERE graph_version=?1 AND checkpoint_id=?2
              AND attempt_id=?3 AND status='PREPARED'
            "#,
            params![graph_version, checkpoint_id, attempt_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn record_verification_evidence(
        &self,
        project_root: &Path,
        owner: &str,
        graph_version: i64,
        jobpack_id: &str,
        change_set_id: &str,
        evidence: &VerificationEvidence,
    ) -> Result<VerificationResult> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        assert_active_jobpack_binding_tx(&tx, graph_version, jobpack_id)?;

        let workflow_state: Option<(Option<String>, String)> = tx
            .query_row(
                r#"
                SELECT change_set_id, status
                FROM code_workflow_state
                WHERE id=1 AND graph_version=?1 AND jobpack_id=?2
                "#,
                params![graph_version, jobpack_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((state_change_set, state_status)) = workflow_state else {
            bail!("verification target has no code workflow state");
        };
        if state_status != "REVIEW_PASS" || state_change_set.as_deref() != Some(change_set_id) {
            bail!(
                "verification requires exact REVIEW_PASS on change set {}; found status={} change_set={:?}",
                change_set_id,
                state_status,
                state_change_set
            );
        }

        let latest_review: Option<String> = tx
            .query_row(
                r#"
                SELECT verdict FROM code_reviews
                WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
                ORDER BY id DESC LIMIT 1
                "#,
                params![graph_version, jobpack_id, change_set_id],
                |row| row.get(0),
            )
            .optional()?;
        if latest_review.as_deref() != Some("PASS") {
            bail!("verification requires exact latest code Reviewer PASS");
        }

        let result = evidence.derived_result();
        tx.execute(
            r#"
            INSERT INTO verification_runs
                (graph_version, jobpack_id, change_set_id, result,
                 test_surface_changed, profile_json, commands_json, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                graph_version,
                jobpack_id,
                change_set_id,
                result.as_str(),
                i64::from(evidence.test_surface_changed),
                serde_json::to_string(&evidence.profile)?,
                serde_json::to_string(&evidence.commands)?,
                unix_seconds()?
            ],
        )?;
        append_event_tx(
            &tx,
            "VERIFICATION_RESULT",
            &serde_json::json!({
                "graph_version": graph_version,
                "jobpack": jobpack_id,
                "change_set_id": change_set_id,
                "result": result.as_str(),
                "test_surface_changed": evidence.test_surface_changed
            }),
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn latest_verification_result(
        &self,
        graph_version: i64,
        jobpack_id: &str,
        change_set_id: &str,
    ) -> Result<Option<String>> {
        self.conn
            .query_row(
                r#"
                SELECT result FROM verification_runs
                WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
                ORDER BY id DESC LIMIT 1
                "#,
                params![graph_version, jobpack_id, change_set_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn latest_verification_run_id(
        &self,
        graph_version: i64,
        jobpack_id: &str,
        change_set_id: &str,
    ) -> Result<Option<i64>> {
        self.conn
            .query_row(
                r#"
                SELECT id FROM verification_runs
                WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
                ORDER BY id DESC LIMIT 1
                "#,
                params![graph_version, jobpack_id, change_set_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn verification_run_evidence(&self, run_id: i64) -> Result<Option<VerificationEvidence>> {
        let row: Option<(String, String, i64)> = self
            .conn
            .query_row(
                r#"
                SELECT profile_json, commands_json, test_surface_changed
                FROM verification_runs
                WHERE id=?1
                "#,
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        row.map(|(profile_json, commands_json, test_surface_changed)| {
            Ok(VerificationEvidence {
                profile: serde_json::from_str(&profile_json)?,
                commands: serde_json::from_str(&commands_json)?,
                test_surface_changed: test_surface_changed != 0,
            })
        })
        .transpose()
    }

    pub fn checklist_checked(
        &self,
        graph_version: i64,
        todo_id: &str,
        position: i64,
    ) -> Result<Option<bool>> {
        self.conn
            .query_row(
                r#"
                SELECT checked FROM execution_checklist_items
                WHERE graph_version=?1 AND todo_id=?2 AND position=?3
                "#,
                params![graph_version, todo_id, position],
                |row| Ok(row.get::<_, i64>(0)? != 0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn todo_status(&self, graph_version: i64, todo_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                r#"
                SELECT status FROM execution_todos
                WHERE graph_version=?1 AND todo_id=?2
                "#,
                params![graph_version, todo_id],
                |row| row.get(0),
            )
            .optional()
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
                let completed_before: i64 = tx.query_row(
                    r#"
                    SELECT COUNT(*) FROM execution_milestones
                    WHERE graph_version=?1 AND position<?2 AND status='COMPLETE'
                    "#,
                    params![graph_version, position],
                    |row| row.get(0),
                )?;
                if completed_before != 0 {
                    ensure_milestone_checkpoint_tx(
                        &tx,
                        plan_revision,
                        Some(&milestone_id),
                        "milestone_ready_explicit_start",
                    )?;
                    tx.commit()?;
                    return Ok(None);
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
                        "milestone": milestone_id,
                        "activation_policy": "INITIAL_START"
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
        let workflow_state = tx
            .query_row(
                r#"
                SELECT change_set_id, status
                FROM code_workflow_state
                WHERE id=1 AND graph_version=?1 AND jobpack_id=?2
                "#,
                params![graph_version, work.jobpack_id],
                |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .context("Job Pack terminal requires code workflow state")?;
        let change_set_id = workflow_state
            .0
            .context("Job Pack terminal requires exact change set")?;
        if workflow_state.1 != "REVIEW_PASS" {
            bail!("Job Pack terminal requires exact REVIEW_PASS");
        }
        assert_code_change_set_current_tx(
            &tx,
            project_root,
            graph_version,
            &work.jobpack_id,
            &change_set_id,
        )?;

        let cr_work = code_cr_boundary_work_tx(&tx, graph_version)?
            .context("Job Pack is not mature for terminal Local CR")?;
        if !cr_work.terminal
            || !cr_work
                .boundary_ids
                .iter()
                .any(|boundary| boundary == "JOBPACK_TERMINAL")
        {
            bail!("Job Pack terminal gates are not fully satisfied");
        }
        let cr_pass = cr_work
            .existing_review
            .as_ref()
            .is_some_and(|review| review.verdict == "PASS");
        if !cr_pass {
            bail!("Job Pack terminal requires exact Local CR PASS");
        }

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
        let Some((graph_version, plan_revision, _plan_hash)) = current_graph_binding_tx(&tx)?
        else {
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

        let milestone_checkpoints = {
            let mut statement = tx.prepare(
                r#"
                SELECT definition_json
                FROM execution_test_checkpoints
                WHERE graph_version=?1 AND milestone_id=?2
                ORDER BY checkpoint_id
                "#,
            )?;
            let rows = statement
                .query_map(params![graph_version, milestone_id], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        for checkpoint_json in milestone_checkpoints {
            let checkpoint: TestCheckpointSpec = serde_json::from_str(&checkpoint_json)?;
            if checkpoint.boundary != CheckpointBoundaryKind::MilestoneGate {
                continue;
            }

            let target =
                build_tester_target_tx(&tx, graph_version, &checkpoint)?.with_context(|| {
                    format!(
                        "milestone checkpoint {} prerequisites are not ready",
                        checkpoint.id
                    )
                })?;
            match tester_boundary_state_tx(&tx, graph_version, &checkpoint, true)? {
                TesterBoundaryState::Reached => {}
                TesterBoundaryState::NotReached => {
                    bail!(
                        "milestone checkpoint {} boundary is not reached",
                        checkpoint.id
                    );
                }
                TesterBoundaryState::Crossed(reason) => {
                    bail!(
                        "milestone checkpoint {} is not satisfiable: {}",
                        checkpoint.id,
                        reason
                    );
                }
            }

            let target_fingerprint = target.fingerprint(graph_version, &checkpoint.id)?;
            let attempt = latest_tester_attempt_for_target_tx(
                &tx,
                graph_version,
                &checkpoint.id,
                &target_fingerprint,
            )?
            .with_context(|| {
                format!(
                    "milestone checkpoint {} requires a satisfied Tester attempt",
                    checkpoint.id
                )
            })?;
            attempt.validate_against_checkpoint(&checkpoint)?;
            validate_tester_target_tx(&tx, graph_version, &attempt.target)?;
            if !tester_attempt_satisfied(&attempt) {
                bail!(
                    "milestone checkpoint {} latest exact-target attempt is not satisfied",
                    checkpoint.id
                );
            }
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

        let checkpoint_sequence = if let Some((next_id, next_status, next_position)) = next {
            if next_position != position + 1 {
                bail!("milestone ordering is not contiguous after {milestone_id}");
            }
            if next_status != "LOCKED" {
                bail!("next milestone {next_id} must be LOCKED; found {next_status}");
            }
            append_event_tx(
                &tx,
                "MILESTONE_READY_FOR_EXPLICIT_START",
                &serde_json::json!({
                    "graph_version": graph_version,
                    "milestone": next_id
                }),
            )?
        } else {
            complete_sequence
        };

        write_checkpoint_tx(
            &tx,
            checkpoint_sequence,
            plan_revision,
            Some(milestone_id),
            None,
            "milestone_complete",
            None,
        )?;
        tx.commit()?;
        Ok(None)
    }

    pub fn activate_next_milestone(
        &self,
        project_root: &Path,
        owner: &str,
    ) -> Result<Option<ActiveWorkRecord>> {
        let tx = self.conn.unchecked_transaction()?;
        assert_lease_owner_tx(&tx, project_root, owner)?;
        let Some((graph_version, plan_revision, plan_hash)) = current_graph_binding_tx(&tx)? else {
            bail!("cannot activate Milestone without a current execution graph");
        };
        if let Some(work) = active_work_tx(&tx, graph_version)? {
            tx.commit()?;
            return Ok(Some(work));
        }
        if current_milestone_tx(&tx, graph_version)?.is_some() {
            bail!("cannot explicitly activate the next Milestone while one is already ACTIVE or VERIFY");
        }

        let next: Option<(String, String, i64)> = tx
            .query_row(
                r#"
                SELECT milestone_id, status, position
                FROM execution_milestones
                WHERE graph_version=?1 AND status!='COMPLETE'
                ORDER BY position ASC LIMIT 1
                "#,
                params![graph_version],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((milestone_id, status, position)) = next else {
            ensure_milestone_checkpoint_tx(&tx, plan_revision, None, "execution_graph_complete")?;
            tx.commit()?;
            return Ok(None);
        };
        if status != "LOCKED" {
            bail!("cannot activate milestone {milestone_id} from state {status}; expected LOCKED");
        }

        let earlier_incomplete: i64 = tx.query_row(
            r#"
            SELECT COUNT(*) FROM execution_milestones
            WHERE graph_version=?1 AND position<?2 AND status!='COMPLETE'
            "#,
            params![graph_version, position],
            |row| row.get(0),
        )?;
        if earlier_incomplete != 0 {
            bail!("cannot activate {milestone_id}; an earlier milestone is incomplete");
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
                "milestone": milestone_id,
                "activation_policy": "EXPLICIT_START"
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
            Some((existing_owner, _)) if pid_owner_known_dead(&existing_owner) => {
                tx.execute(
                    "DELETE FROM execution_lease WHERE project_root = ?1",
                    params![project_root],
                )?;
                tx.execute(
                    "INSERT INTO execution_lease (project_root, owner, acquired_at) VALUES (?1, ?2, ?3)",
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

    pub fn latest_tester_resume_state(&self) -> Result<Option<TesterResumeState>> {
        let Some(checkpoint) = self.latest_checkpoint()? else {
            return Ok(None);
        };
        decode_tester_resume_state(&checkpoint.stage)
    }
}

const TESTER_RESUME_PREFIX: &str = "tester_resume:";

fn encode_tester_resume_state(state: &TesterResumeState) -> Result<String> {
    Ok(format!(
        "{}{}",
        TESTER_RESUME_PREFIX,
        serde_json::to_string(state)?
    ))
}

fn decode_tester_resume_state(stage: &str) -> Result<Option<TesterResumeState>> {
    let Some(payload) = stage.strip_prefix(TESTER_RESUME_PREFIX) else {
        return Ok(None);
    };
    serde_json::from_str(payload)
        .context("invalid Tester resume state in latest_checkpoint")
        .map(Some)
}

fn plan_revision_for_graph_tx(tx: &Transaction<'_>, graph_version: i64) -> Result<i64> {
    tx.query_row(
        "SELECT plan_revision FROM execution_graph WHERE version=?1 AND status='CURRENT'",
        params![graph_version],
        |row| row.get(0),
    )
    .optional()?
    .context("Tester resume state requires CURRENT execution graph")
}

fn tester_checkpoint_context_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    checkpoint_id: &str,
) -> Result<(i64, String)> {
    tx.query_row(
        r#"
        SELECT eg.plan_revision, c.milestone_id
        FROM execution_graph eg
        JOIN execution_test_checkpoints c ON c.graph_version=eg.version
        WHERE eg.version=?1 AND eg.status='CURRENT' AND c.checkpoint_id=?2
        "#,
        params![graph_version, checkpoint_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()?
    .context("Tester resume checkpoint context is missing or stale")
}

fn write_tester_resume_checkpoint_tx(
    tx: &Transaction<'_>,
    sequence: i64,
    graph_version: i64,
    plan_revision: i64,
    milestone_id: &str,
    checkpoint_id: &str,
    attempt_id: &str,
    target_fingerprint: &str,
    execution_id: Option<&str>,
    stage: &str,
) -> Result<()> {
    let encoded = encode_tester_resume_state(&TesterResumeState {
        graph_version,
        checkpoint_id: checkpoint_id.to_owned(),
        attempt_id: attempt_id.to_owned(),
        target_fingerprint: target_fingerprint.to_owned(),
        execution_id: execution_id.map(str::to_owned),
        stage: stage.to_owned(),
    })?;
    write_checkpoint_tx(
        tx,
        sequence,
        plan_revision,
        Some(milestone_id),
        None,
        &encoded,
        None,
    )
}

fn ensure_tester_resume_checkpoint_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    plan_revision: i64,
    checkpoint: &TestCheckpointSpec,
    attempt_id: &str,
    target_fingerprint: &str,
) -> Result<()> {
    let current: Option<(i64, String)> = tx
        .query_row(
            "SELECT sequence, stage FROM latest_checkpoint WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let max_sequence: i64 =
        tx.query_row("SELECT COALESCE(MAX(sequence),0) FROM events", [], |row| {
            row.get(0)
        })?;

    if let Some((sequence, stage)) = current {
        if sequence == max_sequence {
            if let Some(state) = decode_tester_resume_state(&stage)? {
                if state.graph_version == graph_version
                    && state.checkpoint_id == checkpoint.id
                    && state.attempt_id == attempt_id
                    && state.target_fingerprint == target_fingerprint
                    && state.stage != "ATTEMPT_RECORDED"
                {
                    return Ok(());
                }
            }
        }
    }

    let sequence = append_event_tx(
        tx,
        "TESTER_CHECKPOINT_DUE",
        &serde_json::json!({
            "graph_version": graph_version,
            "checkpoint_id": checkpoint.id,
            "attempt_id": attempt_id,
            "target_fingerprint": target_fingerprint
        }),
    )?;
    write_tester_resume_checkpoint_tx(
        tx,
        sequence,
        graph_version,
        plan_revision,
        &checkpoint.milestone_id,
        &checkpoint.id,
        attempt_id,
        target_fingerprint,
        None,
        "CHECKPOINT_DUE",
    )
}

fn validate_tester_target_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    target_binding: &TesterTargetBinding,
) -> Result<()> {
    let checkpoint_states = target_binding
        .prerequisites
        .iter()
        .map(
            |target| crate::execution_graph::CheckpointPrerequisiteSpec {
                jobpack_id: target.jobpack_id.clone(),
                state: target.state,
            },
        )
        .collect::<Vec<_>>();
    let synthetic = TestCheckpointSpec {
        id: "__target_validation__".into(),
        milestone_id: "__target_validation__".into(),
        boundary: CheckpointBoundaryKind::AfterJobpackSet,
        prerequisites: checkpoint_states,
        before_jobpack_id: None,
        cr_review_boundary: false,
        evidence_need_ids: vec![],
        modes: vec![crate::plan::EvidenceMode::Verify],
        goal: "validate Tester target".into(),
        criteria: vec!["exact prerequisite target".into()],
        required_capabilities: vec![],
        experiment_dimensions: vec![],
        evidence_outputs: vec![],
    };
    let current = build_tester_target_tx(tx, graph_version, &synthetic)?
        .context("Tester target prerequisites are not currently satisfied")?;

    if current.prerequisites.len() != target_binding.prerequisites.len() {
        bail!("Tester target binding is stale for current reviewed prerequisite state");
    }

    for target in &target_binding.prerequisites {
        let Some(observed) = current
            .prerequisites
            .iter()
            .find(|candidate| candidate.jobpack_id == target.jobpack_id)
        else {
            bail!("Tester target binding is stale for current reviewed prerequisite state");
        };

        if observed.state != target.state || observed.change_set_id != target.change_set_id {
            bail!("Tester target binding is stale for current reviewed prerequisite state");
        }

        if let (Some(expected_revision), Some(observed_revision)) =
            (&target.target_revision, &observed.target_revision)
        {
            if expected_revision != observed_revision {
                bail!("Tester target binding is stale for current reviewed prerequisite state");
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
enum TesterBoundaryState {
    NotReached,
    Reached,
    Crossed(String),
}

fn tester_boundary_state_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    checkpoint: &TestCheckpointSpec,
    target_ready: bool,
) -> Result<TesterBoundaryState> {
    match checkpoint.boundary {
        CheckpointBoundaryKind::AfterJobpackSet => {
            if target_ready {
                Ok(TesterBoundaryState::Reached)
            } else {
                Ok(TesterBoundaryState::NotReached)
            }
        }
        CheckpointBoundaryKind::BeforeJobpack => {
            let before = checkpoint
                .before_jobpack_id
                .as_deref()
                .context("BEFORE_JOBPACK checkpoint is missing before_jobpack_id")?;
            let status: Option<String> = tx
                .query_row(
                    r#"
                    SELECT status FROM execution_jobpacks
                    WHERE graph_version=?1 AND jobpack_id=?2
                    "#,
                    params![graph_version, before],
                    |row| row.get(0),
                )
                .optional()?;
            match status.as_deref() {
                Some("PENDING") if target_ready => Ok(TesterBoundaryState::Reached),
                Some("PENDING") => Ok(TesterBoundaryState::NotReached),
                Some("ACTIVE") | Some("DONE") => Ok(TesterBoundaryState::Crossed(format!(
                    "checkpoint {} BEFORE_JOBPACK boundary was crossed by {} state {:?}",
                    checkpoint.id, before, status
                ))),
                Some(other) => Ok(TesterBoundaryState::Crossed(format!(
                    "checkpoint {} gated Job Pack {} is in blocking state {}",
                    checkpoint.id, before, other
                ))),
                None => bail!(
                    "checkpoint {} references missing before Job Pack {}",
                    checkpoint.id,
                    before
                ),
            }
        }
        CheckpointBoundaryKind::MilestoneGate => {
            let status: Option<String> = tx
                .query_row(
                    r#"
                    SELECT status FROM execution_milestones
                    WHERE graph_version=?1 AND milestone_id=?2
                    "#,
                    params![graph_version, checkpoint.milestone_id],
                    |row| row.get(0),
                )
                .optional()?;
            match status.as_deref() {
                Some("VERIFY") if target_ready => Ok(TesterBoundaryState::Reached),
                Some("VERIFY") => Ok(TesterBoundaryState::Crossed(format!(
                    "milestone {} reached VERIFY before checkpoint {} prerequisites were ready",
                    checkpoint.milestone_id, checkpoint.id
                ))),
                Some("COMPLETE") => Ok(TesterBoundaryState::Crossed(format!(
                    "milestone {} completed before checkpoint {} was satisfied",
                    checkpoint.milestone_id, checkpoint.id
                ))),
                Some("ACTIVE") | Some("LOCKED") => Ok(TesterBoundaryState::NotReached),
                Some(other) => bail!(
                    "milestone {} has invalid checkpoint boundary state {}",
                    checkpoint.milestone_id,
                    other
                ),
                None => bail!(
                    "checkpoint {} references missing milestone {}",
                    checkpoint.id,
                    checkpoint.milestone_id
                ),
            }
        }
    }
}

fn build_tester_target_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    checkpoint: &TestCheckpointSpec,
) -> Result<Option<TesterTargetBinding>> {
    let mut prerequisites = Vec::with_capacity(checkpoint.prerequisites.len());
    for prerequisite in &checkpoint.prerequisites {
        let jobpack_status: Option<String> = tx
            .query_row(
                r#"
                SELECT status FROM execution_jobpacks
                WHERE graph_version=?1 AND jobpack_id=?2
                "#,
                params![graph_version, prerequisite.jobpack_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(jobpack_status) = jobpack_status else {
            bail!(
                "checkpoint {} prerequisite {} references missing Job Pack",
                checkpoint.id,
                prerequisite.jobpack_id
            );
        };

        let latest_review: Option<(String, String)> = tx
            .query_row(
                r#"
                SELECT change_set_id, verdict
                FROM code_reviews
                WHERE graph_version=?1 AND jobpack_id=?2
                ORDER BY id DESC LIMIT 1
                "#,
                params![graph_version, prerequisite.jobpack_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((change_set_id, verdict)) = latest_review else {
            return Ok(None);
        };
        if verdict != "PASS" {
            return Ok(None);
        }

        match prerequisite.state {
            PrerequisiteState::ReviewPass => match jobpack_status.as_str() {
                "ACTIVE" => {
                    let workflow_state: Option<(Option<String>, String)> = tx
                        .query_row(
                            r#"
                            SELECT change_set_id, status
                            FROM code_workflow_state
                            WHERE id=1 AND graph_version=?1 AND jobpack_id=?2
                            "#,
                            params![graph_version, prerequisite.jobpack_id],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )
                        .optional()?;
                    let Some((state_change_set, state_status)) = workflow_state else {
                        return Ok(None);
                    };
                    if state_status != "REVIEW_PASS"
                        || state_change_set.as_deref() != Some(change_set_id.as_str())
                    {
                        return Ok(None);
                    }
                }
                "DONE" => {}
                _ => return Ok(None),
            },
            PrerequisiteState::Done => {
                if jobpack_status != "DONE" {
                    return Ok(None);
                }
            }
        }

        prerequisites.push(TesterPrerequisiteTarget {
            jobpack_id: prerequisite.jobpack_id.clone(),
            state: prerequisite.state,
            change_set_id: Some(change_set_id),
            target_revision: None,
        });
    }
    let target = TesterTargetBinding { prerequisites };
    target.validate_against_checkpoint(checkpoint)?;
    Ok(Some(target))
}

fn reconcile_code_change_set_source_tx(
    tx: &Transaction<'_>,
    project_root: &Path,
    graph_version: i64,
    jobpack_id: &str,
    change_set_id: &str,
) -> Result<(RecoveryClassification, String)> {
    let journal_json: Option<String> = tx
        .query_row(
            r#"
            SELECT mutation_journal
            FROM code_checkpoints
            WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
            ORDER BY id DESC LIMIT 1
            "#,
            params![graph_version, jobpack_id, change_set_id],
            |row| row.get(0),
        )
        .optional()?;
    let journal_json = journal_json.context("code change set has no persisted mutation journal")?;
    let journal: serde_json::Value = serde_json::from_str(&journal_json)?;
    let entries = journal
        .as_array()
        .context("persisted mutation journal must be an array")?;
    let canonical_root = project_root.canonicalize()?;

    let mut saw_stale = false;
    let mut saw_ahead = false;
    let mut saw_diverged = false;
    let mut details = Vec::new();

    for entry in entries {
        let path = entry
            .get("path")
            .and_then(serde_json::Value::as_str)
            .context("mutation journal entry is missing path")?;
        let expected_after = entry
            .get("after_sha256")
            .and_then(serde_json::Value::as_str)
            .context("mutation journal entry is missing after_sha256")?;
        let expected_before = entry
            .get("before_sha256")
            .and_then(serde_json::Value::as_str);

        let candidate = canonical_root.join(path);
        let canonical = match candidate.canonicalize() {
            Ok(canonical) if canonical.starts_with(&canonical_root) => canonical,
            Ok(_) => {
                saw_diverged = true;
                details.push(format!("{path}: path escaped project root"));
                continue;
            }
            Err(error) => {
                saw_diverged = true;
                details.push(format!("{path}: source path unavailable ({error})"));
                continue;
            }
        };

        let bytes = match fs::read(&canonical) {
            Ok(bytes) => bytes,
            Err(error) => {
                saw_diverged = true;
                details.push(format!("{path}: source unreadable ({error})"));
                continue;
            }
        };
        let digest = Sha256::digest(bytes);
        let observed = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        if observed == expected_after {
            continue;
        }

        if expected_before.is_some_and(|before| observed == before) {
            saw_stale = true;
            details.push(format!(
                "{path}: live source matches durable before-state instead of reviewed after-state"
            ));
        } else {
            saw_ahead = true;
            details.push(format!(
                "{path}: live source is an unbound revision (expected after {expected_after}, observed {observed})"
            ));
        }
    }

    let classification = if saw_diverged || (saw_stale && saw_ahead) {
        RecoveryClassification::SourceDiverged
    } else if saw_ahead {
        RecoveryClassification::SourceAhead
    } else if saw_stale {
        RecoveryClassification::SourceStale
    } else {
        RecoveryClassification::DurableExact
    };

    let reason = match classification {
        RecoveryClassification::DurableExact => {
            format!("live source exactly matches durable change set {change_set_id}")
        }
        RecoveryClassification::SourceStale => format!(
            "live source is stale relative to durable change set {change_set_id}: {}",
            details.join("; ")
        ),
        RecoveryClassification::SourceAhead => format!(
            "live source is ahead or externally modified relative to durable change set {change_set_id}; automatic adoption is prohibited: {}",
            details.join("; ")
        ),
        RecoveryClassification::SourceDiverged => format!(
            "live source diverged ambiguously from durable change set {change_set_id}: {}",
            details.join("; ")
        ),
        RecoveryClassification::Blocked | RecoveryClassification::NeedsHuman => {
            unreachable!("source reconciliation never emits generic blocker classifications")
        }
    };

    Ok((classification, reason))
}

fn assert_code_change_set_current_tx(
    tx: &Transaction<'_>,
    project_root: &Path,
    graph_version: i64,
    jobpack_id: &str,
    change_set_id: &str,
) -> Result<()> {
    let (classification, reason) = reconcile_code_change_set_source_tx(
        tx,
        project_root,
        graph_version,
        jobpack_id,
        change_set_id,
    )?;
    if classification != RecoveryClassification::DurableExact {
        bail!("{reason}");
    }
    Ok(())
}

fn code_cr_boundary_work_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
) -> Result<Option<CodeCrBoundaryWork>> {
    let Some(work) = active_work_tx(tx, graph_version)? else {
        return Ok(None);
    };

    let workflow_state: Option<(Option<String>, String)> = tx
        .query_row(
            r#"
            SELECT change_set_id, status
            FROM code_workflow_state
            WHERE id=1 AND graph_version=?1 AND jobpack_id=?2
            "#,
            params![graph_version, work.jobpack_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((Some(change_set_id), workflow_status)) = workflow_state else {
        return Ok(None);
    };
    if workflow_status != "REVIEW_PASS" {
        return Ok(None);
    }

    let latest_review: Option<String> = tx
        .query_row(
            r#"
            SELECT verdict
            FROM code_reviews
            WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
            ORDER BY id DESC LIMIT 1
            "#,
            params![graph_version, work.jobpack_id, change_set_id],
            |row| row.get(0),
        )
        .optional()?;
    if latest_review.as_deref() != Some("PASS") {
        return Ok(None);
    }

    let verification: Option<(i64, String)> = tx
        .query_row(
            r#"
            SELECT id, result
            FROM verification_runs
            WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
            ORDER BY id DESC LIMIT 1
            "#,
            params![graph_version, work.jobpack_id, change_set_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((verification_run_id, verification_result)) = verification else {
        return Ok(None);
    };
    if verification_result != "TEST_PASS" {
        return Ok(None);
    }

    let checkpoints = {
        let mut statement = tx.prepare(
            r#"
            SELECT definition_json
            FROM execution_test_checkpoints
            WHERE graph_version=?1
            ORDER BY checkpoint_id
            "#,
        )?;
        let values = statement
            .query_map(params![graph_version], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        values
    };

    let mut tester_evidence = Vec::new();
    let mut cr_checkpoint_boundaries = Vec::new();
    let mut required_checkpoint_ready = true;

    for checkpoint_json in checkpoints {
        let checkpoint: TestCheckpointSpec = serde_json::from_str(&checkpoint_json)?;
        if !checkpoint
            .prerequisites
            .iter()
            .any(|item| item.jobpack_id == work.jobpack_id)
        {
            continue;
        }

        let target = build_tester_target_tx(tx, graph_version, &checkpoint)?;
        let boundary = tester_boundary_state_tx(tx, graph_version, &checkpoint, target.is_some())?;
        if matches!(boundary, TesterBoundaryState::NotReached) {
            continue;
        }

        let Some(target) = target else {
            required_checkpoint_ready = false;
            continue;
        };
        let target_fingerprint = target.fingerprint(graph_version, &checkpoint.id)?;
        let attempt = latest_tester_attempt_for_target_tx(
            tx,
            graph_version,
            &checkpoint.id,
            &target_fingerprint,
        )?;
        let Some(attempt) = attempt else {
            required_checkpoint_ready = false;
            continue;
        };
        if !tester_attempt_satisfied(&attempt) {
            required_checkpoint_ready = false;
            continue;
        }

        tester_evidence.push(CodeCrTesterEvidenceRef {
            checkpoint_id: checkpoint.id.clone(),
            attempt_id: attempt.attempt_id.clone(),
            target_fingerprint,
        });
        if checkpoint.cr_review_boundary {
            cr_checkpoint_boundaries.push(format!("CHECKPOINT:{}", checkpoint.id));
        }
    }

    let incomplete_todos: i64 = tx.query_row(
        r#"
        SELECT COUNT(*)
        FROM execution_todos
        WHERE graph_version=?1 AND jobpack_id=?2 AND status!='DONE'
        "#,
        params![graph_version, work.jobpack_id],
        |row| row.get(0),
    )?;
    let incomplete_checklist: i64 = tx.query_row(
        r#"
        SELECT COUNT(*)
        FROM execution_checklist_items c
        JOIN execution_todos t
          ON t.graph_version=c.graph_version AND t.todo_id=c.todo_id
        WHERE c.graph_version=?1 AND t.jobpack_id=?2 AND c.checked=0
        "#,
        params![graph_version, work.jobpack_id],
        |row| row.get(0),
    )?;

    tester_evidence.sort_by(|left, right| left.checkpoint_id.cmp(&right.checkpoint_id));
    let terminal = incomplete_todos == 0 && incomplete_checklist == 0 && required_checkpoint_ready;

    let mut boundary_ids = cr_checkpoint_boundaries;
    if terminal {
        boundary_ids.push("JOBPACK_TERMINAL".into());
    }
    boundary_ids.sort();
    boundary_ids.dedup();
    if boundary_ids.is_empty() {
        return Ok(None);
    }

    let evidence_bytes = serde_json::to_vec(&serde_json::json!({
        "verification_run_id": verification_run_id,
        "verification_result": verification_result,
        "tester_evidence": tester_evidence
    }))?;
    let digest = Sha256::digest(evidence_bytes);
    let evidence_fingerprint = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let boundary_id = boundary_ids.join("+");
    let key = CodeCrBoundaryKey {
        graph_version,
        jobpack_id: work.jobpack_id,
        change_set_id,
        boundary_id,
        evidence_fingerprint,
    };
    let existing_review = {
        let row: Option<(String, String)> = tx
            .query_row(
                r#"
                SELECT verdict, findings
                FROM code_cr_reviews
                WHERE graph_version=?1 AND jobpack_id=?2 AND change_set_id=?3
                  AND boundary_id=?4 AND evidence_fingerprint=?5
                "#,
                params![
                    key.graph_version,
                    key.jobpack_id,
                    key.change_set_id,
                    key.boundary_id,
                    key.evidence_fingerprint
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(verdict, findings)| {
            Ok::<CodeCrReviewRecord, anyhow::Error>(CodeCrReviewRecord {
                key: key.clone(),
                verdict,
                findings: serde_json::from_str(&findings)?,
            })
        })
        .transpose()?
    };

    Ok(Some(CodeCrBoundaryWork {
        key,
        boundary_ids,
        verification_run_id,
        tester_evidence,
        terminal,
        existing_review,
    }))
}

fn latest_tester_attempt_for_target_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    checkpoint_id: &str,
    target_fingerprint: &str,
) -> Result<Option<TesterAttemptEvidence>> {
    let json: Option<String> = tx
        .query_row(
            r#"
            SELECT attempt_json
            FROM tester_evidence_attempts
            WHERE graph_version=?1 AND checkpoint_id=?2 AND target_fingerprint=?3
            ORDER BY created_at DESC, attempt_id DESC LIMIT 1
            "#,
            params![graph_version, checkpoint_id, target_fingerprint],
            |row| row.get(0),
        )
        .optional()?;
    json.map(|value| serde_json::from_str(&value).map_err(Into::into))
        .transpose()
}

fn tester_attempt_satisfied(attempt: &TesterAttemptEvidence) -> bool {
    attempt
        .mode_results
        .iter()
        .all(|result| result.satisfies_output_requirement())
}

fn tester_attempt_blocking_state(
    attempt: &TesterAttemptEvidence,
) -> (TesterCheckpointDisposition, String) {
    if attempt
        .mode_results
        .iter()
        .any(|result| result.outcome == TesterModeOutcome::NeedsHuman)
    {
        return (
            TesterCheckpointDisposition::NeedsHuman,
            format!(
                "Tester attempt {} requires Human evidence",
                attempt.attempt_id
            ),
        );
    }
    if attempt
        .classifications
        .iter()
        .any(|item| *item == TesterClassification::ProductFailure)
    {
        let detail = tester_failure_summary(attempt);
        return (
            TesterCheckpointDisposition::ProductFailure,
            format!(
                "Tester attempt {} classified PRODUCT_FAILURE: {}",
                attempt.attempt_id, detail
            ),
        );
    }
    if attempt
        .classifications
        .iter()
        .any(|item| *item == TesterClassification::SpecGap)
    {
        return (
            TesterCheckpointDisposition::SpecGap,
            format!(
                "Tester attempt {} classified SPEC_GAP: {}",
                attempt.attempt_id,
                tester_failure_summary(attempt)
            ),
        );
    }
    if attempt
        .classifications
        .iter()
        .any(|item| *item == TesterClassification::IntegrationNotReady)
    {
        return (
            TesterCheckpointDisposition::IntegrationNotReady,
            format!(
                "Tester attempt {} classified the checkpoint as INTEGRATION_NOT_READY",
                attempt.attempt_id
            ),
        );
    }

    (
        TesterCheckpointDisposition::Blocked,
        format!(
            "Tester attempt {} did not satisfy every declared checkpoint mode",
            attempt.attempt_id
        ),
    )
}

fn tester_failure_summary(attempt: &TesterAttemptEvidence) -> String {
    let mut reasons = attempt
        .mode_results
        .iter()
        .filter_map(|result| result.reason.as_deref())
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    reasons.extend(
        attempt
            .limitations
            .iter()
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .map(str::to_owned),
    );
    reasons.sort();
    reasons.dedup();
    if reasons.is_empty() {
        "checkpoint behavior did not satisfy the declared contract".into()
    } else {
        reasons.join("; ")
    }
}

fn tester_product_failure_context(
    attempt: &TesterAttemptEvidence,
    target_fingerprint: &str,
    failure_summary: &str,
) -> Option<TesterRetestContext> {
    attempt
        .classifications
        .iter()
        .any(|item| *item == TesterClassification::ProductFailure)
        .then(|| TesterRetestContext {
            failed_attempt_id: attempt.attempt_id.clone(),
            failed_target_fingerprint: target_fingerprint.to_owned(),
            failure_summary: failure_summary.to_owned(),
        })
}

fn latest_product_failure_context_for_previous_target_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    checkpoint_id: &str,
    current_target_fingerprint: &str,
) -> Result<Option<TesterRetestContext>> {
    let row: Option<(String, String)> = tx
        .query_row(
            r#"
            SELECT target_fingerprint, attempt_json
            FROM tester_evidence_attempts
            WHERE graph_version=?1 AND checkpoint_id=?2
            ORDER BY created_at DESC, attempt_id DESC LIMIT 1
            "#,
            params![graph_version, checkpoint_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((target_fingerprint, attempt_json)) = row else {
        return Ok(None);
    };
    if target_fingerprint == current_target_fingerprint {
        return Ok(None);
    }
    let attempt: TesterAttemptEvidence = serde_json::from_str(&attempt_json)?;
    if !attempt
        .classifications
        .iter()
        .any(|item| *item == TesterClassification::ProductFailure)
    {
        return Ok(None);
    }
    let summary = format!(
        "Retest after {}: {}",
        attempt.attempt_id,
        tester_failure_summary(&attempt)
    );
    Ok(Some(TesterRetestContext {
        failed_attempt_id: attempt.attempt_id,
        failed_target_fingerprint: target_fingerprint,
        failure_summary: summary,
    }))
}

fn validate_tester_evidence_refs(
    tx: &Transaction<'_>,
    project_root: &Path,
    attempt: &TesterAttemptEvidence,
) -> Result<()> {
    let mut workspace: Option<TesterWorkspaceRuntime> = None;
    let refs = attempt
        .experiment
        .iter()
        .flat_map(|experiment| experiment.samples.iter())
        .flat_map(|sample| sample.observations.iter())
        .flat_map(|observation| observation.evidence_refs.iter())
        .chain(
            attempt
                .outputs
                .iter()
                .flat_map(|output| output.evidence_refs.iter()),
        );

    for evidence_ref in refs {
        match evidence_ref {
            TesterEvidenceRef::WorkspaceArtifact { artifact } => {
                if artifact.graph_version != attempt.graph_version
                    || artifact.checkpoint_id != attempt.checkpoint_id
                    || artifact.attempt_id != attempt.attempt_id
                {
                    bail!("workspace artifact evidence ref is bound to another Tester attempt");
                }
                if workspace.is_none() {
                    workspace = Some(TesterWorkspaceRuntime::new(
                        project_root,
                        attempt.graph_version,
                        &attempt.checkpoint_id,
                        &attempt.attempt_id,
                    )?);
                }
                let observed = workspace
                    .as_ref()
                    .expect("workspace initialized")
                    .artifact_ref(&artifact.path)?;
                if &observed != artifact {
                    bail!(
                        "workspace artifact evidence ref is stale for {}",
                        artifact.path
                    );
                }
            }
            TesterEvidenceRef::VerificationRun { run_id } => {
                validate_verification_ref_target_tx(tx, attempt, *run_id)?;
            }
            TesterEvidenceRef::VerificationObservation {
                run_id,
                command_id,
                field,
                observed,
            } => {
                let commands = validate_verification_ref_target_tx(tx, attempt, *run_id)?;
                let command = commands
                    .iter()
                    .find(|command| command.command_id == *command_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "verification observation command {} is missing from run {}",
                            command_id,
                            run_id
                        )
                    })?;
                let actual = match field {
                    VerificationObservationField::Stdout => {
                        ObservedValue::Text(command.stdout.clone())
                    }
                    VerificationObservationField::Stderr => {
                        ObservedValue::Text(command.stderr.clone())
                    }
                    VerificationObservationField::ExitCode => {
                        let code = command.exit_code.ok_or_else(|| {
                            anyhow::anyhow!(
                                "verification observation run {} command {} has no exit code",
                                run_id,
                                command_id
                            )
                        })?;
                        ObservedValue::Integer(i64::from(code))
                    }
                };
                if &actual != observed {
                    bail!(
                        "verification observation {}.{} does not match persisted command evidence",
                        run_id,
                        command_id
                    );
                }
            }
            TesterEvidenceRef::AdapterObservation {
                adapter_id,
                execution_id,
                field,
                observed,
            } => {
                let row: Option<(String, String, String)> = tx
                    .query_row(
                        r#"
                        SELECT target_fingerprint, adapter_id, observation_json
                        FROM tester_execution_steps
                        WHERE graph_version=?1 AND checkpoint_id=?2
                          AND attempt_id=?3 AND execution_id=?4
                          AND status IN ('COMPLETED','FAILED')
                        "#,
                        params![
                            attempt.graph_version,
                            attempt.checkpoint_id,
                            attempt.attempt_id,
                            execution_id
                        ],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .optional()?;
                let Some((target_fingerprint, stored_adapter_id, observation_json)) = row else {
                    bail!(
                        "adapter observation {} has no completed execution record",
                        execution_id
                    );
                };
                if target_fingerprint != attempt.target_fingerprint()? {
                    bail!(
                        "adapter observation {} is bound to another Tester target",
                        execution_id
                    );
                }
                if stored_adapter_id != *adapter_id {
                    bail!(
                        "adapter observation {} adapter mismatch: expected {}, found {}",
                        execution_id,
                        adapter_id,
                        stored_adapter_id
                    );
                }
                let observation =
                    serde_json::from_str::<TesterExecutionObservation>(&observation_json)?;
                let actual = match field {
                    AdapterObservationField::Stdout => {
                        ObservedValue::Text(observation.stdout.clone())
                    }
                    AdapterObservationField::Stderr => {
                        ObservedValue::Text(observation.stderr.clone())
                    }
                    AdapterObservationField::ExitCode => {
                        let code = observation.exit_code.ok_or_else(|| {
                            anyhow::anyhow!("adapter observation {} has no exit code", execution_id)
                        })?;
                        ObservedValue::Integer(i64::from(code))
                    }
                };
                if &actual != observed {
                    bail!(
                        "adapter observation {} does not match persisted execution output",
                        execution_id
                    );
                }
            }
        }
    }
    Ok(())
}

fn validate_verification_ref_target_tx(
    tx: &Transaction<'_>,
    attempt: &TesterAttemptEvidence,
    run_id: i64,
) -> Result<Vec<CommandEvidence>> {
    let binding: Option<(i64, String, String, String)> = tx
        .query_row(
            r#"
            SELECT graph_version, jobpack_id, change_set_id, commands_json
            FROM verification_runs
            WHERE id=?1
            "#,
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((graph_version, jobpack_id, change_set_id, commands_json)) = binding else {
        bail!("verification evidence ref {} does not exist", run_id);
    };
    if graph_version != attempt.graph_version {
        bail!(
            "verification evidence ref {} is bound to another graph",
            run_id
        );
    }
    let matches_target = attempt.target.prerequisites.iter().any(|target| {
        target.jobpack_id == jobpack_id
            && target.change_set_id.as_deref() == Some(change_set_id.as_str())
    });
    if !matches_target {
        bail!(
            "verification evidence ref {} is not bound to the Tester target",
            run_id
        );
    }
    Ok(serde_json::from_str::<Vec<CommandEvidence>>(
        &commands_json,
    )?)
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

fn assert_active_jobpack_binding_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    jobpack_id: &str,
) -> Result<(i64, String)> {
    let Some((current_graph, plan_revision, _plan_hash)) = current_graph_binding_tx(tx)? else {
        bail!("no CURRENT approved execution graph is bound");
    };
    if current_graph != graph_version {
        bail!(
            "stale code target graph: requested {}, current {}",
            graph_version,
            current_graph
        );
    }
    let work = active_work_tx(tx, graph_version)?
        .context("current execution graph has no ACTIVE Job Pack")?;
    if work.jobpack_id != jobpack_id {
        bail!(
            "stale code target Job Pack: requested {}, current {}",
            jobpack_id,
            work.jobpack_id
        );
    }
    Ok((plan_revision, work.milestone_id))
}

fn validate_checklist_claims_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    jobpack_id: &str,
    claims: &[ChecklistClaim],
) -> Result<()> {
    let mut seen = HashSet::new();
    for claim in claims {
        if claim.todo_id.trim().is_empty() || claim.position <= 0 {
            bail!("invalid checklist completion claim");
        }
        if !seen.insert((claim.todo_id.as_str(), claim.position)) {
            bail!(
                "duplicate checklist completion claim {}#{}",
                claim.todo_id,
                claim.position
            );
        }
        let exists: i64 = tx.query_row(
            r#"
            SELECT COUNT(*)
            FROM execution_checklist_items c
            JOIN execution_todos t
              ON t.graph_version=c.graph_version AND t.todo_id=c.todo_id
            WHERE c.graph_version=?1
              AND c.todo_id=?2
              AND c.position=?3
              AND t.jobpack_id=?4
            "#,
            params![graph_version, claim.todo_id, claim.position, jobpack_id],
            |row| row.get(0),
        )?;
        if exists != 1 {
            bail!(
                "checklist claim {}#{} is not part of active Job Pack {}",
                claim.todo_id,
                claim.position,
                jobpack_id
            );
        }
    }
    Ok(())
}

fn apply_checklist_claims_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    jobpack_id: &str,
    claims: &[ChecklistClaim],
) -> Result<()> {
    validate_checklist_claims_tx(tx, graph_version, jobpack_id, claims)?;
    for claim in claims {
        let updated = tx.execute(
            r#"
            UPDATE execution_checklist_items
            SET checked=1
            WHERE graph_version=?1 AND todo_id=?2 AND position=?3
              AND EXISTS (
                  SELECT 1 FROM execution_todos t
                  WHERE t.graph_version=execution_checklist_items.graph_version
                    AND t.todo_id=execution_checklist_items.todo_id
                    AND t.jobpack_id=?4
              )
            "#,
            params![graph_version, claim.todo_id, claim.position, jobpack_id],
        )?;
        if updated != 1 {
            bail!(
                "checklist claim {}#{} changed before Reviewer PASS",
                claim.todo_id,
                claim.position
            );
        }
    }

    tx.execute(
        r#"
        UPDATE execution_todos
        SET status=CASE
            WHEN NOT EXISTS (
                SELECT 1 FROM execution_checklist_items c
                WHERE c.graph_version=execution_todos.graph_version
                  AND c.todo_id=execution_todos.todo_id
                  AND c.checked=0
            )
            THEN 'DONE'
            ELSE 'PENDING'
        END
        WHERE graph_version=?1 AND jobpack_id=?2
        "#,
        params![graph_version, jobpack_id],
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
        jobpack_id: row.6.clone(),
        jobpack_title: row.7,
        jobpack_status: row.8,
        goal: row.9,
        required_inputs: serde_json::from_str(&row.10)?,
        expected_outputs: serde_json::from_str(&row.11)?,
        acceptance: serde_json::from_str(&row.12)?,
        verification_hints: serde_json::from_str(&row.13)?,
        tester_evidence: resolve_required_tester_evidence_tx(tx, graph_version, &row.6)?,
    }))
}

fn tester_output_mode_satisfied(
    attempt: &TesterAttemptEvidence,
    mode: crate::plan::EvidenceMode,
) -> bool {
    attempt
        .mode_results
        .iter()
        .find(|result| result.mode == mode)
        .map(|result| result.satisfies_output_requirement())
        .unwrap_or(false)
}

fn applicability_context_for_attempt(attempt: &TesterAttemptEvidence) -> ApplicabilityContext {
    let mut context = ApplicabilityContext::default();
    if attempt.target.prerequisites.len() == 1 {
        let target = &attempt.target.prerequisites[0];
        context.change_set_id = target.change_set_id.clone();
        context.product_revision = target.target_revision.clone();
    }
    if let Some(sample) = attempt
        .experiment
        .as_ref()
        .and_then(|experiment| experiment.samples.last())
    {
        if context.product_revision.is_none() {
            context.product_revision = sample.target_revision.clone();
        }
        if context.change_set_id.is_none() {
            context.change_set_id = sample.change_set_id.clone();
        }
        context.runtime_identity = sample.runtime_identity.clone();
        context.capability_fingerprint = sample.capability_fingerprint.clone();
    }
    context
}

fn resolve_required_tester_evidence_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    jobpack_id: &str,
) -> Result<Vec<ResolvedTesterEvidence>> {
    let requirements = {
        let mut statement = tx.prepare(
            r#"
            SELECT checkpoint_id, output_id, required
            FROM execution_evidence_requirements
            WHERE graph_version=?1 AND consumer_jobpack_id=?2
            ORDER BY checkpoint_id, output_id
            "#,
        )?;
        let rows = statement
            .query_map(params![graph_version, jobpack_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? != 0,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };

    let mut resolved = Vec::new();
    for (checkpoint_id, output_id, required) in requirements {
        let row: Option<(String, String)> = tx
            .query_row(
                r#"
                SELECT r.attempt_id, r.record_json
                FROM tester_evidence_records r
                JOIN tester_evidence_attempts a
                  ON a.graph_version=r.graph_version
                 AND a.checkpoint_id=r.checkpoint_id
                 AND a.attempt_id=r.attempt_id
                WHERE r.graph_version=?1
                  AND r.checkpoint_id=?2
                  AND r.output_id=?3
                ORDER BY r.id DESC
                LIMIT 1
                "#,
                params![graph_version, checkpoint_id, output_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        let Some((attempt_id, record_json)) = row else {
            if required {
                bail!(
                    "PLAN_GAP: required Tester evidence {}:{} for consumer {} is missing",
                    checkpoint_id,
                    output_id,
                    jobpack_id
                );
            }
            continue;
        };

        let record: TesterEvidenceOutputRecord = serde_json::from_str(&record_json)?;
        if record.provenance != EvidenceProvenance::Observed {
            if required {
                bail!(
                    "PLAN_GAP: required Tester evidence {}:{} for consumer {} is not OBSERVED",
                    checkpoint_id,
                    output_id,
                    jobpack_id
                );
            }
            continue;
        }

        let attempt = tester_attempt_evidence_tx(tx, graph_version, &checkpoint_id, &attempt_id)?
            .context("Tester evidence output references a missing attempt")?;
        if let Err(error) = validate_tester_target_tx(tx, graph_version, &attempt.target) {
            if required {
                bail!(
                    "PLAN_GAP: required Tester evidence {}:{} for consumer {} has a stale target: {:#}",
                    checkpoint_id,
                    output_id,
                    jobpack_id,
                    error
                );
            }
            continue;
        }
        if !tester_output_mode_satisfied(&attempt, record.mode) {
            if required {
                bail!(
                    "PLAN_GAP: required Tester evidence {}:{} for consumer {} came from an unsuccessful {:?} mode",
                    checkpoint_id,
                    output_id,
                    jobpack_id,
                    record.mode
                );
            }
            continue;
        }

        let context = applicability_context_for_attempt(&attempt);
        match record.applicability.evaluate(&context)? {
            ApplicabilityDecision::Compatible => {}
            ApplicabilityDecision::Invalidated | ApplicabilityDecision::RevalidationRequired => {
                if required {
                    bail!(
                        "PLAN_GAP: required Tester evidence {}:{} for consumer {} is stale or requires revalidation",
                        checkpoint_id,
                        output_id,
                        jobpack_id
                    );
                }
                continue;
            }
        }

        let value = record
            .value
            .clone()
            .context("OBSERVED Tester evidence is missing its value")?;
        resolved.push(ResolvedTesterEvidence {
            checkpoint_id,
            output_id,
            attempt_id,
            value,
            evidence_refs: record.evidence_refs.clone(),
        });
    }
    Ok(resolved)
}

fn tester_attempt_evidence_tx(
    tx: &Transaction<'_>,
    graph_version: i64,
    checkpoint_id: &str,
    attempt_id: &str,
) -> Result<Option<TesterAttemptEvidence>> {
    let json: Option<String> = tx
        .query_row(
            r#"
            SELECT attempt_json
            FROM tester_evidence_attempts
            WHERE graph_version=?1 AND checkpoint_id=?2 AND attempt_id=?3
            "#,
            params![graph_version, checkpoint_id, attempt_id],
            |row| row.get(0),
        )
        .optional()?;
    json.map(|value| serde_json::from_str(&value).map_err(Into::into))
        .transpose()
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
fn pid_owner_known_dead(owner: &str) -> bool {
    let Some(pid) = owner
        .strip_prefix("pid:")
        .and_then(|value| value.parse::<i32>().ok())
    else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    !owner_process_alive(owner)
}

#[cfg(not(unix))]
fn pid_owner_known_dead(_owner: &str) -> bool {
    false
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
    fn pid_reclaim_requires_positive_dead_process_evidence() {
        let current = format!("pid:{}", std::process::id());
        assert!(!pid_owner_known_dead(&current));
        assert!(pid_owner_known_dead("pid:999999999"));
        assert!(!pid_owner_known_dead("pid:not-a-number"));
        assert!(!pid_owner_known_dead("owner-a"));
    }

    #[cfg(unix)]
    #[test]
    fn dead_pid_lease_is_reclaimed_immediately_for_crash_resume() {
        let (dir, registry) = registry();
        registry
            .acquire_lease(
                dir.path(),
                "pid:999999999",
                Duration::from_secs(6 * 60 * 60),
            )
            .unwrap();

        let owner = format!("pid:{}", std::process::id());
        registry
            .acquire_lease(dir.path(), &owner, Duration::from_secs(6 * 60 * 60))
            .unwrap();

        let stored: String = registry
            .conn
            .query_row(
                "SELECT owner FROM execution_lease WHERE project_root=?1",
                params![canonical_or_original(dir.path())
                    .to_string_lossy()
                    .into_owned()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, owner);
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
    fn old_incomplete_planning_is_preserved_and_blocks_implicit_reset() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("planning.db");
        {
            let old = Registry::open_at(&db).unwrap();
            old.begin_plan_workflow().unwrap();
            old.set_workflow_state(Some(7), 2, 1, "REVIEWER").unwrap();
        }
        let registry = Registry::open_at(&db).unwrap();
        let state = registry.planning_run_state().unwrap().unwrap();
        assert_eq!(state.stage, "LEGACY_RECOVERY_REQUIRED");
        assert!(state.requirement.is_none());
        assert!(state
            .legacy_snapshot_json
            .as_ref()
            .unwrap()
            .contains("REVIEWER"));
        let old = registry.workflow_state().unwrap();
        assert_eq!(old.current_revision, Some(7));
        assert_eq!(old.status, "REVIEWER");
        assert!(registry.begin_plan_workflow().is_err());

        let decision = registry.resolve_resume_decision(dir.path(), &[]).unwrap();
        assert_eq!(decision.classification, RecoveryClassification::NeedsHuman);
        assert_eq!(decision.action, ResumeAction::BlockedNeedsHuman);
        assert!(decision.reason.contains("LEGACY_RECOVERY_REQUIRED"));
    }

    #[test]
    fn human_decline_and_empty_requirement_do_not_mutate_legacy_recovery() {
        let (_dir, registry) = registry();
        registry.begin_plan_workflow().unwrap();
        registry.set_workflow_state(None, 0, 0, "PLANNING").unwrap();
        registry.migrate_planning_run_state().unwrap();

        let count_events = || -> i64 {
            registry
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM events WHERE kind='LEGACY_PLANNING_ABANDONED'",
                    [],
                    |row| row.get(0),
                )
                .unwrap()
        };
        for input in ["", "   ", "Create implementation plan"] {
            assert!(registry
                .confirm_legacy_planning_recovery(input, false)
                .is_err());
        }
        assert!(registry
            .confirm_legacy_planning_recovery("  ", true)
            .is_err());
        assert_eq!(count_events(), 0);
        assert_eq!(
            registry.planning_run_state().unwrap().unwrap().stage,
            "LEGACY_RECOVERY_REQUIRED"
        );
        assert_eq!(registry.workflow_state().unwrap().status, "PLANNING");
    }

    #[test]
    fn confirmed_legacy_recovery_persists_exact_requirement_and_event_atomically() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("recovery.db");
        {
            let registry = Registry::open_at(&db).unwrap();
            registry.begin_plan_workflow().unwrap();
            registry
                .set_workflow_state(Some(4), 1, 0, "REVIEWER")
                .unwrap();
        }
        let registry = Registry::open_at(&db).unwrap();
        let requirement = "Create an implementation plan for the new requirements.";
        let state = registry
            .confirm_legacy_planning_recovery(requirement, true)
            .unwrap();
        assert_eq!(state.stage, "PLANNER");
        assert_eq!(state.requirement.as_deref(), Some(requirement));
        assert!(state.legacy_snapshot_json.is_some());
        assert!(registry.begin_plan_workflow().is_err());
        assert!(registry
            .confirm_legacy_planning_recovery(requirement, true)
            .is_err());

        let (kind, payload): (String, String) = registry
            .conn
            .query_row(
                "SELECT kind, payload FROM events WHERE kind='LEGACY_PLANNING_ABANDONED'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, "LEGACY_PLANNING_ABANDONED");
        let event: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(event["new_requirement"], requirement);
        assert_eq!(event["legacy"]["current_revision"], 4);
        drop(registry);

        let reopened = Registry::open_at(&db).unwrap();
        assert_eq!(
            reopened
                .planning_run_state()
                .unwrap()
                .unwrap()
                .requirement
                .as_deref(),
            Some(requirement)
        );
        let count: i64 = reopened
            .conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='LEGACY_PLANNING_ABANDONED'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(reopened.workflow_state().unwrap().status, "REVIEWER");
    }

    #[test]
    fn recovery_event_write_failure_rolls_back_planning_state_transition() {
        let (_dir, registry) = registry();
        registry.begin_plan_workflow().unwrap();
        registry.migrate_planning_run_state().unwrap();
        registry
            .conn
            .execute_batch(
                "CREATE TRIGGER reject_legacy_abandon BEFORE INSERT ON events
             WHEN NEW.kind='LEGACY_PLANNING_ABANDONED'
             BEGIN SELECT RAISE(ABORT, 'blocked event'); END;",
            )
            .unwrap();
        assert!(registry
            .confirm_legacy_planning_recovery("new plan", true)
            .is_err());
        let state = registry.planning_run_state().unwrap().unwrap();
        assert_eq!(state.stage, "LEGACY_RECOVERY_REQUIRED");
        assert!(state.requirement.is_none());
    }

    #[test]
    fn approved_without_registered_current_graph_is_ambiguous_legacy() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("partial-approval.db");
        {
            let registry = Registry::open_at(&db).unwrap();
            registry
                .conn
                .execute(
                    "INSERT INTO plan_revisions (revision, plan_hash, content, created_at)
                     VALUES (1, 'hash', '{}', 1)",
                    [],
                )
                .unwrap();
            registry
                .conn
                .execute(
                    "INSERT INTO approved_plan (id, revision, plan_hash, execution_graph_version)
                     VALUES (1, 1, 'hash', 0)",
                    [],
                )
                .unwrap();
            registry
                .conn
                .execute(
                    "INSERT INTO plan_workflow_state (id, current_revision, status)
                     VALUES (1, 1, 'APPROVED')",
                    [],
                )
                .unwrap();
        }
        let reopened = Registry::open_at(&db).unwrap();
        assert_eq!(
            reopened.planning_run_state().unwrap().unwrap().stage,
            "LEGACY_RECOVERY_REQUIRED"
        );
        assert_eq!(reopened.workflow_state().unwrap().status, "APPROVED");
    }

    #[test]
    fn completed_and_empty_legacy_are_not_mistaken_for_pending_recovery() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("complete.db");
        {
            let registry = Registry::open_at(&db).unwrap();
            registry
                .conn
                .execute(
                    "INSERT INTO plan_revisions (revision, plan_hash, content, created_at)
                 VALUES (3, 'hash', '{}', 1)",
                    [],
                )
                .unwrap();
            registry
                .conn
                .execute(
                    "INSERT INTO approved_plan (id, revision, plan_hash, execution_graph_version)
                 VALUES (1, 3, 'hash', 1)",
                    [],
                )
                .unwrap();
            registry
                .conn
                .execute(
                    "INSERT INTO execution_graph (version, plan_revision, plan_hash, status)
                     VALUES (1, 3, 'hash', 'CURRENT')",
                    [],
                )
                .unwrap();
            for actor in ["REVIEWER", "LOCAL_CR"] {
                registry
                    .conn
                    .execute(
                        "INSERT INTO plan_verdicts
                         (actor, revision, plan_hash, verdict, findings, created_at)
                         VALUES (?1, 3, 'hash', 'PASS', '[]', 1)",
                        params![actor],
                    )
                    .unwrap();
            }
            registry
                .conn
                .execute(
                    "INSERT INTO plan_workflow_state
                 (id, current_revision, reviewer_attempts, cr_attempts, status)
                 VALUES (1, 3, 1, 1, 'APPROVED')",
                    [],
                )
                .unwrap();
        }
        let reopened = Registry::open_at(&db).unwrap();
        assert!(reopened.planning_run_state().unwrap().is_none());
        assert_eq!(reopened.workflow_state().unwrap().status, "APPROVED");

        let (_dir, empty) = registry();
        assert!(empty.planning_run_state().unwrap().is_none());
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
            evidence_needs: vec![],
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
            evidence_needs: vec![],
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
