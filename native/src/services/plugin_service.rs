//! Install/list/enable declarative plugin packs.
//!
//! The service never executes anything from a pack: a pack is one JSON file
//! under `<data_dir>/plugins/<id>/plugin.json` plus a tiny `install.json`
//! recording where it came from. Enabled ids live in the settings blob.

use crate::error::AppError;
use crate::models::plugin::{
    self, PluginDetail, PluginManifest, PluginPreset, PluginSummary, MANIFEST_MAX_BYTES,
};
use crate::models::settings::AppSettings;
use crate::paths::AppPaths;
use crate::services::http_client::{build_outbound_http_client_with_timeouts, OutboundTimeouts};
use crate::services::settings_service::SettingsService;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Serializes load-modify-save of `plugins_enabled_json`. A `tokio` mutex keeps
/// the future `Send`, which matters because command dispatch runs on the runtime.
static SETTINGS_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const PLUGIN_FILE: &str = "plugin.json";
const INSTALL_FILE: &str = "install.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallInfo {
    source: String,
}

impl Default for InstallInfo {
    fn default() -> Self {
        Self {
            source: "local".to_string(),
        }
    }
}

fn validation(code: &'static str, message: String) -> AppError {
    AppError::Validation {
        code,
        message,
        details: None,
        recoverable: true,
    }
}

fn filesystem(code: &'static str, message: String) -> AppError {
    AppError::Filesystem {
        code,
        message,
        details: None,
        recoverable: true,
    }
}

fn pack_dir(paths: &AppPaths, id: &str) -> Result<PathBuf, AppError> {
    plugin::validate_pack_id(id)?;
    Ok(paths.plugins_dir.join(id))
}

async fn read_install_info(dir: &Path) -> InstallInfo {
    let path = dir.join(INSTALL_FILE);
    match tokio::fs::read_to_string(&path).await {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => InstallInfo::default(),
    }
}

async fn read_pack(paths: &AppPaths, id: &str) -> Result<(PluginManifest, InstallInfo), AppError> {
    let dir = pack_dir(paths, id)?;
    if !dir.is_dir() {
        return Err(filesystem(
            "plugin.not_installed",
            format!("插件 {id} 未安装。"),
        ));
    }
    let bytes = tokio::fs::read(dir.join(PLUGIN_FILE))
        .await
        .map_err(|error| filesystem("plugin.read_failed", format!("读取插件 {id} 失败：{error}")))?;
    let manifest = plugin::parse_manifest(&bytes)?;
    Ok((manifest, read_install_info(&dir).await))
}

fn enabled_ids(settings: &AppSettings) -> BTreeSet<String> {
    settings
        .plugins_enabled_json
        .as_deref()
        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .map(|values| values.into_iter().collect())
        .unwrap_or_default()
}

async fn mutate_enabled<F>(paths: &AppPaths, mutate: F) -> Result<(), AppError>
where
    F: FnOnce(&mut BTreeSet<String>),
{
    let _guard = SETTINGS_LOCK.lock().await;
    let mut settings = SettingsService::load(paths).await?;
    let mut ids = enabled_ids(&settings);
    mutate(&mut ids);
    settings.plugins_enabled_json = Some(
        serde_json::to_string(&ids.into_iter().collect::<Vec<_>>())
            .map_err(|error| validation("plugin.settings_serialize_failed", error.to_string()))?,
    );
    SettingsService::save(paths, &settings).await
}

async fn install_bytes(
    paths: &AppPaths,
    bytes: &[u8],
    source: &str,
    middleware_js: Option<&[u8]>,
) -> Result<PluginSummary, AppError> {
    let manifest = plugin::parse_manifest(bytes)?;
    let dir = pack_dir(paths, &manifest.id)?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|error| filesystem("plugin.dir_create_failed", error.to_string()))?;
    tokio::fs::write(dir.join(PLUGIN_FILE), bytes)
        .await
        .map_err(|error| filesystem("plugin.write_failed", error.to_string()))?;
    if let Some(js) = middleware_js {
        if js.len() > 256 * 1024 {
            return Err(validation("plugin.script_too_large", "中间件脚本不能超过 256 KiB。".into()));
        }
        tokio::fs::write(dir.join("middleware.js"), js)
            .await
            .map_err(|error| filesystem("plugin.write_failed", error.to_string()))?;
    }
    let info = InstallInfo {
        source: source.to_string(),
    };
    let info_bytes = serde_json::to_vec(&info)
        .map_err(|error| validation("plugin.install_info_failed", error.to_string()))?;
    tokio::fs::write(dir.join(INSTALL_FILE), info_bytes)
        .await
        .map_err(|error| filesystem("plugin.write_failed", error.to_string()))?;
    mutate_enabled(paths, |ids| {
        ids.insert(manifest.id.clone());
    })
    .await?;
    crate::services::plugin_middleware::mark_dirty();
    Ok(manifest.summary(true, source))
}

pub struct PluginService;

impl PluginService {
    pub async fn list(paths: &AppPaths) -> Result<Vec<PluginSummary>, AppError> {
        let settings = SettingsService::load(paths).await?;
        let enabled = enabled_ids(&settings);
        tokio::fs::create_dir_all(&paths.plugins_dir)
            .await
            .map_err(|error| filesystem("plugin.dir_create_failed", error.to_string()))?;
        let mut entries = tokio::fs::read_dir(&paths.plugins_dir)
            .await
            .map_err(|error| filesystem("plugin.read_failed", error.to_string()))?;
        let mut summaries = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| filesystem("plugin.read_failed", error.to_string()))?
        {
            if !entry.path().is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            // A malformed or unsupported pack is skipped, never fatal: one bad
            // directory cannot make the whole settings page unusable.
            let (manifest, install_info) = match read_pack(paths, &id).await {
                Ok(pack) => pack,
                Err(_) => continue,
            };
            summaries.push(manifest.summary(enabled.contains(&id), &install_info.source));
        }
        summaries.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(summaries)
    }

    pub async fn get_detail(paths: &AppPaths, id: &str) -> Result<PluginDetail, AppError> {
        let (manifest, install_info) = read_pack(paths, id).await?;
        let settings = SettingsService::load(paths).await?;
        Ok(PluginDetail {
            manifest,
            enabled: enabled_ids(&settings).contains(id),
            source: install_info.source,
            path: pack_dir(paths, id)?.display().to_string(),
        })
    }

    /// Install from a directory containing `plugin.json`. Only that file is
    /// read; no script, binary or nested folder is ever copied or executed.
    pub async fn install_folder(paths: &AppPaths, folder: &str) -> Result<PluginSummary, AppError> {
        if folder.trim().is_empty() {
            return Err(validation(
                "plugin.install_path_required",
                "请选择插件目录。".into(),
            ));
        }
        let source_path = PathBuf::from(folder);
        let canonical = tokio::fs::canonicalize(&source_path)
            .await
            .map_err(|error| filesystem("plugin.source_not_found", format!("找不到插件目录：{error}")))?;
        let manifest_path = canonical.join(PLUGIN_FILE);
        let bytes = tokio::fs::read(&manifest_path).await.map_err(|error| {
            filesystem(
                "plugin.manifest_not_found",
                format!("目录中找不到 {PLUGIN_FILE}：{error}"),
            )
        })?;
        let manifest = plugin::parse_manifest(&bytes)?;
        let middleware_js = match &manifest.contributes.middleware {
            Some(contribution) => Some(
                tokio::fs::read(canonical.join(&contribution.file))
                    .await
                    .map_err(|error| filesystem("plugin.script_not_found", format!("找不到中间件脚本：{error}")))?,
            ),
            None => None,
        };
        install_bytes(paths, &bytes, "local", middleware_js.as_deref()).await
    }

    /// Install from a URL. The caller supplies the SHA-256 digest (published
    /// alongside the manifest); a mirror may change transport but never trust.
    pub async fn install_url(
        paths: &AppPaths,
        url: &str,
        sha256: &str,
    ) -> Result<PluginSummary, AppError> {
        let parsed = url::Url::parse(url).map_err(|_| {
            validation(
                "plugin.install_url_invalid",
                "插件地址不是合法的 URL。".into(),
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(validation(
                "plugin.install_url_invalid",
                "插件地址仅支持 http/https。".into(),
            ));
        }
        let digest = sha256.trim().to_ascii_lowercase();
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(validation(
                "plugin.sha256_invalid",
                "请提供 64 位十六进制 SHA-256 校验值。".into(),
            ));
        }

        let client = build_outbound_http_client_with_timeouts(OutboundTimeouts {
            total: Some(Duration::from_secs(30)),
            connect: Some(Duration::from_secs(10)),
            read: Some(Duration::from_secs(15)),
        })
        .map_err(|error| validation("plugin.http_client_failed", error))?;
        let mut response = client
            .get(parsed.clone())
            .send()
            .await
            .map_err(|error| validation("plugin.download_failed", error.to_string()))?;
        if !response.status().is_success() {
            return Err(validation(
                "plugin.download_failed",
                format!("下载插件失败：HTTP {}", response.status()),
            ));
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| validation("plugin.download_failed", error.to_string()))?
        {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > MANIFEST_MAX_BYTES {
                return Err(validation(
                    "plugin.manifest_too_large",
                    format!("插件清单超过 {MANIFEST_MAX_BYTES} 字节上限。"),
                ));
            }
        }

        // Verify before writing the final manifest: a hash failure must never
        // leave an installable pack behind.
        let temp = paths.plugins_dir.join(format!(".install-{}.part", std::process::id()));
        tokio::fs::create_dir_all(&paths.plugins_dir)
            .await
            .map_err(|error| filesystem("plugin.dir_create_failed", error.to_string()))?;
        tokio::fs::write(&temp, &bytes)
            .await
            .map_err(|error| filesystem("plugin.write_failed", error.to_string()))?;
        let verified = tokio::task::spawn_blocking({
            let temp = temp.clone();
            let digest = digest.clone();
            move || crate::services::pii_model::download::verify_sha256(&temp, &digest)
        })
        .await
        .map_err(|error| validation("plugin.verify_failed", error.to_string()))?;
        let _ = tokio::fs::remove_file(&temp).await;
        verified.map_err(|error| validation("plugin.sha256_mismatch", error))?;
        if bytes.len() > MANIFEST_MAX_BYTES {
            return Err(validation(
                "plugin.manifest_too_large",
                format!("插件清单超过 {MANIFEST_MAX_BYTES} 字节上限。"),
            ));
        }
        let manifest = plugin::parse_manifest(&bytes)?;
        if manifest.contributes.middleware.is_some() {
            return Err(validation(
                "plugin.middleware_url_unsupported",
                "含中间件脚本的插件请用本地文件夹安装。".into(),
            ));
        }
        install_bytes(paths, &bytes, "url", None).await
    }

    pub async fn uninstall(paths: &AppPaths, id: &str) -> Result<(), AppError> {
        crate::services::plugin_middleware::mark_dirty();
        let dir = pack_dir(paths, id)?;
        if !dir.is_dir() {
            return Err(filesystem(
                "plugin.not_installed",
                format!("插件 {id} 未安装。"),
            ));
        }
        tokio::fs::remove_dir_all(&dir)
            .await
            .map_err(|error| filesystem("plugin.uninstall_failed", error.to_string()))?;
        mutate_enabled(paths, |ids| {
            ids.remove(id);
        })
        .await
    }

    pub async fn set_enabled(paths: &AppPaths, id: &str, enabled: bool) -> Result<(), AppError> {
        crate::services::plugin_middleware::mark_dirty();
        // Validate the id and confirm the pack exists before touching settings.
        let _ = read_pack(paths, id).await?;
        mutate_enabled(paths, |ids| {
            if enabled {
                ids.insert(id.to_string());
            } else {
                ids.remove(id);
            }
        })
        .await
    }

    /// Presets from enabled packs only. Disabled or unreadable packs contribute
    /// nothing, so disabling a pack takes effect on the very next query.
    pub async fn list_enabled_presets(paths: &AppPaths) -> Result<Vec<PluginPreset>, AppError> {
        let settings = SettingsService::load(paths).await?;
        let enabled = enabled_ids(&settings);
        let mut presets = Vec::new();
        for id in enabled {
            if let Ok((manifest, _)) = read_pack(paths, &id).await {
                presets.extend(manifest.namespaced_presets());
            }
        }
        presets.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(presets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::platform::PlatformId;
    use tempfile::tempdir;

    fn manifest_json(id: &str) -> String {
        format!(
            r#"{{
                "schema_version": 1,
                "id": "{id}",
                "version": "1.0.0",
                "name": "Test Pack",
                "description": "fixture",
                "author": "tester",
                "license": "MIT",
                "contributes": {{
                    "provider_presets": [{{
                        "id": "primary",
                        "platform": "codex",
                        "label": "Test Relay",
                        "provider": "Test",
                        "default_name": "Test Relay",
                        "base_url": "https://relay.test/v1",
                        "interface_format": "openai",
                        "model_mappings": [{{"from": "m1", "to": "m1"}}]
                    }}]
                }}
            }}"#
        )
    }

    async fn pack_fixture(id: &str) -> (tempfile::TempDir, AppPaths) {
        let dir = tempdir().unwrap();
        let paths = AppPaths::from_data_dir(dir.path().to_path_buf());
        paths.ensure().await.unwrap();
        let folder = dir.path().join("source");
        tokio::fs::create_dir_all(&folder).await.unwrap();
        tokio::fs::write(folder.join(PLUGIN_FILE), manifest_json(id))
            .await
            .unwrap();
        (dir, paths)
    }

    #[tokio::test]
    async fn folder_install_copies_middleware_script() {
        let (_dir, paths) = pack_fixture("mw-pack").await;
        let source = paths.data_dir.join("source");
        let json = manifest_json("mw-pack").replace(
            "\"contributes\": {",
            "\"contributes\": {\"middleware\": {\"file\": \"middleware.js\"},",
        );
        tokio::fs::write(source.join(PLUGIN_FILE), json).await.unwrap();
        tokio::fs::write(source.join("middleware.js"), "function onRequest(body) { return body; }")
            .await
            .unwrap();

        PluginService::install_folder(&paths, source.to_str().unwrap())
            .await
            .unwrap();

        assert!(paths.plugins_dir.join("mw-pack").join("middleware.js").is_file());
    }

    #[tokio::test]
    async fn install_list_disable_and_uninstall_round_trip() {
        let (_dir, paths) = pack_fixture("test-pack").await;
        let source = paths.data_dir.join("source");

        let summary = PluginService::install_folder(&paths, source.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(summary.id, "test-pack");
        assert!(summary.enabled);
        assert_eq!(summary.source, "local");
        assert_eq!(summary.preset_count, 1);

        let list = PluginService::list(&paths).await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled);

        let detail = PluginService::get_detail(&paths, "test-pack").await.unwrap();
        assert_eq!(detail.manifest.name, "Test Pack");
        assert_eq!(
            std::path::Path::new(&detail.path).file_name(),
            Some(std::ffi::OsStr::new("test-pack"))
        );

        let presets = PluginService::list_enabled_presets(&paths).await.unwrap();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].id, "test-pack:primary");

        PluginService::set_enabled(&paths, "test-pack", false)
            .await
            .unwrap();
        assert!(PluginService::list_enabled_presets(&paths)
            .await
            .unwrap()
            .is_empty());
        assert!(!PluginService::list(&paths).await.unwrap()[0].enabled);

        PluginService::uninstall(&paths, "test-pack").await.unwrap();
        assert!(PluginService::list(&paths).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_pack_id_path_traversal() {
        let (_dir, paths) = pack_fixture("safe-pack").await;
        let error = PluginService::set_enabled(&paths, "../escape", true)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "plugin.id_invalid");
    }

    #[tokio::test]
    async fn skips_broken_packs_without_failing_the_list() {
        let (_dir, paths) = pack_fixture("safe-pack").await;
        let source = paths.data_dir.join("source");
        PluginService::install_folder(&paths, source.to_str().unwrap())
            .await
            .unwrap();
        let broken = paths.plugins_dir.join("broken");
        tokio::fs::create_dir_all(&broken).await.unwrap();
        tokio::fs::write(broken.join(PLUGIN_FILE), b"not json")
            .await
            .unwrap();
        let list = PluginService::list(&paths).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "safe-pack");
    }

    #[test]
    fn platform_parse_is_reused_for_preset_validation() {
        assert!(PlatformId::parse("codex").is_ok());
        assert!(PlatformId::parse("video").is_err());
    }

    #[tokio::test]
    async fn sha256_verify_rejects_a_changed_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        tokio::fs::write(&path, b"hello").await.unwrap();
        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let size = crate::services::pii_model::download::verify_sha256(&path, expected).unwrap();
        assert_eq!(size, 5);
        assert!(crate::services::pii_model::download::verify_sha256(&path, &"0".repeat(64)).is_err());
    }
}
