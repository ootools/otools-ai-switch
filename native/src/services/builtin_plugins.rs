//! Built-in plugins exposed through the same plugin architecture as user packs.
//!
//! Every built-in ships as an embedded `middleware.js` script plus a descriptor
//! in `scripts/manifest.json`. The catalog, the QuickJS runtime and the account
//! chain all consume that manifest, so a built-in can be exported as a normal
//! third-party pack without changing the host.

use crate::error::AppError;
use crate::models::pipeline::PipelineConfig;
use std::sync::OnceLock;
use crate::models::route_credential::RouteCredentialFailurePolicy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginLane {
    Request,
    Response,
    Recovery,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Transform,
    Schedule,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginConfigField {
    pub key: String,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub default: Value,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub min: Option<u32>,
    #[serde(default)]
    pub max: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuiltinPluginDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub lane: PluginLane,
    pub kind: PluginKind,
    /// Where in the proxy pipeline this plugin runs. Not sent to the UI in a
    /// meaningful way, but kept on the wire shape for simplicity.
    #[serde(default = "default_stage")]
    pub stage: String,
    #[serde(default)]
    pub config_fields: Vec<PluginConfigField>,
    #[serde(default)]
    pub defaults: Value,
    #[serde(default)]
    pub legacy_rules: Value,
    #[serde(default)]
    pub limitations: String,
    #[serde(default = "api_version")]
    pub api_version: u32,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub stages: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub policy_domain: String,
    #[serde(default)]
    pub rule_editor: String,
    #[serde(default)]
    pub class_limits_editor: bool,
}

fn api_version() -> u32 { 1 }

fn default_stage() -> String {
    "perAttempt".into()
}

const MANIFEST: &str = include_str!("builtin_plugins/scripts/manifest.json");

/// Embedded scripts, keyed by the same ids used in account chains.
pub fn embedded_sources() -> Vec<(&'static str, &'static str)> {
    vec![
        ("builtin.request.requestHeaders",include_str!("builtin_plugins/scripts/builtin.request.requestHeaders.js")),
        ("builtin.request.imageProcessing",include_str!("builtin_plugins/scripts/builtin.request.imageProcessing.js")),
        ("builtin.recovery.retry", include_str!("builtin_plugins/scripts/builtin.recovery.retry.js")),
        ("builtin.recovery.stream", include_str!("builtin_plugins/scripts/builtin.recovery.stream.js")),
        ("builtin.recovery.accountHealth", include_str!("builtin_plugins/scripts/builtin.recovery.accountHealth.js")),
        ("builtin.request.plaintextReasoning", include_str!("builtin_plugins/scripts/builtin.response.plaintextReasoning.js")),
        ("builtin.request.crossResourceStrip", include_str!("builtin_plugins/scripts/builtin.response.crossResourceStrip.js")),
        ("builtin.request.encryptedContentCleanup", include_str!("builtin_plugins/scripts/builtin.response.encryptedContentCleanup.js")),
        ("builtin.request.compression", include_str!("builtin_plugins/scripts/builtin.request.compression.js")),
        ("builtin.request.contentRejection", include_str!("builtin_plugins/scripts/builtin.request.contentRejection.js")),
        ("builtin.request.imageLimit", include_str!("builtin_plugins/scripts/builtin.request.imageLimit.js")),
        ("builtin.request.forceReasoning", include_str!("builtin_plugins/scripts/builtin.request.forceReasoning.js")),
        ("builtin.request.inlineRemoteImages", include_str!("builtin_plugins/scripts/builtin.request.inlineRemoteImages.js")),
        ("builtin.request.turnReminder", include_str!("builtin_plugins/scripts/builtin.request.turnReminder.js")),
        ("builtin.request.customToolCompat", include_str!("builtin_plugins/scripts/builtin.request.customToolCompat.js")),
        ("builtin.schedule.retry", include_str!("builtin_plugins/scripts/builtin.schedule.retry.js")),
        ("builtin.schedule.semanticThreshold", include_str!("builtin_plugins/scripts/builtin.schedule.semanticThreshold.js")),
        ("builtin.schedule.cooldown", include_str!("builtin_plugins/scripts/builtin.schedule.cooldown.js")),
        ("builtin.schedule.errorStatus", include_str!("builtin_plugins/scripts/builtin.schedule.errorStatus.js")),
        ("builtin.schedule.streamPrelude", include_str!("builtin_plugins/scripts/builtin.schedule.streamPrelude.js")),
        ("builtin.schedule.streamSalvage", include_str!("builtin_plugins/scripts/builtin.schedule.streamSalvage.js")),
        ("builtin.schedule.streamContinue", include_str!("builtin_plugins/scripts/builtin.schedule.streamContinue.js")),
        ("builtin.schedule.failover", include_str!("builtin_plugins/scripts/builtin.schedule.failover.js")),
        ("builtin.response.plaintextReasoning", include_str!("builtin_plugins/scripts/builtin.response.plaintextReasoning.js")),
        ("builtin.response.crossResourceStrip", include_str!("builtin_plugins/scripts/builtin.response.crossResourceStrip.js")),
        ("builtin.response.encryptedContentCleanup", include_str!("builtin_plugins/scripts/builtin.response.encryptedContentCleanup.js")),
    ]
}

/// The catalog shown by the visual flow editor.
pub fn list() -> Vec<BuiltinPluginDescriptor> {
    let mut catalog = serde_json::from_str::<Vec<BuiltinPluginDescriptor>>(MANIFEST).expect("valid builtin plugin manifest");
    for entry in &mut catalog {
        if entry.rule_editor == "health" { entry.defaults["rules"] = serde_json::to_value(crate::services::failure_rules::health_defaults()).expect("health rules"); }
        if matches!(entry.rule_editor.as_str(), "retry" | "stream") {
            entry.defaults["rules"] = serde_json::to_value(crate::services::failure_rules::defaults()).expect("rules serialize");
            entry.legacy_rules = serde_json::to_value(crate::services::failure_rules::legacy_defaults()).expect("legacy rules");
        }
    }
    catalog
}

pub fn get(id: &str) -> Option<BuiltinPluginDescriptor> {
    list().into_iter().find(|plugin| plugin.id == id)
}

pub fn is_known_builtin(id: &str) -> bool {
    get(id).is_some()
}

/// Stage declared by an embedded plugin, or `perAttempt` for unknown ids.
pub fn stage_for(id: &str) -> String {
    get(id).map(|descriptor| descriptor.stage).unwrap_or_else(|| "perAttempt".into())
}

pub fn validate(pipeline: &PipelineConfig) -> Result<(), AppError> {
    crate::models::pipeline::validate_pipeline(pipeline)
}

fn merge_policy(policy: &mut RouteCredentialFailurePolicy, patch: &Value) {
    let Ok(serialized) = serde_json::to_value(*policy) else { return };
    let mut map = serialized.as_object().cloned().unwrap_or_default();
    let Some(patch) = patch.as_object() else { return };
    for (key, value) in patch {
        if map.contains_key(key) && !value.is_null() {
            map.insert(key.clone(), value.clone());
        }
    }
    if let Ok(merged) = serde_json::from_value::<RouteCredentialFailurePolicy>(Value::Object(map)) {
        let mut check=merged;
        // Inactive baseline uses zero as the disabled threshold; validate only
        // plugin-supplied values against the normal persisted-policy constraints.
        if !patch.contains_key("semantic_error_threshold"){check.semantic_error_threshold=check.semantic_error_threshold.max(1);}
        if !patch.contains_key("cooldown_seconds"){check.cooldown_seconds=check.cooldown_seconds.max(1);}
        if check.validate().is_err(){crate::services::plugin_host::log("host","error","Plugin policy exceeds supported budgets");return;}
        *policy = merged;
    }
}

/// Failure policy cache. The same account config is resolved on every retry
/// attempt, so cache by config_json and invalidate when plugins change.
static POLICY_CACHE: OnceLock<std::sync::Mutex<std::collections::HashMap<String, (u64, RouteCredentialFailurePolicy)>>> = OnceLock::new();
static POLICY_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const POLICY_CACHE_MAX: usize = 512;

/// Invalidate cached policies after a plugin install/uninstall/enable change.
pub fn mark_policy_dirty() {
    POLICY_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Resolve enabled recovery plugins from an inactive baseline. V1 policy semantics
/// are preserved by migration; V2 request passthrough does not disable recovery.
pub fn failure_policy_from_pipeline(pipeline: &PipelineConfig, config_json: &str) -> RouteCredentialFailurePolicy {
    let cache_key = format!("{}:{}", config_json, serde_json::to_string(pipeline).unwrap_or_default());
    let generation = POLICY_GENERATION.load(std::sync::atomic::Ordering::Relaxed);
    let cache = POLICY_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Ok(map) = cache.lock() {
        if let Some((cached_generation, policy)) = map.get(&cache_key) {
            if *cached_generation == generation {
                return *policy;
            }
        }
    }
    let policy = compute_policy(pipeline, config_json);
    if let Ok(mut map) = cache.lock() {
        if map.len() >= POLICY_CACHE_MAX {
            map.clear();
        }
        map.insert(cache_key, (generation, policy));
    }
    policy
}

/// V2 is explicit: absent or disabled recovery has no implicit behavior.
fn compute_policy(pipeline: &PipelineConfig, config_json: &str) -> RouteCredentialFailurePolicy {
    if pipeline.version < 2 { return legacy_policy(pipeline, config_json); }
    policy_for_domain(pipeline,"request")
}

pub fn policy_for_domain(pipeline:&PipelineConfig,domain:&str)->RouteCredentialFailurePolicy {
    let mut policy=zero_policy(RouteCredentialFailurePolicy::default());
    let mut seen=std::collections::HashSet::new();
    for plugin in crate::services::plugin_host::enabled(pipeline) {
        if !seen.insert(plugin.id.clone()){continue;}
        let Some(binding)=crate::services::plugin_middleware::loaded(&plugin.id) else{continue};
        let Some(d)=binding.descriptor.as_ref() else{continue};
        if !d.capabilities.iter().any(|c|c=="policy.configure") || !d.hooks.iter().any(|h|h=="policy"){continue;}
        let config=crate::services::plugin_contract::merge_config(&d.defaults,&plugin.config);
        if let Some(patch)=crate::services::plugin_middleware::policy_bound(&binding,&plugin.id,&config,&json!({"domain":domain,"phase":"policy"})){merge_policy(&mut policy,&patch);}
    }
    policy
}

pub fn stream_policy(pipeline: &PipelineConfig, _config_json: &str) -> RouteCredentialFailurePolicy {
    policy_for_domain(pipeline,"stream")
}

pub fn effective_policy(config_json: &str) -> RouteCredentialFailurePolicy {
    let pipeline = crate::models::pipeline::resolve_pipeline(config_json);
    failure_policy_from_pipeline(&pipeline, config_json)
}

pub fn legacy_policy(pipeline: &PipelineConfig, config_json: &str) -> RouteCredentialFailurePolicy {
    let mut policy = RouteCredentialFailurePolicy::from_config_json(config_json);
    if pipeline.is_passthrough(true) {
        return zero_policy(policy);
    }
    for plugin in pipeline.enabled_chain(true) {
        apply_schedule_fallback(&mut policy, &plugin.id, plugin.config.as_object());
    }
    policy
}

pub fn zero_policy(mut policy: RouteCredentialFailurePolicy) -> RouteCredentialFailurePolicy {
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
    policy
}

fn apply_schedule_fallback(policy: &mut RouteCredentialFailurePolicy, id: &str, fields: Option<&serde_json::Map<String, Value>>) {
    let empty = serde_json::Map::new();
    let fields = fields.unwrap_or(&empty);
    let u32_cfg = |key: &str, fallback: u32| -> u32 {
        fields.get(key).and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).unwrap_or(fallback)
    };
    match id {
        "builtin.schedule.retry" => {
            policy.retry_count = u32_cfg("retry_count", policy.retry_count);
            policy.retry_interval_ms = u32_cfg("retry_interval_ms", policy.retry_interval_ms);
            policy.max_retry_elapsed_seconds = u32_cfg("max_retry_elapsed_seconds", policy.max_retry_elapsed_seconds);
            policy.capacity_retry_interval_ms = u32_cfg("capacity_retry_interval_ms", policy.capacity_retry_interval_ms);
            if let Some(limits) = fields.get("retry_class_limits") {
                if let Ok(parsed) = serde_json::from_value(limits.clone()) {
                    policy.retry_class_limits = parsed;
                }
            }
        }
        "builtin.schedule.semanticThreshold" => policy.semantic_error_threshold = u32_cfg("value", policy.semantic_error_threshold),
        "builtin.schedule.cooldown" => {
            policy.cooldown_enabled = true;
            policy.cooldown_seconds = u32_cfg("seconds", policy.cooldown_seconds);
        }
        "builtin.schedule.errorStatus" => policy.error_status_enabled = true,
        "builtin.schedule.streamPrelude" => policy.stream_prelude_wait_ms = u32_cfg("ms", policy.stream_prelude_wait_ms),
        "builtin.schedule.streamSalvage" => policy.stream_salvage_enabled = true,
        "builtin.schedule.streamContinue" => {
            policy.stream_continue_enabled = true;
            policy.stream_continue_max = u32_cfg("max", policy.stream_continue_max);
        }
        "builtin.schedule.failover" => policy.failover_enabled = true,
        _ => {}
    }
}

/// Legacy config keys are no longer used to drive transforms, but keeping them
/// in sync lets other services (import/export, repository failure accounting)
/// observe the same settings. The derived policy is written to `failure_policy`
/// so cooldown / semantic threshold / error status take effect.
pub fn effective_config(pipeline: &PipelineConfig, config_json: &str) -> Value {
    effective_config_with_policy(pipeline, config_json, None)
}

pub fn effective_config_with_policy(
    pipeline: &PipelineConfig,
    config_json: &str,
    policy: Option<&RouteCredentialFailurePolicy>,
) -> Value {
    let mut config: Value = serde_json::from_str(config_json).unwrap_or_else(|_| json!({}));
    if !config.is_object() {
        config = json!({});
    }
    if let Some(policy) = policy {
        if let Ok(value) = serde_json::to_value(policy) {
            config.as_object_mut().expect("object").insert("failure_policy".into(), value);
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::pipeline::{resolve_pipeline, DirectionConfig, PipelineMode, PIPELINE_VERSION};

    #[test]
    fn catalog_has_all_lanes() {
        let catalog = list();
        assert!(catalog.iter().any(|p| p.id == "builtin.request.compression"));
        assert!(catalog.iter().any(|p| p.id == "builtin.recovery.retry"));
        assert!(catalog.iter().any(|p| p.id == "builtin.request.plaintextReasoning"));
        assert!(catalog.len() <= embedded_sources().len());
    }

    #[test]
    fn every_descriptor_has_an_embedded_script() {
        for descriptor in list() {
            assert!(embedded_sources().iter().any(|(id, _)| *id == descriptor.id), "{}", descriptor.id);
            assert!(!stage_for(&descriptor.id).is_empty());
        }
    }

    #[test]
    fn v2_stream_budget_does_not_inherit_retry_class_overrides() {
        let config=json!({"pipeline":{"version":2,"recovery":{"mode":"process","chain":[
            {"id":"builtin.recovery.retry","enabled":true,"config":{"retry_class_limits":{"capacity":100},"capacity_retry_interval_ms":60000}},
            {"id":"builtin.recovery.stream","enabled":true,"config":{"retry_count":2}}
        ]}}}).to_string();
        let pipeline=resolve_pipeline(&config);
        let policy=stream_policy(&pipeline,&config);
        assert_eq!(policy.retry_limit_for(crate::models::route_credential::RouteFailureClass::Capacity),2);
        assert_eq!(policy.capacity_retry_interval_ms,0);
    }

    #[test]
    fn passthrough_zeroes_policy() {
        let pipeline = PipelineConfig {
            version: PIPELINE_VERSION,
            catalog_revision: 1,
            request: DirectionConfig { mode: PipelineMode::Passthrough, chain: Vec::new() },
            response: DirectionConfig::default(),
            recovery: DirectionConfig::default(),
        };
        let policy = failure_policy_from_pipeline(&pipeline, "{}");
        assert_eq!(policy.retry_count, 0);
        assert!(!policy.failover_enabled);
    }
    #[test]
    fn policy_cache_returns_consistent_values_and_generation_advances() {
        let config = json!({
            "base_url": "https://relay.example.com/v1",
            "interface_format": "openai",
            "pipeline": {
                "version": 1,
                "request": {
                    "mode": "process",
                    "chain": [{ "id": "builtin.schedule.retry", "enabled": true, "config": { "retry_count": 9 } }]
                },
                "response": { "mode": "process", "chain": [] }
            }
        })
        .to_string();
        let pipeline = resolve_pipeline(&config);
        let first = failure_policy_from_pipeline(&pipeline, &config);
        let second = failure_policy_from_pipeline(&pipeline, &config);
        assert_eq!(first, second);
        assert_eq!(second.retry_count, 9);
        mark_policy_dirty();
        let third = failure_policy_from_pipeline(&pipeline, &config);
        assert_eq!(third.retry_count, 9);
    }
}



