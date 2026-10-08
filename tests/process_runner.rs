#[cfg(not(target_os = "macos"))]
use gsa_local::{
    process_runner::LocalProcessRunner,
    verification::{VerificationCapability, VerificationCommand, VerificationCommandKind},
};
#[cfg(not(target_os = "macos"))]
use std::time::Duration;
#[cfg(not(target_os = "macos"))]
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

#[cfg(target_os = "macos")]
#[test]
fn macos_production_runner_must_execute_real_successfully_sandboxed_command() {
    use gsa_local::{
        process_runner::LocalProcessRunner,
        verification::{VerificationCapability, VerificationCommand, VerificationCommandKind},
    };
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();
    let runner = LocalProcessRunner::production();
    assert!(
        runner.is_available(),
        "macOS CI requires a functional sandbox-exec backend; a blocked runner cannot prove success"
    );
    let command = VerificationCommand {
        id: "real-sandbox-success".into(),
        kind: VerificationCommandKind::Test,
        capability: VerificationCapability::Unit,
        argv: vec!["/bin/echo".into(), "gsa-sandbox-observed".into()],
        source_paths: vec![],
        config_hash: "positive-control".into(),
    };
    let result = runner
        .run(dir.path(), &command, Duration::from_secs(3))
        .expect("production runner should return observed process evidence");

    assert_eq!(
        result.blocked_reason,
        None,
        "sandbox must not block a valid command: {result:?}"
    );
    assert_eq!(
        result.exit_code,
        Some(0),
        "real sandbox command did not succeed: {result:?}"
    );
    assert_eq!(result.stdout.trim(), "gsa-sandbox-observed");
    assert!(!result.timed_out);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_production_runner_cannot_falsely_pass_missing_executable() {
    use gsa_local::{
        process_runner::LocalProcessRunner,
        verification::{VerificationCapability, VerificationCommand, VerificationCommandKind},
    };
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();
    let runner = LocalProcessRunner::production();
    let command = VerificationCommand {
        id: "must-never-succeed".into(),
        kind: VerificationCommandKind::Test,
        capability: VerificationCapability::Unit,
        argv: vec!["/gsa-local-intentionally-missing-executable".into()],
        source_paths: vec![],
        config_hash: "negative-control".into(),
    };
    let result = runner
        .run(dir.path(), &command, Duration::from_secs(3))
        .expect("production runner must return a structured observation");

    // On a macOS runner with sandbox-exec, the requested executable must fail.
    // On a host without the sandbox backend, the result must be an explicit BLOCKED.
    // Neither condition is allowed to masquerade as a successful verification.
    assert_ne!(result.exit_code, Some(0));
    assert!(
        result.blocked_reason.is_some() || result.exit_code.is_some(),
        "negative control needs an explicit failure or blocked reason: {result:?}"
    );
    assert!(!result.timed_out, "missing executable must not hang");
}
