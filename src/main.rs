use anyhow::Result;
use gsa_local::app::App;

#[cfg(target_os = "macos")]
fn prevent_idle_sleep() -> Result<std::process::Child> {
    use anyhow::Context;
    use std::process::{Command, Stdio};

    // -i prevents idle system sleep on battery or AC.
    // -w ties the assertion to this GSA process, including abnormal exits.
    // Closing the laptop lid or explicit user sleep can still suspend macOS.
    let pid = std::process::id();
    Command::new("/usr/bin/caffeinate")
        .args(["-i", "-w", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("GSA_SLEEP_GUARD_UNAVAILABLE: failed to start macOS caffeinate; refusing unprotected long-running session")
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut app = App::new()?;
    #[cfg(target_os = "macos")]
    let _sleep_guard = prevent_idle_sleep()?;
    app.run().await
}
