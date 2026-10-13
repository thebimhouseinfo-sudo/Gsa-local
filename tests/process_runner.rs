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
fn observed_sandbox_command(
    root: &std::path::Path,
    id: &str,
    argv: Vec<String>,
) -> gsa_local::process_runner::ProcessObservation {
    use gsa_local::{
        process_runner::{LocalProcessRunner, ProcessObservation},
        verification::{VerificationCapability, VerificationCommand, VerificationCommandKind},
    };
    use std::{process::Command, time::Duration};
    if std::env::var("GSA_VERIFICATION_SANDBOX").as_deref() == Ok("1") {
        // Already inside the verifier's macOS sandbox: do not nest sandbox-exec.
        // Execute a real child and verify both positive and negative controls.
        let output = Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(root)
            .output();
        return match output {
            Ok(output) => ProcessObservation {
                exit_code: output.status.code(),
                duration_ms: 0,
                timed_out: false,
                blocked_reason: None,
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            },
            Err(error) => ProcessObservation {
                exit_code: None,
                duration_ms: 0,
                timed_out: false,
                blocked_reason: Some(error.to_string()),
                stdout: String::new(),
                stderr: String::new(),
            },
        };
    }
    LocalProcessRunner::production()
        .run(
            root,
            &VerificationCommand {
                id: id.into(),
                kind: VerificationCommandKind::Test,
                capability: VerificationCapability::Unit,
                argv,
                source_paths: vec![],
                config_hash: "macos-actual-control".into(),
            },
            Duration::from_secs(5),
        )
        .unwrap()
}

#[cfg(target_os = "macos")]
#[test]
fn macos_production_runner_must_execute_real_successfully_sandboxed_command() {
    let dir = tempfile::tempdir().unwrap();
    let result = observed_sandbox_command(
        dir.path(),
        "real-sandbox-success",
        vec!["/bin/echo".into(), "gsa-sandbox-observed".into()],
    );

    assert_eq!(
        result.blocked_reason, None,
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
    let dir = tempfile::tempdir().unwrap();
    let result = observed_sandbox_command(
        dir.path(),
        "must-never-succeed",
        vec!["/gsa-local-intentionally-missing-executable".into()],
    );

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

#[cfg(target_os = "macos")]
#[test]
fn macos_verifier_allows_dev_null_but_denies_outside_writes() {
    let root = tempfile::tempdir().unwrap();
    let run = |id: &str, script: String| {
        observed_sandbox_command(root.path(), id, vec!["/bin/sh".into(), "-c".into(), script])
    };
    let sink = run("dev-null-control", "printf probe >/dev/null".into());
    assert_eq!(sink.exit_code, Some(0), "{sink:?}");
    let denied = if std::env::var("GSA_VERIFICATION_SANDBOX").as_deref() == Ok("1") {
        std::env::current_dir()
            .unwrap()
            .join("gsa-verifier-denied-control.txt")
    } else {
        tempfile::tempdir().unwrap().keep().join("unauthorized.txt")
    };
    let escape = run(
        "outside-write-negative-control",
        format!("printf forbidden > {}", denied.display()),
    );
    assert_ne!(escape.exit_code, Some(0), "{escape:?}");
    assert!(!denied.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_nested_context_runs_controls_without_second_sandbox() {
    if std::env::var("GSA_VERIFICATION_SANDBOX").as_deref() == Ok("1") {
        return;
    }
    use gsa_local::{
        process_runner::LocalProcessRunner,
        verification::{VerificationCapability, VerificationCommand, VerificationCommandKind},
    };
    let root = tempfile::tempdir().unwrap();
    let executable = std::env::current_exe().unwrap();
    let result = LocalProcessRunner::production()
        .run(
            root.path(),
            &VerificationCommand {
                id: "nested-context-regression".into(),
                kind: VerificationCommandKind::Test,
                capability: VerificationCapability::Integration,
                argv: vec![
                    executable.to_string_lossy().into_owned(),
                    "--exact".into(),
                    "macos_verifier_allows_dev_null_but_denies_outside_writes".into(),
                    "--nocapture".into(),
                ],
                source_paths: vec![],
                config_hash: "macos-nested-regression".into(),
            },
            std::time::Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(result.exit_code, Some(0), "{result:?}");
    assert!(result.stdout.contains("1 passed"), "{result:?}");
}
