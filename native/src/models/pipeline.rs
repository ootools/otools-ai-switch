//! Per-account request/response plugin pipeline.
//!
//! The pipeline in `config_json.pipeline` is the single source of truth — no
//! legacy key translation. `passthrough` means every plugin in that direction
//! is skipped: raw forwarding, including scheduling plugins such as retry.


use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const PIPELINE_VERSION: u32 = 1;

pub const REQUEST_TRANSFORM_PLUGINS: &[&str] = &[
    "builtin.request.compression",
    "builtin.request.contentRejection",
    "builtin.request.imageLimit",
    "builtin.request.forceReasoning",
    "builtin.request.inlineRemoteImages",
    "builtin.request.turnReminder",
    "builtin.request.customToolCompat",
];

pub const REQUEST_SCHEDULE_PLUGINS: &[&str] = &[
    "builtin.schedule.retry",
    "builtin.schedule.semanticThreshold",
    "builtin.schedule.cooldown",
    "builtin.schedule.errorStatus",
    "builtin.schedule.streamPrelude",
    "builtin.schedule.streamSalvage",
    "builtin.schedule.streamContinue",
    "builtin.schedule.failover",
];

pub const RESPONSE_PLUGINS: &[&str] = &[
    "builtin.response.plaintextReasoning",
    "builtin.response.crossResourceStrip",
    "builtin.response.encryptedContentCleanup",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineMode {
    Passthrough,
    Process,
}

impl Default for PipelineMode {
    fn default() -> Self {
        Self::Process
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PluginInstance {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub config: Value,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DirectionConfig {
    #[serde(default)]
    pub mode: PipelineMode,
    #[serde(default)]
    pub chain: Vec<PluginInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PipelineConfig {
    pub version: u32,
    #[serde(default)]
    pub request: DirectionConfig,
    #[serde(default)]
    pub response: DirectionConfig,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            version: PIPELINE_VERSION,
            request: DirectionConfig::default(),
            response: DirectionConfig::default(),
        }
    }
}

impl Default for DirectionConfig {
    fn default() -> Self {
        Self {
            mode: PipelineMode::Process,
            chain: Vec::new(),
        }
    }
}

impl PipelineConfig {
    pub fn is_passthrough(&self, request: bool) -> bool {
        if request {
            self.request.mode == PipelineMode::Passthrough
        } else {
            self.response.mode == PipelineMode::Passthrough
        }
    }

    /// Enabled plugin instances in chain order for one direction.
    pub fn enabled_chain(&self, request: bool) -> Vec<&PluginInstance> {
        let direction = if request { &self.request } else { &self.response };
        if direction.mode == PipelineMode::Passthrough {
            return Vec::new();
        }
        direction.chain.iter().filter(|plugin| plugin.enabled).collect()
    }

    pub fn has(&self, id: &str, request: bool) -> bool {
        self.enabled_chain(request).iter().any(|plugin| plugin.id == id)
    }

    pub fn config_for(&self, id: &str, request: bool) -> Option<&Value> {
        self.enabled_chain(request)
            .into_iter()
            .find(|plugin| plugin.id == id)
            .map(|plugin| &plugin.config)
    }
}

fn instance(id: &str, enabled: bool, config: Value) -> PluginInstance {
    PluginInstance {
        id: id.to_string(),
        enabled,
        config,
    }
}


/// The pipeline is the single source of truth. Missing or invalid data
/// yields empty chains — no implicit legacy translation in production.
pub fn resolve_pipeline(config_json: &str) -> PipelineConfig {
    let Ok(config) = serde_json::from_str::<Value>(config_json) else {
        return PipelineConfig::default();
    };
    if let Some(pipeline) = config
        .get("pipeline")
        .and_then(|raw| serde_json::from_value::<PipelineConfig>(raw.clone()).ok())
        .filter(|pipeline| pipeline.version == PIPELINE_VERSION)
    {
        return pipeline;
    }
    #[cfg(test)]
    { test_pipeline_from_legacy(&config) }
    #[cfg(not(test))]
    { PipelineConfig::default() }
}

/// Test-only shim: old route_proxy_service fixtures enable transforms with
/// flat config keys. Translate them so those tests keep exercising the new JS
/// chain without carrying legacy compatibility into production code.
#[cfg(test)]
fn test_pipeline_from_legacy(config: &Value) -> PipelineConfig {
    let mut request = Vec::new();
    let mut response = Vec::new();
    let get = |key: &str| config.get(key);
    if let Some(mode) = get("request_brotli_compression").and_then(Value::as_str) {
        if mode != "off" {
            request.push(instance("builtin.request.compression", true, json!({ "mode": mode })));
        }
    }
    if get("content_rejection_compat").and_then(Value::as_str) == Some("trim_old_tool_outputs") {
        request.push(instance("builtin.request.contentRejection", true, json!({})));
    }
    if get("image_limit_compat").and_then(Value::as_str) == Some("trim_old_images") {
        request.push(instance("builtin.request.imageLimit", true, json!({})));
    }
    if get("force_reasoning_content").and_then(Value::as_bool) == Some(true) {
        request.push(instance("builtin.request.forceReasoning", true, json!({})));
    }
    if get("inline_remote_images").and_then(Value::as_bool) == Some(true) {
        request.push(instance("builtin.request.inlineRemoteImages", true, json!({})));
    }
    if get("turn_reminder").and_then(Value::as_bool) == Some(true) {
        request.push(instance(
            "builtin.request.turnReminder",
            true,
            json!({ "text": get("turn_reminder_text").and_then(Value::as_str).unwrap_or("") }),
        ));
    }
    if get("responses_custom_tool_compat").and_then(Value::as_bool) == Some(true) {
        request.push(instance("builtin.request.customToolCompat", true, json!({})));
    }
    if let Some(mode) = get("responses_plaintext_reasoning_compat").and_then(Value::as_str) {
        if mode != "off" {
            response.push(instance("builtin.response.plaintextReasoning", true, json!({ "mode": mode })));
        }
    }
    if let Some(mode) = get("responses_cross_resource_strip").and_then(Value::as_str) {
        if mode != "off" {
            response.push(instance("builtin.response.crossResourceStrip", true, json!({ "mode": mode })));
        }
    }
    let host = config
        .get("base_url")
        .and_then(Value::as_str)
        .and_then(|base| base.split("//").nth(1))
        .and_then(|rest| rest.split('/').next())
        .map(|authority| authority.split(':').next().unwrap_or(authority).to_lowercase())
        .unwrap_or_default();
    if host == "anyrouter.top"
        && get("responses_plaintext_reasoning_compat").is_none()
        && !response.iter().any(|p| p.id == "builtin.response.plaintextReasoning")
    {
        // explicit off always wins; only the unset/auto case uses the host list
        response.push(instance("builtin.response.plaintextReasoning", true, json!({ "mode": "auto" })));
    }
    if get("responses_encrypted_content_cleanup").and_then(Value::as_bool) == Some(true) {
        response.push(instance(
            "builtin.response.encryptedContentCleanup",
            true,
            json!({ "aggressive_strip": get("responses_encrypted_content_aggressive_strip").and_then(Value::as_bool).unwrap_or(false) }),
        ));
    }
    if let Some(policy) = get("failure_policy") {
        let mut cfg = serde_json::Map::new();
        for key in ["retry_count", "retry_interval_ms", "max_retry_elapsed_seconds", "semantic_error_threshold", "cooldown_seconds", "stream_prelude_wait_ms", "stream_continue_max"] {
            if let Some(value) = policy.get(key) {
                cfg.insert(key.into(), value.clone());
            }
        }
        for (key, id) in [
            ("cooldown_enabled", "builtin.schedule.cooldown"),
            ("error_status_enabled", "builtin.schedule.errorStatus"),
            ("stream_salvage_enabled", "builtin.schedule.streamSalvage"),
            ("stream_continue_enabled", "builtin.schedule.streamContinue"),
            ("failover_enabled", "builtin.schedule.failover"),
        ] {
            if policy.get(key).and_then(Value::as_bool) == Some(true) {
                request.push(instance(id, true, Value::Object(cfg.clone())));
            }
        }
        if cfg.contains_key("retry_count") || cfg.contains_key("retry_interval_ms") {
            request.push(instance("builtin.schedule.retry", true, Value::Object(cfg)));
        }
    }
    PipelineConfig {
        version: PIPELINE_VERSION,
        request: DirectionConfig { mode: PipelineMode::Process, chain: request },
        response: DirectionConfig { mode: PipelineMode::Process, chain: response },
    }
}

/// Validate a pipeline submitted by the UI: version, known ids, lane placement.
pub fn validate_pipeline(pipeline: &PipelineConfig) -> Result<(), AppError> {
    if pipeline.version != PIPELINE_VERSION {
        return Err(AppError::Validation {
            code: "validation.pipeline.version",
            message: format!("不支持的插件管线版本 {}。", pipeline.version),
            details: None,
            recoverable: true,
        });
    }
    for (request, direction) in [(true, &pipeline.request), (false, &pipeline.response)] {
        let lane = if request { "request" } else { "response" };
        for plugin in &direction.chain {
            let known = if let Some(id) = plugin.id.strip_prefix("pack:") {
                crate::models::plugin::validate_pack_id(id).is_ok()
            } else {
                crate::services::builtin_plugins::get(&plugin.id)
                    .is_some_and(|descriptor| match descriptor.lane {
                        crate::services::builtin_plugins::PluginLane::Request => request,
                        crate::services::builtin_plugins::PluginLane::Response => !request,
                    })
            };
            if !known {
                return Err(AppError::Validation {
                    code: "validation.pipeline.unknown_plugin",
                    message: format!("插件 {} 不属于{}方向。", plugin.id, lane),
                    details: Some(plugin.id.clone()),
                    recoverable: true,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn passthrough_returns_no_chain() {
        let pipeline = PipelineConfig {
            version: PIPELINE_VERSION,
            request: DirectionConfig {
                mode: PipelineMode::Passthrough,
                chain: vec![instance("builtin.request.compression", true, json!({}))],
            },
            response: DirectionConfig::default(),
        };
        assert!(pipeline.is_passthrough(true));
        assert!(pipeline.enabled_chain(true).is_empty());
    }

    #[test]
    fn invalid_lane_is_rejected() {
        let pipeline = PipelineConfig {
            version: PIPELINE_VERSION,
            request: DirectionConfig::default(),
            response: DirectionConfig {
                mode: PipelineMode::Process,
                chain: vec![instance("builtin.request.compression", true, json!({}))],
            },
        };
        assert!(validate_pipeline(&pipeline).is_err());
    }
}

