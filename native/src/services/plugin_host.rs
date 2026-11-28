//! Uniform lifecycle hooks and deferred, capability-checked host commands.
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
#[derive(Debug, Default)]
pub struct HookOutcome {
    pub actions: Vec<Value>,
    pub value: Option<Value>,
    pub error: Option<String>,
}
static DIAGNOSTICS: OnceLock<Mutex<VecDeque<Value>>> = OnceLock::new();
pub fn log(plugin: &str, level: &str, message: &str) {
    let mut rows = DIAGNOSTICS
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if rows.len() >= 256 {
        rows.pop_front();
    }
    rows.push_back(json!({"plugin":plugin,"level":level,"message":message.chars().take(4096).collect::<String>(),"time":chrono::Utc::now().to_rfc3339()}));
}
pub fn diagnostics() -> Vec<Value> {
    DIAGNOSTICS
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect()
}
pub fn invoke(id: &str, hook: &str, event: &Value, context: &Value, config: &Value) -> HookOutcome {
    let Some(binding) = crate::services::plugin_middleware::loaded(id) else {
        return HookOutcome::default();
    };
    invoke_bound(&binding, id, hook, event, context, config)
}
pub fn invoke_bound(
    binding: &crate::services::plugin_middleware::LoadedPlugin,
    id: &str,
    hook: &str,
    event: &Value,
    context: &Value,
    config: &Value,
) -> HookOutcome {
    let Some(descriptor) = binding.descriptor.as_ref() else {
        return HookOutcome::default();
    };
    if !descriptor.hooks.iter().any(|h| h == hook) {
        return HookOutcome::default();
    }
    let mut ctx = context.clone();
    if !ctx.is_object() {
        ctx = json!({});
    }
    ctx["config"] = crate::services::plugin_contract::merge_config(&descriptor.defaults, config);
    ctx["plugin_id"] = json!(id);
    ctx["phase"] = json!(hook);
    let result =
        crate::services::plugin_middleware::lifecycle_bound(&binding, id, hook, event, &ctx);
    let Some(result) = result else {
        log(
            id,
            "error",
            "Hook failed or exceeded budget; no actions committed",
        );
        return HookOutcome {
            error: Some("hook_failed".into()),
            ..Default::default()
        };
    };
    let actions = result
        .get("actions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if actions.len() > 32 {
        return HookOutcome {
            error: Some("too_many_actions".into()),
            ..Default::default()
        };
    }
    for action in &actions {
        let kind = action["type"].as_str().unwrap_or("");
        let phase = event["phase"].as_str().unwrap_or("");
        let can_write = hook == "onComplete" || (hook == "onFailure" && phase == "account_event");
        if kind == "account.failure" && phase != "account_failure" {
            let message="recordFailure belongs to the account_failure settlement phase; use account.update for explicit state changes";
            log(id, "error", message);
            return HookOutcome {
                error: Some(message.into()),
                ..Default::default()
            };
        }
        if (kind.starts_with("account.") || kind.starts_with("state."))
            && !can_write
            && !(kind == "account.failure" && phase == "account_failure")
        {
            let message="Persistent actions are only valid in account_event/onComplete; no actions committed";
            log(id, "error", message);
            return HookOutcome {
                error: Some(message.into()),
                ..Default::default()
            };
        }
        if kind == "request.replace" && phase != "request_recovery" {
            return HookOutcome {
                error: Some(
                    "Request replacement is only valid before output in request_recovery".into(),
                ),
                ..Default::default()
            };
        }
        if ["retry", "next_account", "return", "continue"].contains(&kind)
            && !["before_output", "stream"].contains(&phase)
        {
            let message = "Recovery actions are only valid in before_output/stream phases";
            log(id, "error", message);
            return HookOutcome {
                error: Some(message.into()),
                ..Default::default()
            };
        }
        if let Err(error) = validate_action(&descriptor.capabilities, hook, action) {
            log(id, "error", &error);
            return HookOutcome {
                error: Some(error),
                ..Default::default()
            };
        }
    }
    for action in &actions {
        if action["type"] == "log" {
            log(
                id,
                action["level"].as_str().unwrap_or("info"),
                action["message"].as_str().unwrap_or(""),
            );
        }
    }
    let capability = match hook {
        "onRequest" | "onRequestHeaders" => "request.transform",
        "onResponse" | "onResponseHeaders" => "response.transform",
        "onStreamEvent" => "stream.transform",
        _ => "",
    };
    let value = if descriptor.capabilities.iter().any(|c| c == capability) {
        result.get("value").cloned()
    } else {
        None
    };
    HookOutcome {
        actions,
        value,
        error: None,
    }
}
pub fn validate_action(capabilities: &[String], hook: &str, action: &Value) -> Result<(), String> {
    let kind = action
        .get("type")
        .and_then(Value::as_str)
        .ok_or("Action has no type")?;
    let capability = match kind {
        "request.replace" => "request.transform",
        "retry" | "next_account" | "return" => "retry.request",
        "continue" => "stream.continue",
        "account.update" | "account.failure" | "account.success" => "account.write",
        "state.set" | "state.increment" => "state.write",
        "log" => "diagnostics.log",
        _ => return Err(format!("Unknown host action: {kind}")),
    };
    let allowed: &[&str] = match kind {
        "account.update" => &["type", "status", "cooldown_seconds"],
        "account.failure" => &[
            "type",
            "cooldown_seconds",
            "error_status_enabled",
            "threshold",
            "rule_id",
            "rule_name",
            "message",
        ],
        "account.success" => &["type"],
        "state.set" => &["type", "key", "value"],
        "state.increment" => &["type", "key", "amount"],
        "request.replace" => &["type", "body"],
        "log" => &["type", "level", "message"],
        _ => &[
            "type",
            "rule_id",
            "rule_name",
            "action",
            "class",
            "observed",
        ],
    };
    if action
        .as_object()
        .is_some_and(|a| a.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err("Unknown action option; no options are silently ignored".into());
    }
    if !capabilities.iter().any(|c| c == capability) {
        return Err(format!("Missing declared capability: {capability}"));
    }
    if action.get("account_id").is_some() || action.get("plugin_id").is_some() {
        return Err("Action target is bound by host context".into());
    }
    if ["retry", "next_account", "continue", "return"].contains(&kind) && hook != "onFailure" {
        return Err("Recovery action is invalid in this phase".into());
    }
    if let Some(s) = action.get("status") {
        if !["ok", "error"].iter().any(|v| s.as_str() == Some(v)) {
            return Err("Plugin status must be ok or error".into());
        }
    }
    if action
        .get("cooldown_seconds")
        .filter(|v| !v.is_null())
        .is_some_and(|v| !v.as_u64().is_some_and(|v| v <= 86400))
    {
        return Err("cooldown_seconds must be 0..86400".into());
    }
    if kind == "request.replace" && (!action["body"].is_object() || hook != "onFailure") {
        return Err("request.replace requires a JSON object from onFailure".into());
    }
    if action
        .get("threshold")
        .is_some_and(|v| !v.as_u64().is_some_and(|v| (1..=1000).contains(&v)))
    {
        return Err("Failure threshold must be 1..1000".into());
    }
    if action
        .get("error_status_enabled")
        .is_some_and(|v| !v.is_boolean())
    {
        return Err("error_status_enabled must be boolean".into());
    }
    if action.get("class").is_some_and(|v| {
        serde_json::from_value::<crate::models::route_credential::RouteFailureClass>(v.clone())
            .is_err()
    }) {
        return Err("Unknown recovery class".into());
    }
    if kind.starts_with("state.") {
        if !action["key"]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= 128)
        {
            return Err("Invalid state key".into());
        }
        if action.to_string().len() > 65536 {
            return Err("State value exceeds 64KiB".into());
        }
        if kind == "state.increment"
            && !action["amount"]
                .as_i64()
                .is_some_and(|n| (-1000000..=1000000).contains(&n))
        {
            return Err("Invalid counter increment".into());
        }
    }
    Ok(())
}
pub fn enabled(
    pipeline: &crate::models::pipeline::PipelineConfig,
) -> Vec<&crate::models::pipeline::PluginInstance> {
    [&pipeline.request, &pipeline.response, &pipeline.recovery]
        .into_iter()
        .filter(|lane| lane.mode != crate::models::pipeline::PipelineMode::Passthrough)
        .flat_map(|lane| lane.chain.iter().filter(|p| p.enabled))
        .collect()
}
pub fn dispatch(
    pipeline: &crate::models::pipeline::PipelineConfig,
    hook: &str,
    event: &Value,
    context: &Value,
) -> Vec<(String, HookOutcome)> {
    let mut value = event.clone();
    let lanes: Vec<_> = match hook {
        "onRequest" | "onRequestHeaders" => vec![&pipeline.request],
        "onResponse" | "onResponseHeaders" | "onStreamEvent" => {
            vec![&pipeline.response, &pipeline.recovery]
        }
        _ => vec![&pipeline.request, &pipeline.response, &pipeline.recovery],
    };
    let mut seen = std::collections::HashSet::new();
    lanes
        .into_iter()
        .filter(|l| l.mode != crate::models::pipeline::PipelineMode::Passthrough)
        .flat_map(|l| l.chain.iter().filter(|p| p.enabled))
        .filter(|p| seen.insert(p.id.clone()))
        .map(|p| {
            let outcome = invoke(&p.id, hook, &value, context, &p.config);
            if let Some(next) = &outcome.value {
                if matches!(hook,"onRequestHeaders"|"onResponseHeaders") {
                    if let Some(patch)=next.get("headers").and_then(Value::as_object){
                        let headers=value["headers"].as_object_mut();
                        if let Some(headers)=headers {for (name,v) in patch {headers.retain(|k,_|!k.eq_ignore_ascii_case(name));if !v.is_null(){headers.insert(name.clone(),v.clone());}}}
                    }
                } else {value=next.clone();}
            }
            (p.id.clone(), outcome)
        })
        .collect()
}
