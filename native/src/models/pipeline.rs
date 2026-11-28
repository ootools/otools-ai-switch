//! Per-account middleware lifecycle configuration.
//! V2 separates request/response transforms from failure recovery. V1 and flat
//! account settings migrate through production code; explicit V2 absence disables.



use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const PIPELINE_VERSION: u32 = 2;

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
    pub catalog_revision: u32,
    #[serde(default)]
    pub request: DirectionConfig,
    #[serde(default)]
    pub response: DirectionConfig,
    #[serde(default)]
    pub recovery: DirectionConfig,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            version: PIPELINE_VERSION,
            catalog_revision: 1,
            request: DirectionConfig::default(),
            response: DirectionConfig::default(),
            recovery: DirectionConfig::default(),
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


/// Resolve versioned pipelines and migrate legacy account settings once.
pub fn has_explicit_pipeline(config_json: &str) -> bool {
    serde_json::from_str::<Value>(config_json).ok().is_some_and(|c|c.get("pipeline").is_some())
}

pub fn resolve_pipeline(config_json: &str) -> PipelineConfig {
    let Ok(config) = serde_json::from_str::<Value>(config_json) else {
        return PipelineConfig::default();
    };
    let pipeline = config.get("pipeline")
        .and_then(|raw| serde_json::from_value::<PipelineConfig>(raw.clone()).ok())
        .filter(|p| p.version == 1 || p.version == PIPELINE_VERSION)
        .unwrap_or_else(|| legacy_pipeline(&config));
    upgrade_catalog(migrate_pipeline(pipeline, config_json),&config)
}

fn upgrade_catalog(mut pipeline:PipelineConfig,config:&Value)->PipelineConfig {
    if pipeline.catalog_revision>=1{return pipeline;}
    let mut chain=Vec::new();let mut merged:Option<usize>=None;
    let has_images=pipeline.request.chain.iter().any(|p|p.id=="builtin.request.imageProcessing");
    let mut redundant=Vec::new();
    for plugin in std::mem::take(&mut pipeline.request.chain) {
        let key=match plugin.id.as_str(){"builtin.request.inlineRemoteImages"=>"inline_remote","builtin.request.imageLimit"=>"trim_old_images",_=>{chain.push(plugin);continue;}};
        if has_images {redundant.push(plugin);continue;}
        let index=*merged.get_or_insert_with(||{chain.push(instance("builtin.request.imageProcessing",false,json!({"inline_remote":false,"trim_old_images":false,"max_images":10,"legacy_plugins":[]})));chain.len()-1});
        let entry=&mut chain[index];entry.enabled|=plugin.enabled;entry.config[key]=json!(plugin.enabled);
        entry.config["legacy_plugins"].as_array_mut().unwrap().push(serde_json::to_value(plugin).unwrap());
    }
    if !redundant.is_empty(){
        if let Some(entry)=chain.iter_mut().find(|p|p.id=="builtin.request.imageProcessing") {
            let mut old=entry.config.get("legacy_plugins").and_then(Value::as_array).cloned().unwrap_or_default();
            old.extend(redundant.into_iter().map(|p|serde_json::to_value(p).unwrap()));entry.config["legacy_plugins"]=json!(old);
        }
    }
    if !chain.iter().any(|p|p.id=="builtin.request.requestHeaders") {
        if let Some(values)=config.get("headers").and_then(Value::as_object){
            let mut ua=String::new();let mut headers=Vec::new();
            for (name,value) in values {if let Some(value)=value.as_str().map(str::trim).filter(|v|!v.is_empty()) {
                if name.eq_ignore_ascii_case("user-agent"){ua=value.into();}else{headers.push(json!({"name":name,"value":value,"operation":"fill"}));}
            }}
            if !ua.is_empty() || !headers.is_empty(){
                if pipeline.request.mode==PipelineMode::Passthrough {for p in &mut chain {p.enabled=false;} pipeline.request.mode=PipelineMode::Process;}
                chain.push(instance("builtin.request.requestHeaders",true,json!({"user_agent":ua,"headers":headers})));
            }
        }
    }
    pipeline.request.chain=chain;pipeline.catalog_revision=1;pipeline
}

fn migrate_pipeline(mut pipeline: PipelineConfig, config_json: &str) -> PipelineConfig {
    if pipeline.version == PIPELINE_VERSION { return pipeline; }
    let policy = crate::services::builtin_plugins::legacy_policy(&pipeline, config_json);
    let old_request_mode = pipeline.request.mode;
    let old_schedule: Vec<_> = pipeline.request.chain.iter().filter(|p| p.id.starts_with("builtin.schedule.")).cloned().collect();
    pipeline.request.chain.retain(|p| !p.id.starts_with("builtin.schedule."));
    if old_request_mode == PipelineMode::Passthrough { for p in &mut pipeline.request.chain { p.enabled = false; } }
    let mut response = Vec::new();
    for mut plugin in std::mem::take(&mut pipeline.response.chain) {
        if RESPONSE_PLUGINS.contains(&plugin.id.as_str()) {
            plugin.id = plugin.id.replacen("builtin.response.", "builtin.request.", 1);
            plugin.enabled &= pipeline.response.mode != PipelineMode::Passthrough;
            if plugin.enabled { pipeline.request.mode = PipelineMode::Process; }
            pipeline.request.chain.push(plugin);
        } else { response.push(plugin); }
    }
    pipeline.response.chain = response;
    pipeline.recovery = DirectionConfig { mode: old_request_mode, chain: recovery_plugins(policy, true) };
    for plugin in &mut pipeline.recovery.chain { plugin.config["legacy_plugins"] = json!(old_schedule); }
    pipeline.request.chain.retain(|p| p.enabled || !untouched_disabled(p));
    pipeline.version = PIPELINE_VERSION;
    pipeline
}

fn untouched_disabled(p: &PluginInstance) -> bool {
    let known = REQUEST_TRANSFORM_PLUGINS.contains(&p.id.as_str()) || ["builtin.request.plaintextReasoning","builtin.request.crossResourceStrip","builtin.request.encryptedContentCleanup"].contains(&p.id.as_str());
    known && (p.config == json!({}) || p.config == json!({"mode":"off"}) || p.config == json!({"text":""}) || p.config == json!({"aggressive_strip":false}))
}

pub fn recovery_plugins(p: crate::models::route_credential::RouteCredentialFailurePolicy, legacy: bool) -> Vec<PluginInstance> {
    let mut result = vec![
        instance("builtin.recovery.retry", p.retry_count > 0 || p.failover_enabled || p.stream_prelude_wait_ms > 0, json!({
            "retry_count":p.retry_count,"retry_interval_ms":p.retry_interval_ms,"max_retry_elapsed_seconds":p.max_retry_elapsed_seconds,
            "capacity_retry_interval_ms":p.capacity_retry_interval_ms,"retry_class_limits":p.retry_class_limits,
            "failover_enabled":p.failover_enabled,"stream_prelude_wait_ms":p.stream_prelude_wait_ms,"legacy_matching":legacy
        })),
        instance("builtin.recovery.accountHealth", p.cooldown_enabled || p.error_status_enabled, json!({
            "cooldown_enabled":p.cooldown_enabled,"cooldown_seconds":p.cooldown_seconds,
            "semantic_error_threshold":p.semantic_error_threshold,"error_status_enabled":p.error_status_enabled
        }))
    ];
    if p.stream_continue_enabled || p.stream_salvage_enabled {
        result.push(instance("builtin.recovery.stream", true, json!({
            "stream_continue_enabled":p.stream_continue_enabled,"stream_continue_max":p.stream_continue_max,
            "stream_salvage_enabled":p.stream_salvage_enabled,"retry_count":p.retry_count,"retry_interval_ms":p.retry_interval_ms,
            "max_retry_elapsed_seconds":p.max_retry_elapsed_seconds,"capacity_retry_interval_ms":p.capacity_retry_interval_ms,
            "retry_class_limits":p.retry_class_limits,"failover_enabled":p.failover_enabled
        })));
    }
    result
}

/// Production migration of accounts created before versioned pipelines.
fn legacy_pipeline(config: &Value) -> PipelineConfig {
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
    if host == "ps.air-outer.com" && get("request_brotli_compression").is_none()
        && config.get("base_url").and_then(Value::as_str).is_some_and(|u| u.starts_with("https://")) {
        request.push(instance("builtin.request.compression",true,json!({"mode":"auto"})));
    }
    if ["agentrouter.org", "ps.air-outer.com"].contains(&host.as_str()) && get("responses_cross_resource_strip").is_none() {
        response.push(instance("builtin.response.crossResourceStrip", true, json!({"mode":"auto"})));
    }
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
        version: 1,
        catalog_revision: 0,        recovery: DirectionConfig::default(),
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
    for (lane, direction) in [("request", &pipeline.request), ("response", &pipeline.response), ("recovery", &pipeline.recovery)] {
        for plugin in &direction.chain {
            let descriptor=crate::services::plugin_contract::descriptor(&plugin.id);
            if !plugin.config.is_object() { return Err(AppError::Validation { code:"validation.pipeline.config",message:"插件配置必须是对象".into(),details:Some(plugin.id.clone()),recoverable:true }); }
            if descriptor.is_none() {
                if plugin.id.strip_prefix("pack:").is_some_and(|id|crate::models::plugin::validate_pack_id(id).is_ok()){continue;}
            }
            if let Some(d)=descriptor.as_ref(){crate::services::plugin_contract::validate_config(d,&plugin.config).map_err(|message|AppError::Validation{code:"validation.pipeline.config",message,details:Some(plugin.id.clone()),recoverable:true})?;}
            if lane == "recovery" && descriptor.as_ref().is_some_and(|d|d.capabilities.iter().any(|c|c=="policy.configure")) {
                let mut policy = serde_json::to_value(crate::models::route_credential::RouteCredentialFailurePolicy::default()).expect("policy");
                for (key,value) in plugin.config.as_object().expect("object") { if policy.get(key).is_some() { policy[key]=value.clone(); } }
                let result = serde_json::from_value::<crate::models::route_credential::RouteCredentialFailurePolicy>(policy).map_err(|e|e.to_string()).and_then(|p|p.validate());
                result.map_err(|message|AppError::Validation{code:"validation.pipeline.config",message,details:Some(plugin.id.clone()),recoverable:true})?;
            }
            if let Some(raw) = plugin.config.get("rules").filter(|_| descriptor.as_ref().is_some_and(|d|!d.rule_editor.is_empty())) {
                let rules = serde_json::from_value::<Vec<crate::services::failure_rules::FailureRule>>(raw.clone())
                    .map_err(|e| AppError::Validation { code:"validation.pipeline.rules", message:"错误规则格式无效".into(), details:Some(e.to_string()),recoverable:true })?;
                crate::services::failure_rules::validate(&rules).map_err(|message| AppError::Validation {code:"validation.pipeline.rules",message,details:Some(plugin.id.clone()),recoverable:true})?;
                let valid_actions: &[&str] = if descriptor.as_ref().is_some_and(|d|d.rule_editor=="health") { &["record","ignore"] } else if descriptor.as_ref().is_some_and(|d|d.rule_editor=="stream") { &["retry","return"] } else { &["retry","return","next_account"] };
                if rules.iter().any(|r| !valid_actions.contains(&r.action.as_str())) { return Err(AppError::Validation{code:"validation.pipeline.rules",message:"此插件不支持所选规则动作".into(),details:Some(plugin.id.clone()),recoverable:true}); }
            }
            let known=descriptor.is_some_and(|d|match d.lane {
                crate::services::builtin_plugins::PluginLane::Request=>lane=="request",
                crate::services::builtin_plugins::PluginLane::Response=>lane=="response",
                crate::services::builtin_plugins::PluginLane::Recovery=>lane=="recovery",
                crate::services::builtin_plugins::PluginLane::Both=>lane!="recovery",
            });
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
            catalog_revision: 1,
            request: DirectionConfig {
                mode: PipelineMode::Passthrough,
                chain: vec![instance("builtin.request.compression", true, json!({}))],
            },
            response: DirectionConfig::default(),
            recovery: DirectionConfig::default(),
        };
        assert!(pipeline.is_passthrough(true));
        assert!(pipeline.enabled_chain(true).is_empty());
    }

    #[test]
    fn invalid_lane_is_rejected() {
        let pipeline = PipelineConfig {
            version: PIPELINE_VERSION,
            catalog_revision: 1,
            request: DirectionConfig::default(),
            recovery: DirectionConfig::default(),
            response: DirectionConfig {
                mode: PipelineMode::Process,
                chain: vec![instance("builtin.request.compression", true, json!({}))],
            },
        };
        assert!(validate_pipeline(&pipeline).is_err());
    }
}


#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::services::builtin_plugins::failure_policy_from_pipeline;

    #[test]
    fn response_cleanup_survives_old_request_passthrough_without_enabling_request_plugins() {
        let c=json!({"pipeline":{"version":1,"request":{"mode":"passthrough","chain":[{"id":"builtin.request.turnReminder","enabled":true,"config":{"text":"keep"}}]},"response":{"mode":"process","chain":[{"id":"builtin.response.encryptedContentCleanup","enabled":true,"config":{"aggressive_strip":true}}]}}}).to_string();
        let p=resolve_pipeline(&c);
        assert!(p.has("builtin.request.encryptedContentCleanup",true));
        assert!(!p.has("builtin.request.turnReminder",true));
        assert_eq!(p.recovery.mode,PipelineMode::Passthrough);
    }

    #[test]
    fn header_image_migration_preserves_passthrough_headers_and_archives_duplicates(){
        let p=resolve_pipeline(&json!({"headers":{"User-Agent":"Pinned/1"},"pipeline":{"version":2,"request":{"mode":"passthrough","chain":[{"id":"builtin.request.compression","enabled":true,"config":{"mode":"on"}}]}}}).to_string());
        assert_eq!(p.request.mode,PipelineMode::Process);assert!(!p.request.chain[0].enabled);assert!(p.has("builtin.request.requestHeaders",true));
        let p=resolve_pipeline(&json!({"pipeline":{"version":2,"request":{"mode":"process","chain":[{"id":"builtin.request.inlineRemoteImages","enabled":true,"config":{"vendor":"keep"}},{"id":"builtin.request.imageProcessing","enabled":false,"config":{"inline_remote":false}}]}}}).to_string());
        assert_eq!(p.request.chain.len(),1);assert!(!p.request.chain[0].enabled);assert_eq!(p.request.chain[0].config["legacy_plugins"][0]["config"]["vendor"],"keep");
    }

    #[test]
    fn header_fields_reject_invalid_rows_and_transport_overrides(){
        let d=crate::services::builtin_plugins::get("builtin.request.requestHeaders").unwrap();
        for config in [json!({"headers":[{"name":"X-Bad","value":"a\r\nb: c","operation":"set"}]}),json!({"headers":[{"name":"authorization","value":"bad","operation":"set"}]})]{assert!(crate::services::plugin_contract::validate_config(&d,&config).is_err());}
        for operation in ["fill","set","remove"] {assert!(crate::services::plugin_contract::validate_config(&d,&json!({"headers":[{"name":"Content-Encoding","value":"br","operation":operation}]})).is_err());}
        assert!(crate::services::plugin_contract::validate_config(&d,&json!({"headers":[{"name":"X-Relay","value":"ok","operation":"fill"}]})).is_ok());
        let d=crate::services::builtin_plugins::get("builtin.request.imageProcessing").unwrap();
        assert!(crate::services::plugin_contract::validate_config(&d,&json!({"max_images":0})).is_err());
    }

    #[test]
    fn header_image_migration_is_idempotent_and_does_not_resurrect_removed_headers(){
        let c=json!({"headers":{"User-Agent":"OldBot/1","X-Relay":"value"},"pipeline":{"version":2,"request":{"mode":"process","chain":[{"id":"builtin.request.inlineRemoteImages","enabled":true,"config":{"vendor":"keep"}},{"id":"builtin.request.imageLimit","enabled":false,"config":{}}]}}}).to_string();
        let mut p=resolve_pipeline(&c);
        assert_eq!(p.request.chain[0].id,"builtin.request.imageProcessing");
        assert_eq!(p.request.chain[0].config["inline_remote"],true);
        assert_eq!(p.request.chain[0].config["trim_old_images"],false);
        assert_eq!(p.request.chain[1].id,"builtin.request.requestHeaders");
        assert_eq!(p.request.chain[1].config["user_agent"],"OldBot/1");
        assert_eq!(resolve_pipeline(&json!({"pipeline":p}).to_string()),p);
        p.request.chain.clear();
        assert!(resolve_pipeline(&json!({"headers":{"User-Agent":"OldBot/1"},"pipeline":p}).to_string()).request.chain.is_empty());
    }

    #[test]
    fn v2_removed_recovery_does_not_inherit_legacy_defaults() {
        let config = json!({"pipeline":{"version":2,"request":{"mode":"process","chain":[]},"response":{"mode":"process","chain":[]},"recovery":{"mode":"process","chain":[]}},"failure_policy":{"retry_count":9,"cooldown_enabled":true}}).to_string();
        let p = resolve_pipeline(&config);
        let effective = failure_policy_from_pipeline(&p, &config);
        assert_eq!(effective.retry_count, 0);
        assert!(!effective.failover_enabled);
        assert!(!effective.cooldown_enabled);
        assert!(!effective.error_status_enabled);
    }

    #[test]
    fn migration_moves_cleanup_without_activating_response_passthrough() {
        let config = json!({"pipeline":{"version":1,"request":{"mode":"process","chain":[]},"response":{"mode":"passthrough","chain":[{"id":"builtin.response.encryptedContentCleanup","enabled":true,"config":{"aggressive_strip":true}}]}}}).to_string();
        let p = resolve_pipeline(&config);
        assert_eq!(p.version, 2);
        assert!(p.response.chain.is_empty());
        assert!(p.request.chain.iter().any(|p| p.id == "builtin.request.encryptedContentCleanup" && !p.enabled && p.config["aggressive_strip"] == true));
        let encoded = json!({"pipeline":p}).to_string();
        assert_eq!(resolve_pipeline(&encoded), p);
    }

    #[test]
    fn migration_preserves_effective_retry_even_when_old_row_was_disabled() {
        let config = json!({"pipeline":{"version":1,"request":{"mode":"process","chain":[{"id":"builtin.schedule.retry","enabled":false,"config":{"retry_count":8}}]},"response":{"mode":"process","chain":[]}}}).to_string();
        let p = resolve_pipeline(&config);
        let policy = failure_policy_from_pipeline(&p, &config);
        assert_eq!(p.version, 2);
        assert_eq!(policy.retry_count, 2);
        assert!(policy.failover_enabled);
    }
}
