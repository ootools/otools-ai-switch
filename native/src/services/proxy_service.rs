use crate::error::AppError;
use crate::models::settings::AppSettings;
use std::sync::{OnceLock, RwLock};

static PROXY: OnceLock<RwLock<Option<String>>> = OnceLock::new();

pub fn normalized_proxy_url(settings: &AppSettings) -> Option<&str> {
    settings
        .proxy_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn validate(settings: &AppSettings) -> Result<(), AppError> {
    if settings.proxy_enabled && normalized_proxy_url(settings).is_none() {
        return Err(AppError::Validation {
            code: "validation.proxy_url_required",
            message: "Proxy URL is required when the proxy is enabled".into(),
            details: None,
            recoverable: true,
        });
    }
    if settings.proxy_enabled {
        reqwest::Proxy::all(normalized_proxy_url(settings).unwrap()).map_err(|error| {
            AppError::Validation {
                code: "validation.proxy_url",
                message: "Proxy URL is invalid".into(),
                details: Some(error.to_string()),
                recoverable: true,
            }
        })?;
    }
    Ok(())
}

/// Plugin-local outbound configuration; never change the host environment.
pub fn apply(settings: &AppSettings) {
    *PROXY
        .get_or_init(Default::default)
        .write()
        .unwrap_or_else(|e| e.into_inner()) = if settings.proxy_enabled {
        normalized_proxy_url(settings).map(str::to_owned)
    } else {
        None
    };
}
pub fn apply_startup(settings: &AppSettings) {
    apply(settings);
}
pub fn configured_proxy() -> Option<String> {
    PROXY
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}
/// Historical name retained for callers; clears only plugin-owned memory.
pub fn clear_proxy_env() {
    *PROXY
        .get_or_init(Default::default)
        .write()
        .unwrap_or_else(|e| e.into_inner()) = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_proxy_settings_preserves_the_host_environment() {
        let keys = [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ];
        let before: Vec<_> = keys.iter().map(std::env::var_os).collect();
        let mut settings = AppSettings::defaults_for_data_dir("test".into());
        settings.proxy_enabled = true;
        settings.proxy_url = Some("http://127.0.0.1:7890".into());
        apply(&settings);
        assert_eq!(configured_proxy().as_deref(), Some("http://127.0.0.1:7890"));
        clear_proxy_env();
        assert_eq!(
            keys.iter().map(std::env::var_os).collect::<Vec<_>>(),
            before
        );
    }
}
