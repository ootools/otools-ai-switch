//! Versioned metadata and identical capability vocabulary for shipped and installed middleware.
use crate::services::builtin_plugins::BuiltinPluginDescriptor;
use serde_json::{json, Value};
const CAPABILITIES: &[&str] = &[
    "request.transform",
    "response.transform",
    "stream.transform",
    "policy.configure",
    "retry.request",
    "stream.continue",
    "account.read",
    "account.write",
    "state.read",
    "state.write",
    "codec.brotli",
    "network.image",
    "diagnostics.log",
];
pub fn validate_descriptor(d: &BuiltinPluginDescriptor) -> Result<(), String> {
    if d.api_version != 1 {
        return Err("Unsupported middleware api_version".into());
    }
    if d.capabilities
        .iter()
        .any(|c| !CAPABILITIES.contains(&c.as_str()))
    {
        return Err("Unknown middleware capability".into());
    }
    if d.hooks.iter().any(|h| {
        ![
            "policy",
            "onRequest",
            "onRequestHeaders",
            "onResponseHeaders",
            "onResponse",
            "onStreamEvent",
            "onFailure",
            "onComplete",
        ]
        .contains(&h.as_str())
    }) {
        return Err("Unknown middleware hook".into());
    }
    if !["", "retry", "stream", "health"].contains(&d.rule_editor.as_str())
        || !["", "retry", "stream", "health"].contains(&d.policy_domain.as_str())
    {
        return Err("Unknown middleware editor/domain".into());
    }
    let stages = [
        "policy",
        "perAttempt",
        "inlineRemote",
        "imageLimit",
        "preBridge",
        "forceReasoning",
        "postBridgeCross",
        "postBridgePlaintext",
        "postBridgeEncrypted",
        "postBridgeReminder",
        "wire",
        "retryRecovery",
        "response",
    ];
    if std::iter::once(&d.stage)
        .chain(d.stages.iter())
        .any(|s| !stages.contains(&s.as_str()))
    {
        return Err("Unknown execution stage".into());
    }
    if !d.defaults.is_null() && !d.defaults.is_object() {
        return Err("Middleware defaults must be an object".into());
    }
    if d.config_fields.len() > 64 || d.hooks.len() > 16 || d.capabilities.len() > 32 {
        return Err("Middleware descriptor exceeds supported limits".into());
    }
    Ok(())
}
pub fn descriptor(id: &str) -> Option<BuiltinPluginDescriptor> {
    crate::services::plugin_middleware::descriptor(id)
}
pub fn config(id: &str, config: &Value) -> Value {
    merge_config(
        &descriptor(id)
            .map(|d| d.defaults)
            .unwrap_or_else(|| json!({})),
        config,
    )
}
pub fn merge_config(defaults: &Value, config: &Value) -> Value {
    let mut merged = defaults.clone();
    if !merged.is_object() {
        merged = json!({});
    }
    if let Some(values) = config.as_object() {
        merged.as_object_mut().unwrap().extend(values.clone());
    }
    if config["legacy_matching"].as_bool() == Some(true) && config.get("rules").is_none() {
        merged.as_object_mut().unwrap().remove("rules");
    }
    merged
}
pub fn stage_matches(id: &str, stage: &str) -> bool {
    descriptor(id).is_some_and(|d| d.stage == stage || d.stages.iter().any(|s| s == stage))
}
pub fn allows(id: &str, capability: &str) -> bool {
    descriptor(id).is_some_and(|d| d.capabilities.iter().any(|c| c == capability))
}

pub fn has_stage(pipeline: &crate::models::pipeline::PipelineConfig, stage: &str) -> bool {
    crate::services::plugin_host::enabled(pipeline)
        .iter()
        .any(|p| stage_matches(&p.id, stage))
}

/// A recovery request is allowed only when a plugin produces a different body.
pub fn recover_request(
    pipeline: &crate::models::pipeline::PipelineConfig,
    status: u16,
    error: &[u8],
    body: &[u8],
    used: &mut std::collections::HashSet<String>,
) -> Option<Vec<u8>> {
    let request: Value = serde_json::from_slice(body).ok()?;
    let event =
        crate::services::failure_rules::event(Some(status), Some(error), false, "request_recovery");
    for p in crate::services::plugin_host::enabled(pipeline) {
        if used.contains(&p.id) {
            continue;
        }
        let outcome = crate::services::plugin_host::invoke(
            &p.id,
            "onFailure",
            &event,
            &json!({"request":request,"output_committed":false}),
            &p.config,
        );
        for action in outcome.actions {
            if action["type"] == "request.replace" && action["body"] != request {
                used.insert(p.id.clone());
                return serde_json::to_vec(&action["body"]).ok();
            }
        }
    }
    None
}

/// Descriptor-driven configuration validation, shared by shipped and external fields.
pub fn validate_config(descriptor:&BuiltinPluginDescriptor,config:&Value)->Result<(),String>{
    let config=merge_config(&descriptor.defaults,config);
    for field in &descriptor.config_fields {
        let Some(value)=config.get(&field.key) else{continue};
        match field.kind.as_str(){
            "userAgent"=>{
                let text=value.as_str().ok_or_else(||format!("{} 必须是文本",field.label))?;
                if !text.is_empty(){axum::http::HeaderValue::from_str(text).map_err(|_|format!("{} 包含无效请求头字符",field.label))?;}
            },
            "headers"=>{
                let rows=value.as_array().ok_or("请求头规则必须是数组")?;
                if rows.len()>64{return Err("最多 64 条请求头规则".into());}
                let mut names=std::collections::HashSet::new();
                for row in rows {
                    let name=row["name"].as_str().unwrap_or("").trim();
                    axum::http::HeaderName::from_bytes(name.as_bytes()).map_err(|_|format!("请求头名称无效：{name}"))?;
                    if !names.insert(name.to_ascii_lowercase()){return Err(format!("请求头重复：{name}"));}
                    let operation=row["operation"].as_str().unwrap_or("fill");
                    if !["fill","set","remove"].contains(&operation){return Err("请求头操作无效".into());}
                    if ["host","content-length","content-encoding","transfer-encoding","connection","accept-encoding"].contains(&name.to_ascii_lowercase().as_str()){return Err(format!("{name} 是连接或编码头，必须由宿主管理"));}
                    if operation!="fill" && ["authorization","proxy-authorization","x-api-key","api-key","host","content-length","content-encoding","transfer-encoding","connection","accept-encoding"].contains(&name.to_ascii_lowercase().as_str()){return Err(format!("{name} 由宿主管理，不能覆盖或移除"));}
                    if operation!="remove" {
                        let text=row["value"].as_str().ok_or("请求头值必须是文本")?;
                        if text.len()>8192{return Err("请求头值不能超过 8192 字节".into());}
                        axum::http::HeaderValue::from_str(text).map_err(|_|format!("请求头 {name} 的值无效"))?;
                    }
                }
            },
            "number"=>{
                if field.min.is_some() || field.max.is_some(){
                    let number=value.as_u64().ok_or_else(||format!("{} 必须是非负整数",field.label))?;
                    if field.min.is_some_and(|min|number<u64::from(min))||field.max.is_some_and(|max|number>u64::from(max)){return Err(format!("{} 超出允许范围",field.label));}
                }
            },
            "bool"=>{if !value.is_boolean(){return Err(format!("{} 必须是开关值",field.label));}},
            _=>{}
        }
    }
    Ok(())
}
