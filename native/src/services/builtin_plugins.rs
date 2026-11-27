//! Built-in plugins exposed through the same plugin architecture as user packs.
//!
//! Unlike user packs (QuickJS, global, credential-free), built-ins are native
//! descriptors selected and ordered per account in `config_json.pipeline`. They
//! are applied by translating the chain into the flat config keys the existing
//! transform helpers already read, so no request logic is duplicated here.

use crate::error::AppError;
use crate::models::pipeline::{
    PipelineConfig, REQUEST_SCHEDULE_PLUGINS, REQUEST_TRANSFORM_PLUGINS, RESPONSE_PLUGINS,
};
use crate::models::route_credential::RouteCredentialFailurePolicy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginLane {
    Request,
    Response,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Transform,
    Schedule,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginConfigField {
    pub key: String,
    pub label: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuiltinPluginDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub lane: PluginLane,
    pub kind: PluginKind,
    #[serde(default)]
    pub config_fields: Vec<PluginConfigField>,
}

fn field(key: &str, label: &str, kind: &str) -> PluginConfigField {
    PluginConfigField {
        key: key.to_string(),
        label: label.to_string(),
        kind: kind.to_string(),
    }
}

fn descriptor(
    id: &str,
    name: &str,
    description: &str,
    lane: PluginLane,
    kind: PluginKind,
    fields: Vec<PluginConfigField>,
) -> BuiltinPluginDescriptor {
    BuiltinPluginDescriptor {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        lane,
        kind,
        config_fields: fields,
    }
}

/// The catalog shown by the visual flow editor.
pub fn list() -> Vec<BuiltinPluginDescriptor> {
    let request = PluginLane::Request;
    let response = PluginLane::Response;
    let transform = PluginKind::Transform;
    let schedule = PluginKind::Schedule;
    vec![
        descriptor("builtin.request.compression", "请求压缩", "按账号策略对出站请求体做 Brotli 压缩。", request.clone(), transform.clone(), vec![field("mode", "模式", "select")]),
        descriptor("builtin.request.contentRejection", "内容过滤兼容", "裁剪旧工具输出以避开上游内容审核。", request.clone(), transform.clone(), vec![]),
        descriptor("builtin.request.imageLimit", "图片超限兼容", "裁剪旧图片以避开上游图片数量限制。", request.clone(), transform.clone(), vec![]),
        descriptor("builtin.request.forceReasoning", "强制回传推理", "要求上游回传推理内容。", request.clone(), transform.clone(), vec![]),
        descriptor("builtin.request.inlineRemoteImages", "内联远程图片", "把远程图片内联后发送。", request.clone(), transform.clone(), vec![]),
        descriptor("builtin.request.turnReminder", "每轮纠偏提醒", "每轮请求追加一段自定义提醒。", request.clone(), transform.clone(), vec![field("text", "提醒内容", "textarea")]),
        descriptor("builtin.request.customToolCompat", "自定义工具兼容", "为 Responses 客户端补齐自定义工具字段。", request.clone(), transform.clone(), vec![]),
        descriptor("builtin.schedule.retry", "重试策略", "失败后的重试次数、间隔与按错误类型的上限。", request.clone(), schedule.clone(), vec![field("retry_count", "重试次数", "number"), field("retry_interval_ms", "重试间隔(ms)", "number"), field("max_retry_elapsed_seconds", "总时长上限(秒)", "number")]),
        descriptor("builtin.schedule.semanticThreshold", "语义失败阈值", "判定语义失败的阈值。", request.clone(), schedule.clone(), vec![field("value", "阈值", "number")]),
        descriptor("builtin.schedule.cooldown", "失败冷却", "失败后临时冷却该账号。", request.clone(), schedule.clone(), vec![field("seconds", "冷却秒数", "number")]),
        descriptor("builtin.schedule.errorStatus", "错误状态标记", "把失败写回账号状态标签。", request.clone(), schedule.clone(), vec![]),
        descriptor("builtin.schedule.streamPrelude", "建流等待", "等待上游建流帧的时长。", request.clone(), schedule.clone(), vec![field("ms", "等待(ms)", "number")]),
        descriptor("builtin.schedule.streamSalvage", "流内失败挽救", "流内失败时续写完成本轮。", request.clone(), schedule.clone(), vec![]),
        descriptor("builtin.schedule.streamContinue", "断流续写", "断流后继续请求补全答案。", request.clone(), schedule.clone(), vec![field("max", "最多轮次", "number")]),
        descriptor("builtin.schedule.failover", "故障转移", "失败后切换到池内其它账号。", request, schedule, vec![]),
        descriptor("builtin.response.plaintextReasoning", "明文推理兼容", "把上游推理字段规范化为明文。", response.clone(), transform.clone(), vec![field("mode", "模式", "select")]),
        descriptor("builtin.response.crossResourceStrip", "跨资源状态剥离", "剥离上游返回的跨资源状态字段。", response.clone(), transform.clone(), vec![field("mode", "模式", "select")]),
        descriptor("builtin.response.encryptedContentCleanup", "推理内容清理", "清理响应中的加密推理内容。", response, transform, vec![field("aggressive_strip", "激进剥离", "bool")]),
    ]
}

pub fn get(id: &str) -> Option<BuiltinPluginDescriptor> {
    list().into_iter().find(|plugin| plugin.id == id)
}

pub fn validate(pipeline: &PipelineConfig) -> Result<(), AppError> {
    crate::models::pipeline::validate_pipeline(pipeline)
}

fn plugin_config(pipeline: &PipelineConfig, id: &str, request: bool) -> Value {
    let Some(config) = pipeline.config_for(id, request) else {
        return json!({});
    };
    match config {
        Value::Object(map) => Value::Object(map.clone()),
        _ => json!({}),
    }
}

fn str_cfg(config: &Map<String, Value>, key: &str, fallback: &str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

fn bool_cfg(config: &Map<String, Value>, key: &str, fallback: bool) -> bool {
    config
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(fallback)
}

fn u32_cfg(config: &Map<String, Value>, key: &str, fallback: u32) -> u32 {
    config
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(fallback)
}

/// Translate the pipeline into the flat config keys read by the existing
/// transform helpers. Passthrough explicitly disables every transform key.
pub fn effective_config(pipeline: &PipelineConfig, config_json: &str) -> Value {
    let mut config: Value = serde_json::from_str(config_json).unwrap_or_else(|_| json!({}));
    if !config.is_object() {
        config = json!({});
    }
    let map = config.as_object_mut().expect("object");

    if pipeline.is_passthrough(true) {
        map.insert("request_brotli_compression".into(), json!("off"));
        map.insert("content_rejection_compat".into(), json!(""));
        map.insert("image_limit_compat".into(), json!(""));
        map.insert("force_reasoning_content".into(), json!(false));
        map.insert("inline_remote_images".into(), json!(false));
        map.insert("turn_reminder".into(), json!(false));
        map.insert("responses_custom_tool_compat".into(), json!(false));
    } else {
        for plugin in pipeline.enabled_chain(true) {
            let cfg = plugin_config(pipeline, &plugin.id, true);
            let fields = cfg.as_object().cloned().unwrap_or_default();
            match plugin.id.as_str() {
                "builtin.request.compression" => {
                    let mode = str_cfg(&fields, "mode", "");
                    if !mode.is_empty() {
                        map.insert("request_brotli_compression".into(), json!(mode));
                    }
                }
                "builtin.request.contentRejection" => {
                    map.insert("content_rejection_compat".into(), json!("trim_old_tool_outputs"));
                }
                "builtin.request.imageLimit" => {
                    map.insert("image_limit_compat".into(), json!("trim_old_images"));
                }
                "builtin.request.forceReasoning" => {
                    map.insert("force_reasoning_content".into(), json!(true));
                }
                "builtin.request.inlineRemoteImages" => {
                    map.insert("inline_remote_images".into(), json!(true));
                }
                "builtin.request.turnReminder" => {
                    map.insert("turn_reminder".into(), json!(true));
                    map.insert("turn_reminder_text".into(), json!(str_cfg(&fields, "text", "")));
                }
                "builtin.request.customToolCompat" => {
                    map.insert("responses_custom_tool_compat".into(), json!(true));
                }
                _ => {}
            }
        }
    }

    if pipeline.is_passthrough(false) {
        map.insert("responses_plaintext_reasoning_compat".into(), json!("off"));
        map.insert("responses_cross_resource_strip".into(), json!("off"));
        map.insert("responses_encrypted_content_cleanup".into(), json!(false));
    } else {
        for plugin in pipeline.enabled_chain(false) {
            let cfg = plugin_config(pipeline, &plugin.id, false);
            let fields = cfg.as_object().cloned().unwrap_or_default();
            match plugin.id.as_str() {
                "builtin.response.plaintextReasoning" => {
                    let mode = str_cfg(&fields, "mode", "");
                    if !mode.is_empty() {
                        map.insert("responses_plaintext_reasoning_compat".into(), json!(mode));
                    }
                }
                "builtin.response.crossResourceStrip" => {
                    let mode = str_cfg(&fields, "mode", "");
                    if !mode.is_empty() {
                        map.insert("responses_cross_resource_strip".into(), json!(mode));
                    }
                }
                "builtin.response.encryptedContentCleanup" => {
                    map.insert("responses_encrypted_content_cleanup".into(), json!(true));
                    map.insert("responses_encrypted_content_aggressive_strip".into(), json!(bool_cfg(&fields, "aggressive_strip", false)));
                }
                _ => {}
            }
        }
    }
    config
}

/// Build the failure policy from the request lane. Passthrough means one
/// attempt with no retry, no failover and no cooldown — raw forwarding.
pub fn failure_policy_from_pipeline(
    pipeline: &PipelineConfig,
    config_json: &str,
) -> RouteCredentialFailurePolicy {
    let mut policy = RouteCredentialFailurePolicy::from_config_json(config_json);
    if pipeline.is_passthrough(true) {
        policy.retry_count = 0;
        policy.retry_class_limits = Default::default();
        policy.capacity_retry_interval_ms = 0;
        policy.max_retry_elapsed_seconds = 0;
        policy.semantic_error_threshold = 0;
        policy.cooldown_enabled = false;
        policy.error_status_enabled = false;
        policy.stream_continue_enabled = false;
        policy.stream_salvage_enabled = false;
        policy.failover_enabled = false;
        policy.stream_prelude_wait_ms = 0;
        return policy;
    }
    for plugin in pipeline.enabled_chain(true) {
        let cfg = plugin_config(pipeline, &plugin.id, true);
        let fields = cfg.as_object().cloned().unwrap_or_default();
        match plugin.id.as_str() {
            "builtin.schedule.retry" => {
                policy.retry_count = u32_cfg(&fields, "retry_count", policy.retry_count);
                policy.retry_interval_ms = u32_cfg(&fields, "retry_interval_ms", policy.retry_interval_ms);
                policy.max_retry_elapsed_seconds = u32_cfg(&fields, "max_retry_elapsed_seconds", policy.max_retry_elapsed_seconds);
                policy.capacity_retry_interval_ms = u32_cfg(&fields, "capacity_retry_interval_ms", policy.capacity_retry_interval_ms);
                if let Some(limits) = fields.get("retry_class_limits") {
                    if let Ok(parsed) = serde_json::from_value(limits.clone()) {
                        policy.retry_class_limits = parsed;
                    }
                }
            }
            "builtin.schedule.semanticThreshold" => {
                policy.semantic_error_threshold = u32_cfg(&fields, "value", policy.semantic_error_threshold);
            }
            "builtin.schedule.cooldown" => {
                policy.cooldown_enabled = true;
                policy.cooldown_seconds = u32_cfg(&fields, "seconds", policy.cooldown_seconds);
            }
            "builtin.schedule.errorStatus" => policy.error_status_enabled = true,
            "builtin.schedule.streamPrelude" => {
                policy.stream_prelude_wait_ms = u32_cfg(&fields, "ms", policy.stream_prelude_wait_ms);
            }
            "builtin.schedule.streamSalvage" => policy.stream_salvage_enabled = true,
            "builtin.schedule.streamContinue" => {
                policy.stream_continue_enabled = true;
                policy.stream_continue_max = u32_cfg(&fields, "max", policy.stream_continue_max);
            }
            "builtin.schedule.failover" => policy.failover_enabled = true,
            _ => {}
        }
    }
    policy
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::pipeline::{resolve_pipeline, DirectionConfig, PipelineMode, PIPELINE_VERSION};

    #[test]
    fn catalog_has_all_lanes() {
        let catalog = list();
        assert!(catalog.iter().any(|p| p.id == "builtin.request.compression"));
        assert!(catalog.iter().any(|p| p.id == "builtin.schedule.retry"));
        assert!(catalog.iter().any(|p| p.id == "builtin.response.plaintextReasoning"));
        assert_eq!(catalog.len(), (REQUEST_TRANSFORM_PLUGINS.len() + REQUEST_SCHEDULE_PLUGINS.len() + RESPONSE_PLUGINS.len()));
    }

    #[test]
    fn effective_config_applies_chain() {
        let config = json!({ "request_brotli_compression": "off" });
        let pipeline = PipelineConfig {
            version: PIPELINE_VERSION,
            request: DirectionConfig {
                mode: PipelineMode::Process,
                chain: vec![crate::models::pipeline::PluginInstance {
                    id: "builtin.request.compression".into(),
                    enabled: true,
                    config: json!({ "mode": "on" }),
                }],
            },
            response: DirectionConfig::default(),
        };
        let effective = effective_config(&pipeline, &config.to_string());
        assert_eq!(effective["request_brotli_compression"], "on");
    }

    #[test]
    fn passthrough_disables_transforms_and_scheduling() {
        let pipeline = PipelineConfig {
            version: PIPELINE_VERSION,
            request: DirectionConfig {
                mode: PipelineMode::Passthrough,
                chain: Vec::new(),
            },
            response: DirectionConfig::default(),
        };
        let effective = effective_config(&pipeline, r#"{"request_brotli_compression":"on"}"#);
        assert_eq!(effective["request_brotli_compression"], "off");
        let policy = failure_policy_from_pipeline(&pipeline, "{}");
        assert_eq!(policy.retry_count, 0);
        assert!(!policy.failover_enabled);
    }

    #[test]
    fn legacy_pipeline_keeps_policy_equivalent() {
        let config = json!({ "failure_policy": { "retry_count": 7, "failover_enabled": false } });
        let pipeline = resolve_pipeline(&config.to_string());
        let policy = failure_policy_from_pipeline(&pipeline, &config.to_string());
        assert_eq!(policy.retry_count, 7);
        assert!(!policy.failover_enabled);
    }
}

/// Apply third-party pack hooks selected in the account chain, in order.
/// Built-in transforms are applied by the existing native helpers; packs run
/// here so they are no longer global or implicit.
pub fn apply_pack_chain(
    pipeline: &PipelineConfig,
    body: &[u8],
    request: bool,
    platform: &str,
    path: &str,
) -> Vec<u8> {
    if pipeline.is_passthrough(request) {
        return body.to_vec();
    }
    let hook = if request { "onRequest" } else { "onResponse" };
    let mut current = body.to_vec();
    for plugin in pipeline.enabled_chain(request) {
        if let Some(id) = plugin.id.strip_prefix("pack:") {
            current =
                crate::services::plugin_middleware::apply_pack_hook(id, hook, &current, platform, path);
        }
    }
    current
}
