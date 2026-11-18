//! Reads provider and login entries out of Magpie's own configuration.
//!
//! Placeholder dispatch target: `resolve_source_dir` returns the default config
//! directory and `prepare_entries` contributes nothing yet. Task 5 fills in the
//! real reading and extraction (`providers.json` API providers + `logins.json`
//! subscription accounts).

use crate::error::AppError;
use crate::services::external_client_import_service::PreparedExternalEntry;
use std::path::{Path, PathBuf};

fn home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|base| base.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Magpie's config directory; the Go app uses `~/.config/magpie` on every
/// platform, honouring `XDG_CONFIG_HOME`.
pub fn default_config_dir() -> PathBuf {
    match std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        Some(value) => PathBuf::from(value).join("magpie"),
        None => home_dir().join(".config").join("magpie"),
    }
}

pub fn resolve_source_dir(source_path: Option<&str>) -> Result<PathBuf, AppError> {
    match source_path.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(default_config_dir()),
    }
}

pub async fn prepare_entries(_dir: &Path) -> Result<Vec<PreparedExternalEntry>, AppError> {
    Ok(Vec::new())
}
