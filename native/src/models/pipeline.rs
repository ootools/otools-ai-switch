//! Per-account request/response plugin pipeline.
//!
//! The pipeline in `config_json.pipeline` is the single source of truth — no
//! legacy key translation. `passthrough` means every plugin in that direction
//! is skipped: raw forwarding, including scheduling plugins such as retry.

//! raw forwarding, including scheduling plugins such as retry.

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
/// yields empty chains — no implicit legacy translation.
pub fn resolve_pipeline(config_json: &str) -> PipelineConfig {
    let Ok(config) = serde_json::from_str::<Value>(config_json) else {
        return PipelineConfig::default();
    };
    config
        .get("pipeline")
        .and_then(|raw| serde_json::from_value::<PipelineConfig>(raw.clone()).ok())
        .filter(|pipeline| pipeline.version == PIPELINE_VERSION)
        .unwrap_or_default()
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
        let allowed: &[&str] = if request {
            &[REQUEST_TRANSFORM_PLUGINS, REQUEST_SCHEDULE_PLUGINS].concat()
        } else {
            RESPONSE_PLUGINS
        };
        for plugin in &direction.chain {
            if !allowed.contains(&plugin.id.as_str()) {
                return Err(AppError::Validation {
                    code: "validation.pipeline.unknown_plugin",
                    message: format!("插件 {} 不属于该方向。", plugin.id),
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
