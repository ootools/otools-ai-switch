//! Reads provider entries out of Codex++'s own configuration.
//!
//! Placeholder dispatch target: `resolve_source_dir` returns the default state
//! directory and `prepare_entries` contributes nothing yet. Task 4 fills in the
//! real reading and extraction (`settings.json` relayProfiles + `gateway.json`
//! + `claude-providers.json`).

use crate::error::AppError;
use crate::services::external_client_import_service::PreparedExternalEntry;
use std::path::{Path, PathBuf};

fn home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|base| base.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Codex++'s state directory (its legacy name predates the manager app).
pub fn default_state_dir() -> PathBuf {
    match std::env::var("CODEX_PLUS_STATE_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        Some(value) => PathBuf::from(value),
        None => home_dir().join(".codex-session-delete"),
    }
}

pub fn resolve_source_dir(source_path: Option<&str>) -> Result<PathBuf, AppError> {
    match source_path.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(default_state_dir()),
    }
}

pub async fn prepare_entries(_dir: &Path) -> Result<Vec<PreparedExternalEntry>, AppError> {
    Ok(Vec::new())
}
