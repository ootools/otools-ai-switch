//! One-click onboarding of subscriptions the user already logged into with
//! their own CLI.
//!
//! Interactive browser/device login is implemented for Codex only (see
//! `codex_oauth`); the other vendors' CLIs own their login UI and write a
//! credentials file. This module detects those files and imports them through
//! the same official-account path, so refresh/failover behave identically to a
//! Codex subscription. It never refreshes a token at the source — copying a
//! still-valid snapshot is the only supported action.

use crate::error::AppError;
use crate::services::codex_oauth::credential::import_account;
use crate::services::codex_oauth::ImportedCodexAccount;
use crate::services::cpa_import_service::parse_cpa_text;
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionVendor {
    Claude,
    Copilot,
    Gemini,
    Grok,
}

impl SubscriptionVendor {
    fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Copilot => "copilot",
            Self::Gemini => "gemini",
            Self::Grok => "grok",
        }
    }

    /// Relative to the user's home directory. The first existing file wins.
    fn candidates(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[".claude/.credentials.json", ".claude/credentials.json"],
            Self::Copilot => &[".copilot/auth.json", ".config/github-copilot/apps.json"],
            Self::Gemini => &[".gemini/oauth_creds.json"],
            Self::Grok => &[".grok/auth.json", ".xai/auth.json"],
        }
    }

    /// The `type` hint CPA parsing needs for a bare CLI credentials file.
    /// Claude's native `claudeAiOauth` shape is already unambiguous.
    fn type_hint(self) -> Option<&'static str> {
        match self {
            Self::Claude => None,
            // Copilot has no PlatformId in this build. Storing it under the
            // unified pool identity with an explicit auth_kind keeps it
            // importable without pretending a new routable provider exists.
            Self::Copilot => Some("codex"),
            Self::Gemini => Some("gemini"),
            Self::Grok => Some("grok"),
        }
    }
}

fn home_dir() -> Result<PathBuf, AppError> {
    BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| AppError::Validation {
            code: "validation.home_dir_unavailable",
            message: "Could not resolve the user home directory".to_string(),
            details: None,
            recoverable: true,
        })
}

/// The first existing credentials file for a vendor, without reading it.
pub fn detect_subscription_login(vendor: SubscriptionVendor) -> Result<Option<String>, AppError> {
    let home = home_dir()?;
    Ok(vendor
        .candidates()
        .iter()
        .map(|relative| home.join(relative))
        .find(|path| path.is_file())
        .map(|path| path.display().to_string()))
}

/// Normalize a raw CLI credentials file into the CPA shape the shared parser
/// already understands. Existing explicit type/vendor fields are preserved.
fn typed_payload(vendor: SubscriptionVendor, text: &str) -> Result<String, AppError> {
    let Some(hint) = vendor.type_hint() else {
        return Ok(text.to_string());
    };
    let mut value: Value = serde_json::from_str(text).map_err(|error| AppError::Validation {
        code: "validation.subscription_login_invalid",
        message: format!("Login file is not valid JSON: {error}"),
        details: None,
        recoverable: true,
    })?;
    if let Value::Object(object) = &mut value {
        object
            .entry("type")
            .or_insert_with(|| json!(hint));
    }
    serde_json::to_string(&value).map_err(|error| AppError::Validation {
        code: "validation.subscription_login_invalid",
        message: error.to_string(),
        details: None,
        recoverable: true,
    })
}

/// Import the detected login file as one or more official accounts.
pub async fn import_subscription_login_core(
    pool: &SqlitePool,
    vendor: SubscriptionVendor,
    batch_name: &str,
    path: Option<&str>,
) -> Result<Vec<ImportedCodexAccount>, AppError> {
    if batch_name.trim().is_empty() {
        return Err(AppError::Validation {
            code: "validation.batch_name_required",
            message: "Batch name is required".to_string(),
            details: None,
            recoverable: true,
        });
    }
    let path = match path {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(detect_subscription_login(vendor)?.ok_or_else(|| {
            AppError::Validation {
                code: "validation.subscription_login_not_found",
                message: format!(
                    "No {} login file found. Log in with the vendor CLI first.",
                    vendor.as_str()
                ),
                details: None,
                recoverable: true,
            }
        })?),
    };
    // Read-only: a shared login file must never be rewritten by this plugin.
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(|error| AppError::Filesystem {
            code: "filesystem.subscription_login_unreadable",
            message: format!("Could not read {}: {error}", path.display()),
            details: None,
            recoverable: true,
        })?;
    let typed = typed_payload(vendor, &text)?;
    let credentials = parse_cpa_text(&typed)?;
    let mut imported = Vec::new();
    for mut credential in credentials {
        if vendor == SubscriptionVendor::Copilot {
            if let Ok(mut config) = serde_json::from_str::<Value>(&credential.config_json) {
                if let Some(object) = config.as_object_mut() {
                    object.insert("auth_kind".to_string(), json!("copilot"));
                    object.insert(
                        "token_endpoint".to_string(),
                        json!("https://github.com/login/oauth/access_token"),
                    );
                }
                credential.config_json = config.to_string();
            }
        }
        imported.push(import_account(pool, batch_name, credential).await?);
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_hint_is_injected_without_overwriting_an_explicit_one() {
        let raw = r#"{"access_token":"a","refresh_token":"r"}"#;
        let typed = typed_payload(SubscriptionVendor::Gemini, raw).expect("payload");
        let value: Value = serde_json::from_str(&typed).expect("json");
        assert_eq!(value["type"], "gemini");

        let explicit = r#"{"type":"grok","access_token":"a"}"#;
        let typed = typed_payload(SubscriptionVendor::Gemini, explicit).expect("payload");
        let value: Value = serde_json::from_str(&typed).expect("json");
        assert_eq!(value["type"], "grok");
    }

    #[test]
    fn claude_native_shape_is_left_untouched() {
        let raw = r#"{"claudeAiOauth":{"accessToken":"a"}}"#;
        let typed = typed_payload(SubscriptionVendor::Claude, raw).expect("payload");
        assert_eq!(typed, raw);
    }

    #[test]
    fn candidates_are_relative_to_home() {
        for vendor in [
            SubscriptionVendor::Claude,
            SubscriptionVendor::Copilot,
            SubscriptionVendor::Gemini,
            SubscriptionVendor::Grok,
        ] {
            assert!(vendor
                .candidates()
                .iter()
                .all(|path| !Path::new(path).is_absolute()));
        }
    }
}
