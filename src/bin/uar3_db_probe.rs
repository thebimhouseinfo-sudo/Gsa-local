use anyhow::{Context, Result};
use gsa_local::registry::Registry;
use std::path::PathBuf;

fn main() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("usage: cargo run --bin uar3-db-probe -- <copied-gsa.db>")?;
    if !path.is_file() {
        anyhow::bail!("database snapshot does not exist: {}", path.display());
    }
    let registry = Registry::open_at(&path)?;
    match registry.planning_run_state()? {
        Some(state) => {
            println!("UAR3_DB_STAGE={}", state.stage);
            println!("UAR3_DB_REVISION={:?}", state.current_revision);
            println!("UAR3_DB_LEGACY_SNAPSHOT={}", state.legacy_snapshot_json.is_some());
        }
        None => println!("UAR3_DB_STAGE=NO_ACTIVE_PLANNING"),
    }
    println!("UAR3_DB_PROBE_PASS=1");
    Ok(())
}
