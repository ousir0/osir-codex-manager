//! Explicit local activation smoke: run only when switching this user's Codex
//! to their saved OpenCodex routes is intended. Never prints credentials.
use osir_codex_manager_lib::app::opencodex;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() != Some("--activate-saved") {
        return Err("pass --activate-saved to activate the local saved routes".into());
    }
    let status = opencodex::activate_saved()?;
    println!(
        "enabled={} service={} models={} connection={}",
        status.enabled, status.service_state, status.model_count, status.connection_status
    );
    if !status.enabled || status.service_state != "ready" || status.model_count == 0 {
        return Err("activation did not reach ready".into());
    }
    Ok(())
}
