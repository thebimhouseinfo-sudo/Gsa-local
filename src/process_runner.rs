use crate::verification::VerificationCommand;
use anyhow::{Context, Result};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessObservation {
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub blocked_reason: Option<String>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
enum SandboxBackend {
    MacOs(PathBuf),
    Unavailable(String),
    #[cfg(test)]
    DirectTest,
}

#[derive(Debug, Clone)]
pub struct LocalProcessRunner {
    backend: SandboxBackend,
}

impl LocalProcessRunner {
    pub fn production() -> Self {
        #[cfg(target_os = "macos")]
        {
            let path = PathBuf::from("/usr/bin/sandbox-exec");
            if path.is_file() {
                return Self {
                    backend: SandboxBackend::MacOs(path),
                };
            }
            return Self {
                backend: SandboxBackend::Unavailable(
                    "macOS sandbox-exec is unavailable; refusing unsandboxed verification".into(),
                ),
            };
        }

        #[cfg(not(target_os = "macos"))]
        {
            Self {
                backend: SandboxBackend::Unavailable(
                    "project-bound verification sandbox is implemented for macOS only".into(),
                ),
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn unavailable_for_test(reason: &str) -> Self {
        Self {
            backend: SandboxBackend::Unavailable(reason.to_owned()),
        }
    }

    #[cfg(test)]
    pub(crate) fn direct_for_test() -> Self {
        Self {
            backend: SandboxBackend::DirectTest,
        }
    }

    pub fn run(
        &self,
        project_root: &Path,
        command: &VerificationCommand,
        timeout: Duration,
    ) -> Result<ProcessObservation> {
        let root = project_root
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", project_root.display()))?;
        if let SandboxBackend::Unavailable(reason) = &self.backend {
            return Ok(ProcessObservation {
                exit_code: None,
                duration_ms: 0,
                timed_out: false,
                blocked_reason: Some(reason.clone()),
                stdout: String::new(),
                stderr: String::new(),
            });
        }

        if command.argv.is_empty() {
            return Ok(ProcessObservation {
                exit_code: None,
                duration_ms: 0,
                timed_out: false,
                blocked_reason: Some("verification command argv is empty".into()),
                stdout: String::new(),
                stderr: String::new(),
            });
        }

        let runtime_dir = root.join(".gsa-local/ci");
        let temp_dir = runtime_dir.join("tmp");
        let target_dir = runtime_dir.join("target");
        fs::create_dir_all(&temp_dir)?;
        fs::create_dir_all(&target_dir)?;

        let mut process = match &self.backend {
            SandboxBackend::MacOs(sandbox_exec) => {
                let profile = sandbox_profile(&runtime_dir, &temp_dir)?;
                let mut process = Command::new(sandbox_exec);
                process
                    .arg("-p")
                    .arg(profile)
                    .arg(&command.argv[0])
                    .args(&command.argv[1..]);
                process
            }
            SandboxBackend::Unavailable(_) => unreachable!(),
            #[cfg(test)]
            SandboxBackend::DirectTest => {
                let mut process = Command::new(&command.argv[0]);
                process.args(&command.argv[1..]);
                process
            }
        };
        process
            .current_dir(&root)
            .env("CARGO_TARGET_DIR", &target_dir)
            .env("CARGO_NET_OFFLINE", "true")
            .env("TMPDIR", &temp_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let started = Instant::now();
        let mut child = match process.spawn() {
            Ok(child) => child,
            Err(error) => {
                return Ok(ProcessObservation {
                    exit_code: None,
                    duration_ms: started.elapsed().as_millis() as u64,
                    timed_out: false,
                    blocked_reason: Some(format!(
                        "failed to start sandboxed verification: {error}"
                    )),
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_thread = thread::spawn(move || read_bounded(stdout));
        let stderr_thread = thread::spawn(move || read_bounded(stderr));

        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if started.elapsed() >= timeout {
                timed_out = true;
                let _ = child.kill();
                let status = child.wait().ok();
                break status;
            }
            thread::sleep(POLL_INTERVAL);
        };

        let stdout = stdout_thread.join().unwrap_or_default();
        let stderr = stderr_thread.join().unwrap_or_default();

        Ok(ProcessObservation {
            exit_code: status.and_then(|status| status.code()),
            duration_ms: started.elapsed().as_millis() as u64,
            timed_out,
            blocked_reason: None,
            stdout,
            stderr,
        })
    }
}

fn sandbox_profile(runtime_dir: &Path, temp_dir: &Path) -> Result<String> {
    let runtime = escape_profile_path(
        runtime_dir
            .canonicalize()
            .unwrap_or_else(|_| runtime_dir.to_path_buf())
            .to_string_lossy()
            .as_ref(),
    );
    let temp = escape_profile_path(
        temp_dir
            .canonicalize()
            .unwrap_or_else(|_| temp_dir.to_path_buf())
            .to_string_lossy()
            .as_ref(),
    );

    Ok(format!(
        "(version 1)\n(allow default)\n(deny network*)\n(deny file-write*)\n(allow file-write* (subpath \"{runtime}\") (subpath \"{temp}\"))\n"
    ))
}

fn escape_profile_path(path: &str) -> String {
    path.replace('\\', "\\\\").replace('"', "\\\"")
}

fn read_bounded<R>(reader: Option<R>) -> String
where
    R: Read,
{
    let Some(mut reader) = reader else {
        return String::new();
    };
    let mut bytes = Vec::new();
    let _ = reader
        .by_ref()
        .take((MAX_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes);
    let truncated = bytes.len() > MAX_OUTPUT_BYTES;
    if truncated {
        bytes.truncate(MAX_OUTPUT_BYTES);
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        text.push_str("\n[output truncated]");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_backend_blocks_instead_of_running_unsandboxed() {
        let runner = LocalProcessRunner::unavailable_for_test("sandbox missing");
        let command = VerificationCommand {
            id: "x".into(),
            kind: crate::verification::VerificationCommandKind::Test,
            capability: crate::verification::VerificationCapability::Unit,
            argv: vec!["false".into()],
            source_paths: vec![],
            config_hash: "x".into(),
        };
        let dir = tempfile::tempdir().unwrap();
        let result = runner
            .run(dir.path(), &command, Duration::from_secs(1))
            .unwrap();
        assert_eq!(result.blocked_reason.as_deref(), Some("sandbox missing"));
        assert_eq!(result.exit_code, None);
    }

    #[cfg(unix)]
    #[test]
    fn direct_test_seam_observes_exit_zero_nonzero_and_timeout() {
        let runner = LocalProcessRunner::direct_for_test();
        let dir = tempfile::tempdir().unwrap();
        let command = |id: &str, argv: Vec<String>| VerificationCommand {
            id: id.into(),
            kind: crate::verification::VerificationCommandKind::Test,
            capability: crate::verification::VerificationCapability::Unit,
            argv,
            source_paths: vec![],
            config_hash: "x".into(),
        };

        let passed = runner
            .run(
                dir.path(),
                &command("pass", vec!["/bin/true".into()]),
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(passed.exit_code, Some(0));
        assert!(!passed.timed_out);

        let failed = runner
            .run(
                dir.path(),
                &command("fail", vec!["/bin/false".into()]),
                Duration::from_secs(1),
            )
            .unwrap();
        assert_ne!(failed.exit_code, Some(0));
        assert!(!failed.timed_out);

        let timed_out = runner
            .run(
                dir.path(),
                &command("timeout", vec!["/bin/sleep".into(), "1".into()]),
                Duration::from_millis(50),
            )
            .unwrap();
        assert!(timed_out.timed_out);
    }

    #[test]
    fn bounded_reader_marks_truncation() {
        let input = vec![b'x'; MAX_OUTPUT_BYTES + 10];
        let output = read_bounded(Some(input.as_slice()));
        assert!(output.ends_with("[output truncated]"));
    }
}
