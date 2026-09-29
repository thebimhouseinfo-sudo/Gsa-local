use gsa_local::{
    process_runner::LocalProcessRunner,
    verification::{
        VerificationCapability, VerificationCommand, VerificationCommandKind,
    },
};
use std::time::Duration;
use tempfile::tempdir;

#[cfg(not(target_os = "macos"))]
#[test]
fn production_runner_blocks_when_project_sandbox_backend_is_unavailable() {
    let dir = tempdir().unwrap();
    let runner = LocalProcessRunner::production();
    let command = VerificationCommand {
        id: "never-run".into(),
        kind: VerificationCommandKind::Test,
        capability: VerificationCapability::Unit,
        argv: vec!["/bin/true".into()],
        source_paths: vec![],
        config_hash: "hash".into(),
    };
    let result = runner
        .run(dir.path(), &command, Duration::from_secs(1))
        .unwrap();
    assert_eq!(result.exit_code, None);
    assert!(result.blocked_reason.is_some());
}
