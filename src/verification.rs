use crate::{
    process_runner::{LocalProcessRunner, ProcessObservation},
    registry::Registry,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum VerificationCapability {
    BuildOnly,
    Unit,
    Integration,
    Browser,
}

impl VerificationCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BuildOnly => "BUILD_ONLY",
            Self::Unit => "UNIT",
            Self::Integration => "INTEGRATION",
            Self::Browser => "BROWSER",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationCommandKind {
    Build,
    Test,
    Lint,
    Typecheck,
    AggregateCi,
}

impl VerificationCommandKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Build => "BUILD",
            Self::Test => "TEST",
            Self::Lint => "LINT",
            Self::Typecheck => "TYPECHECK",
            Self::AggregateCi => "AGGREGATE_CI",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationCommand {
    pub id: String,
    pub kind: VerificationCommandKind,
    pub capability: VerificationCapability,
    pub argv: Vec<String>,
    pub source_paths: Vec<String>,
    pub config_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoveryStatus {
    Applicable,
    NotApplicable,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationProfile {
    pub status: DiscoveryStatus,
    pub capabilities: Vec<VerificationCapability>,
    pub commands: Vec<VerificationCommand>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationResult {
    TestPass,
    Fail,
    NotApplicable,
    Blocked,
}

impl VerificationResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TestPass => "TEST_PASS",
            Self::Fail => "FAIL",
            Self::NotApplicable => "NOT_APPLICABLE",
            Self::Blocked => "BLOCKED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEvidence {
    pub command_id: String,
    pub config_hash: String,
    pub argv: Vec<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub blocked_reason: Option<String>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationEvidence {
    pub profile: VerificationProfile,
    pub commands: Vec<CommandEvidence>,
    pub test_surface_changed: bool,
}

impl VerificationEvidence {
    pub fn derived_result(&self) -> VerificationResult {
        match self.profile.status {
            DiscoveryStatus::NotApplicable => return VerificationResult::NotApplicable,
            DiscoveryStatus::Blocked => return VerificationResult::Blocked,
            DiscoveryStatus::Applicable => {}
        }

        if self.profile.commands.is_empty()
            || self.commands.len() != self.profile.commands.len()
        {
            return VerificationResult::Blocked;
        }

        for (command, evidence) in self.profile.commands.iter().zip(&self.commands) {
            if command.id != evidence.command_id
                || command.config_hash != evidence.config_hash
                || command.argv != evidence.argv
            {
                return VerificationResult::Blocked;
            }
            if evidence.blocked_reason.is_some() {
                return VerificationResult::Blocked;
            }
            if evidence.timed_out {
                return VerificationResult::Fail;
            }
            if evidence.exit_code != Some(0) {
                return VerificationResult::Fail;
            }
        }

        VerificationResult::TestPass
    }
}

pub struct VerificationController<'a> {
    registry: &'a Registry,
    project_root: &'a Path,
    lease_owner: &'a str,
    runner: LocalProcessRunner,
}

impl<'a> VerificationController<'a> {
    pub fn new(registry: &'a Registry, project_root: &'a Path, lease_owner: &'a str) -> Self {
        Self {
            registry,
            project_root,
            lease_owner,
            runner: LocalProcessRunner::production(),
        }
    }

    pub fn with_runner(
        registry: &'a Registry,
        project_root: &'a Path,
        lease_owner: &'a str,
        runner: LocalProcessRunner,
    ) -> Self {
        Self {
            registry,
            project_root,
            lease_owner,
            runner,
        }
    }

    pub fn verify(
        &self,
        graph_version: i64,
        jobpack_id: &str,
        change_set_id: &str,
        changed_paths: &[String],
    ) -> Result<VerificationResult> {
        let profile = discover_profile(self.project_root)?;
        let mut evidence = VerificationEvidence {
            profile: profile.clone(),
            commands: Vec::new(),
            test_surface_changed: changed_paths.iter().any(|path| is_test_surface(path)),
        };

        if profile.status == DiscoveryStatus::Applicable {
            for command in &profile.commands {
                let before = ensure_command_config_current(self.project_root, command);
                if let Err(error) = before {
                    evidence.commands.push(CommandEvidence {
                        command_id: command.id.clone(),
                        config_hash: command.config_hash.clone(),
                        argv: command.argv.clone(),
                        exit_code: None,
                        duration_ms: 0,
                        timed_out: false,
                        blocked_reason: Some(format!(
                            "verification command became stale before execution: {error:#}"
                        )),
                        stdout: String::new(),
                        stderr: String::new(),
                    });
                    break;
                }

                let mut observation = match self.runner.run(
                    self.project_root,
                    command,
                    DEFAULT_TIMEOUT,
                ) {
                    Ok(observation) => observation,
                    Err(error) => ProcessObservation {
                        exit_code: None,
                        duration_ms: 0,
                        timed_out: false,
                        blocked_reason: Some(format!(
                            "verification process runner failed: {error:#}"
                        )),
                        stdout: String::new(),
                        stderr: String::new(),
                    },
                };

                if let Err(error) = ensure_command_config_current(self.project_root, command) {
                    observation.blocked_reason = Some(format!(
                        "verification config changed during execution: {error:#}"
                    ));
                }
                let blocked = observation.blocked_reason.is_some();
                evidence
                    .commands
                    .push(command_evidence(command, observation));
                if blocked {
                    break;
                }
            }
        }

        self.registry.record_verification_evidence(
            self.project_root,
            self.lease_owner,
            graph_version,
            jobpack_id,
            change_set_id,
            &evidence,
        )
    }
}

pub fn discover_profile(project_root: &Path) -> Result<VerificationProfile> {
    let root = project_root
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", project_root.display()))?;

    let ci = root.join("scripts/ci.sh");
    if ci.is_file() {
        let source_paths = verification_config_paths(&root, &["scripts/ci.sh"])?;
        let config_hash = hash_config_paths(&root, &source_paths)?;
        return Ok(VerificationProfile {
            status: DiscoveryStatus::Applicable,
            capabilities: vec![
                VerificationCapability::BuildOnly,
                VerificationCapability::Unit,
                VerificationCapability::Integration,
            ],
            commands: vec![VerificationCommand {
                id: "project-ci".into(),
                kind: VerificationCommandKind::AggregateCi,
                capability: VerificationCapability::Integration,
                argv: vec!["/bin/bash".into(), "scripts/ci.sh".into()],
                source_paths,
                config_hash,
            }],
            reason: None,
        });
    }

    let cargo = root.join("Cargo.toml");
    if cargo.is_file() {
        let source_paths = verification_config_paths(&root, &["Cargo.toml"])?;
        let config_hash = hash_config_paths(&root, &source_paths)?;
        return Ok(VerificationProfile {
            status: DiscoveryStatus::Applicable,
            capabilities: vec![
                VerificationCapability::BuildOnly,
                VerificationCapability::Unit,
            ],
            commands: vec![
                VerificationCommand {
                    id: "cargo-check".into(),
                    kind: VerificationCommandKind::Typecheck,
                    capability: VerificationCapability::BuildOnly,
                    argv: vec!["cargo".into(), "check".into()],
                    source_paths: source_paths.clone(),
                    config_hash: config_hash.clone(),
                },
                VerificationCommand {
                    id: "cargo-test".into(),
                    kind: VerificationCommandKind::Test,
                    capability: VerificationCapability::Unit,
                    argv: vec!["cargo".into(), "test".into()],
                    source_paths,
                    config_hash,
                },
            ],
            reason: None,
        });
    }

    let unsupported = ["package.json", "pyproject.toml", "go.mod"]
        .into_iter()
        .find(|name| root.join(name).is_file());
    if let Some(name) = unsupported {
        return Ok(VerificationProfile {
            status: DiscoveryStatus::Blocked,
            capabilities: Vec::new(),
            commands: Vec::new(),
            reason: Some(format!(
                "verification config {name} is detected but its safe deterministic runner is not implemented in this MVP"
            )),
        });
    }

    Ok(VerificationProfile {
        status: DiscoveryStatus::NotApplicable,
        capabilities: Vec::new(),
        commands: Vec::new(),
        reason: Some("no supported deterministic build/test configuration was discovered".into()),
    })
}

pub fn is_test_surface(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.starts_with("tests/")
        || normalized.starts_with("test/")
        || normalized.starts_with("scripts/")
        || normalized.starts_with(".github/")
        || normalized.ends_with("_test.rs")
        || normalized.contains(".test.")
        || normalized.contains(".spec.")
        || matches!(
            normalized.as_str(),
            "cargo.toml" | "cargo.lock" | "package.json" | "pyproject.toml" | "go.mod"
        )
}

fn command_evidence(
    command: &VerificationCommand,
    observation: ProcessObservation,
) -> CommandEvidence {
    CommandEvidence {
        command_id: command.id.clone(),
        config_hash: command.config_hash.clone(),
        argv: command.argv.clone(),
        exit_code: observation.exit_code,
        duration_ms: observation.duration_ms,
        timed_out: observation.timed_out,
        blocked_reason: observation.blocked_reason,
        stdout: observation.stdout,
        stderr: observation.stderr,
    }
}

fn verification_config_paths(root: &Path, primary: &[&str]) -> Result<Vec<String>> {
    let mut paths = BTreeSet::new();
    for path in primary {
        if root.join(path).is_file() {
            paths.insert((*path).to_owned());
        }
    }
    for path in ["Cargo.toml", "Cargo.lock"] {
        if root.join(path).is_file() {
            paths.insert(path.to_owned());
        }
    }
    Ok(paths.into_iter().collect())
}

pub fn hash_config_paths(root: &Path, paths: &[String]) -> Result<String> {
    let mut hasher = Sha256::new();
    for relative in paths {
        let path = safe_config_path(root, relative)?;
        let bytes = fs::read(&path)
            .with_context(|| format!("failed reading verification config {relative}"))?;
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hex_digest(hasher.finalize()))
}

fn ensure_command_config_current(root: &Path, command: &VerificationCommand) -> Result<()> {
    let current = hash_config_paths(root, &command.source_paths)?;
    if current != command.config_hash {
        bail!(
            "verification command {} is stale: config hash changed from {} to {}",
            command.id,
            command.config_hash,
            current
        );
    }
    Ok(())
}

fn safe_config_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let candidate = Path::new(relative);
    if candidate.is_absolute()
        || candidate
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!("verification config path escapes project: {relative}");
    }
    let path = root.join(candidate);
    let canonical = path
        .canonicalize()
        .with_context(|| format!("verification config is missing: {relative}"))?;
    if !canonical.starts_with(root) {
        bail!("verification config escaped project root: {relative}");
    }
    if !canonical.is_file() {
        bail!("verification config is not a regular file: {relative}");
    }
    Ok(canonical)
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    let digest = digest.as_ref();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn aggregate_ci_is_preferred_over_cargo_fallback() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("scripts")).unwrap();
        fs::write(dir.path().join("scripts/ci.sh"), "cargo test\n").unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname='x'\nversion='0.1.0'\n")
            .unwrap();

        let profile = discover_profile(dir.path()).unwrap();
        assert_eq!(profile.status, DiscoveryStatus::Applicable);
        assert_eq!(profile.commands.len(), 1);
        assert_eq!(profile.commands[0].id, "project-ci");
        assert_eq!(
            profile.commands[0].argv,
            vec!["/bin/bash".to_string(), "scripts/ci.sh".to_string()]
        );
    }

    #[test]
    fn cargo_fallback_is_deterministic() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname='x'\nversion='0.1.0'\n")
            .unwrap();

        let first = discover_profile(dir.path()).unwrap();
        let second = discover_profile(dir.path()).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first
                .commands
                .iter()
                .map(|command| command.id.as_str())
                .collect::<Vec<_>>(),
            vec!["cargo-check", "cargo-test"]
        );
    }

    #[test]
    fn unsupported_stack_is_blocked_not_guessed() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        let profile = discover_profile(dir.path()).unwrap();
        assert_eq!(profile.status, DiscoveryStatus::Blocked);
        assert!(profile.commands.is_empty());
    }

    #[test]
    fn config_hash_changes_when_manifest_changes() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "one").unwrap();
        let profile = discover_profile(dir.path()).unwrap();
        let command = &profile.commands[0];
        fs::write(dir.path().join("Cargo.toml"), "two").unwrap();
        assert!(ensure_command_config_current(dir.path(), command).is_err());
    }

    #[test]
    fn evidence_result_cannot_claim_pass_without_matching_observations() {
        let profile = VerificationProfile {
            status: DiscoveryStatus::Applicable,
            capabilities: vec![VerificationCapability::Unit],
            commands: vec![VerificationCommand {
                id: "test".into(),
                kind: VerificationCommandKind::Test,
                capability: VerificationCapability::Unit,
                argv: vec!["cargo".into(), "test".into()],
                source_paths: vec!["Cargo.toml".into()],
                config_hash: "hash".into(),
            }],
            reason: None,
        };
        let missing = VerificationEvidence {
            profile: profile.clone(),
            commands: vec![],
            test_surface_changed: false,
        };
        assert_eq!(missing.derived_result(), VerificationResult::Blocked);

        let failed = VerificationEvidence {
            profile,
            commands: vec![CommandEvidence {
                command_id: "test".into(),
                config_hash: "hash".into(),
                argv: vec!["cargo".into(), "test".into()],
                exit_code: Some(1),
                duration_ms: 1,
                timed_out: false,
                blocked_reason: None,
                stdout: String::new(),
                stderr: String::new(),
            }],
            test_surface_changed: false,
        };
        assert_eq!(failed.derived_result(), VerificationResult::Fail);
    }
}
