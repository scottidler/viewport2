use std::path::PathBuf;

use eyre::{Context, Result};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// Resolve the effective log level from CLI flag, env var, or default.
pub fn resolve_log_level(cli_level: Option<&str>) -> String {
    if let Some(level) = cli_level {
        return level.to_string();
    }
    if let Ok(level) = std::env::var("VIEWPORT2_LOG") {
        return level;
    }
    "info".to_string()
}

/// Initialize tracing with daily rolling file appender at XDG data directory.
/// Returns a guard that must be held for the lifetime of the program.
pub fn setup_tracing(level: &str) -> Result<WorkerGuard> {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("viewport2")
        .join("logs");

    std::fs::create_dir_all(&log_dir).context("Failed to create log directory")?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, "viewport2.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_target(true)
        .with_thread_ids(false)
        .with_ansi(false)
        .init();

    tracing::info!(log_dir = %log_dir.display(), level, "tracing initialized");
    Ok(guard)
}
