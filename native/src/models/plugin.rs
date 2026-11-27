//! Declarative plugin packs.
//!
//! A pack is a single `plugin.json` manifest. It contains **data only** — no
//! entry point, no script, no code. Nothing from a pack is ever executed: the
//! only thing v1 does with a manifest is turn `contributes.provider_presets`
//! into extra choices in the create-account preset dropdown.
//!
//! Keeping the surface this small is deliberate. The host is a cdylib loaded
//! by OTools and owns every credential; running third-party JS in this process
//! (the Magpie model) would hand those credentials to the plugin. See
//! `docs/superpowers/specs/` for the design constraints.

use crate::error::AppError;
use crate::models::platform::PlatformId;
use serde::{Deserialize, Serialize};

/// Hard ceiling on a manifest file. Generous for hand-written JSON, small
/// enough that a URL install cannot be used as a memory/disk bomb.
pub const MANIFEST_MAX_BYTES: usize = 256 * 1024;
pub const MAX_PRESETS_PER_PACK: usize = 50;
pub const MAX_MAPPINGS_PER_PRESET: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub contributes: PluginContributes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginContributes {
    #[serde(default)]
    pub provider_presets: Vec<ProviderPreset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderPreset {
    pub id: String,
    pub platform: String,
    pub label: String,
    pub provider: String,
    pub default_name: String,
    pub base_url: String,
    pub interface_format: String,
    pub model_mappings: Vec<PluginMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginMapping {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginSummary {
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub enabled: bool,
    pub preset_count: usize,
    /// `"local"` or `"url"` — set at install time, not re-derived from the manifest.
    pub source: String,
    /// Non-fatal problems found while loading the pack (e.g. unknown field).
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginDetail {
    pub manifest: PluginManifest,
    pub enabled: bool,
    pub source: String,
    pub path: String,
}

/// A preset exposed to the frontend. `id` is namespaced (`<plugin>:<preset>`)
/// so two packs can both ship a preset called `primary`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginPreset {
    pub id: String,
    pub plugin_id: String,
    pub platform: String,
    pub label: String,
    pub provider: String,
    pub default_name: String,
    pub base_url: String,
    pub interface_format: String,
    pub model_mappings: Vec<PluginMapping>,
}

fn invalid(code: &'static str, message: String) -> AppError {
    AppError::Validation {
        code,
        message,
        details: None,
        recoverable: true,
    }
}

/// Parse a manifest with the size cap applied **before** deserialization.
pub fn parse_manifest(bytes: &[u8]) -> Result<PluginManifest, AppError> {
    if bytes.len() > MANIFEST_MAX_BYTES {
        return Err(invalid(
            "plugin.manifest_too_large",
            format!(
                "插件清单超过 {} 字节上限（实际 {} 字节）。",
                MANIFEST_MAX_BYTES,
                bytes.len()
            ),
        ));
    }
    let manifest: PluginManifest = serde_json::from_slice(bytes).map_err(|error| {
        invalid(
            "plugin.manifest_invalid_json",
            format!("插件清单不是合法的 JSON：{error}"),
        )
    })?;
    manifest.validate()?;
    Ok(manifest)
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.schema_version != 1 {
            return Err(invalid(
                "plugin.schema_version_unsupported",
                format!("不支持的插件清单版本 {}，仅支持 1。", self.schema_version),
            ));
        }
        validate_pack_id(&self.id)?;
        validate_short(&self.version, 64, "plugin.version_invalid", "版本号")?;
        validate_short(&self.name, 120, "plugin.name_invalid", "插件名称")?;
        if self.description.len() > 2000 {
            return Err(invalid(
                "plugin.description_too_long",
                "插件描述不能超过 2000 字符。".into(),
            ));
        }
        validate_short(&self.author, 120, "plugin.author_invalid", "作者")?;
        validate_short(&self.license, 64, "plugin.license_invalid", "许可证")?;

        if self.contributes.provider_presets.len() > MAX_PRESETS_PER_PACK {
            return Err(invalid(
                "plugin.too_many_presets",
                format!("每个插件最多声明 {MAX_PRESETS_PER_PACK} 个供应商预设。"),
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for preset in &self.contributes.provider_presets {
            validate_preset_id(&preset.id)?;
            if !seen.insert(preset.id.as_str()) {
                return Err(invalid(
                    "plugin.duplicate_preset",
                    format!("插件内存在重复的预设 id：{}", preset.id),
                ));
            }
            PlatformId::parse(&preset.platform).map_err(|_| {
                invalid(
                    "plugin.platform_invalid",
                    format!("预设 {} 的平台 {} 不被支持。", preset.id, preset.platform),
                )
            })?;
            if !is_supported_interface_format(&preset.interface_format) {
                return Err(invalid(
                    "plugin.interface_format_invalid",
                    format!(
                        "预设 {} 的接口格式 {} 不被支持。",
                        preset.id, preset.interface_format
                    ),
                ));
            }
            validate_short(
                &preset.label,
                160,
                "plugin.preset_label_invalid",
                "预设标签",
            )?;
            validate_short(
                &preset.provider,
                120,
                "plugin.provider_invalid",
                "供应商名称",
            )?;
            validate_short(
                &preset.default_name,
                120,
                "plugin.default_name_invalid",
                "默认账号名",
            )?;
            validate_base_url(&preset.base_url, &preset.id)?;
            if preset.model_mappings.len() > MAX_MAPPINGS_PER_PRESET {
                return Err(invalid(
                    "plugin.too_many_mappings",
                    format!(
                        "预设 {} 的模型映射超过 {MAX_MAPPINGS_PER_PRESET} 条。",
                        preset.id
                    ),
                ));
            }
            for mapping in &preset.model_mappings {
                validate_short(&mapping.from, 256, "plugin.mapping_from_invalid", "模型别名")?;
                validate_short(&mapping.to, 256, "plugin.mapping_to_invalid", "上游模型名")?;
                if mapping.from.trim().is_empty() || mapping.to.trim().is_empty() {
                    return Err(invalid(
                        "plugin.mapping_empty",
                        format!("预设 {} 含有空的模型映射。", preset.id),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn summary(&self, enabled: bool, source: &str) -> PluginSummary {
        PluginSummary {
            id: self.id.clone(),
            version: self.version.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            author: self.author.clone(),
            license: self.license.clone(),
            enabled,
            preset_count: self.contributes.provider_presets.len(),
            source: source.to_string(),
            warnings: Vec::new(),
        }
    }

    /// Presets namespaced with the pack id. Used by both the service list and
    /// the create-account dropdown.
    pub fn namespaced_presets(&self) -> Vec<PluginPreset> {
        self.contributes
            .provider_presets
            .iter()
            .map(|preset| PluginPreset {
                id: format!("{}:{}", self.id, preset.id),
                plugin_id: self.id.clone(),
                platform: preset.platform.clone(),
                label: preset.label.clone(),
                provider: preset.provider.clone(),
                default_name: preset.default_name.clone(),
                base_url: preset.base_url.clone(),
                interface_format: preset.interface_format.clone(),
                model_mappings: preset.model_mappings.clone(),
            })
            .collect()
    }
}

/// Plugin id doubles as a directory name; this is also the path-traversal
/// boundary (no `/`, `\`, `..`, leading dot, or URL schemes).
pub fn validate_pack_id(id: &str) -> Result<(), AppError> {
    let bytes = id.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 64
        || !(bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
    {
        return Err(invalid(
            "plugin.id_invalid",
            "插件 id 需以小写字母或数字开头，长度 1–64，仅含小写字母、数字、点、下划线和连字符。".into(),
        ));
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(invalid(
            "plugin.id_invalid",
            "插件 id 只能包含小写字母、数字、点、下划线和连字符。".into(),
        ));
    }
    if id.contains("..") {
        return Err(invalid(
            "plugin.id_invalid",
            "插件 id 不能包含连续的点。".into(),
        ));
    }
    Ok(())
}

fn validate_preset_id(id: &str) -> Result<(), AppError> {
    let bytes = id.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 64
        || !bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
    {
        return Err(invalid(
            "plugin.preset_id_invalid",
            "预设 id 需为小写字母、数字、下划线或连字符，长度 1–64。".into(),
        ));
    }
    Ok(())
}

fn validate_short(
    value: &str,
    max: usize,
    code: &'static str,
    label: &str,
) -> Result<(), AppError> {
    if value.trim().is_empty() || value.len() > max {
        return Err(invalid(
            code,
            format!("{label}不能为空且不能超过 {max} 字符。"),
        ));
    }
    Ok(())
}

fn validate_base_url(value: &str, preset_id: &str) -> Result<(), AppError> {
    let parsed = url::Url::parse(value).map_err(|_| {
        invalid(
            "plugin.base_url_invalid",
            format!("预设 {preset_id} 的 base_url 不是合法 URL。"),
        )
    })?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(invalid(
            "plugin.base_url_invalid",
            format!("预设 {preset_id} 的 base_url 仅支持不含账号密码或片段的 http/https URL。"),
        ));
    }
    Ok(())
}

fn is_supported_interface_format(value: &str) -> bool {
    matches!(value, "openai" | "openai-responses" | "anthropic" | "gemini")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_json() -> String {
        r#"{
            "schema_version": 1,
            "id": "example-relay",
            "version": "1.0.0",
            "name": "Example Relay",
            "description": "A test pack",
            "author": "Tester",
            "license": "MIT",
            "contributes": {
                "provider_presets": [{
                    "id": "primary",
                    "platform": "codex",
                    "label": "Example (example.com)",
                    "provider": "Example",
                    "default_name": "Example",
                    "base_url": "https://example.com/v1",
                    "interface_format": "openai-responses",
                    "model_mappings": [{ "from": "model-a", "to": "model-a" }]
                }]
            }
        }"#
        .to_string()
    }

    #[test]
    fn parses_valid_manifest_and_namespaces_presets() {
        let manifest = parse_manifest(manifest_json().as_bytes()).unwrap();
        assert_eq!(manifest.id, "example-relay");
        let presets = manifest.namespaced_presets();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].id, "example-relay:primary");
        assert_eq!(presets[0].model_mappings[0].to, "model-a");
    }

    #[test]
    fn rejects_unknown_fields_so_code_cannot_be_smuggled_in() {
        let json = manifest_json().replace(
            "\"schema_version\": 1,",
            "\"schema_version\": 1, \"entry\": \"index.mjs\",",
        );
        let error = parse_manifest(json.as_bytes()).unwrap_err();
        assert_eq!(error.code(), "plugin.manifest_invalid_json");
    }

    #[test]
    fn rejects_bad_ids_formats_platforms_and_urls() {
        let cases = [
            ("\"id\": \"example-relay\"", "\"id\": \"../escape\"", "plugin.id_invalid"),
            ("\"platform\": \"codex\"", "\"platform\": \"video\"", "plugin.platform_invalid"),
            ("\"interface_format\": \"openai-responses\"", "\"interface_format\": \"runJS\"", "plugin.interface_format_invalid"),
            ("\"base_url\": \"https://example.com/v1\"", "\"base_url\": \"file:///etc/passwd\"", "plugin.base_url_invalid"),
        ];
        for (from, to, expected) in cases {
            let json = manifest_json().replace(from, to);
            let error = parse_manifest(json.as_bytes()).unwrap_err();
            assert_eq!(error.code(), expected, "{to}");
        }
    }

    #[test]
    fn rejects_oversize_manifest_before_parsing() {
        let json = manifest_json().replace("A test pack", &"x".repeat(MANIFEST_MAX_BYTES));
        let error = parse_manifest(json.as_bytes()).unwrap_err();
        assert_eq!(error.code(), "plugin.manifest_too_large");
    }

    #[test]
    fn rejects_duplicate_preset_ids() {
        let mut manifest = parse_manifest(manifest_json().as_bytes()).unwrap();
        let preset = manifest.contributes.provider_presets[0].clone();
        manifest.contributes.provider_presets.push(preset);
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let error = parse_manifest(&bytes).unwrap_err();
        assert_eq!(error.code(), "plugin.duplicate_preset");
    }
}
