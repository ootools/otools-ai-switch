//! Shared, declarative error matching used by runtime and the settings preview.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::models::pipeline::{PipelineConfig, PipelineMode};
use crate::models::route_credential::RouteFailureClass;
use crate::services::response_failure_service::{detect_response_failed, SemanticResponseFailure};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FailureCondition {
    pub field: String,
    pub op: String,
    pub value: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FailureRule {
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// OR groups containing AND conditions.
    pub groups: Vec<Vec<FailureCondition>>,
    #[serde(default)]
    pub exclude: Vec<FailureCondition>,
    pub action: String,
    pub class: RouteFailureClass,
}
fn yes() -> bool { true }
#[derive(Debug, Clone, Serialize)]
pub struct FailureDecision {
    pub rule_id: Option<String>,
    pub rule_name: String,
    pub action: String,
    pub class: RouteFailureClass,
    pub observed: Value,
}

fn normalize(s: &str) -> String { s.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase() }
fn matches(c: &FailureCondition, observed: &Value) -> bool {
    if c.field == "code_or_type" { return ["code","type"].iter().any(|f|matches(&FailureCondition{field:(*f).into(),..c.clone()},observed)); }
    if matches!(c.op.as_str(),"equals_any"|"contains_any") {
        return c.value.lines().filter(|line|!line.trim().is_empty()).any(|line|matches(&FailureCondition{field:c.field.clone(),op:c.op.trim_end_matches("_any").into(),value:line.into()},observed));
    }
    let Some(actual) = observed.get(&c.field).filter(|v| !v.is_null()) else { return false; };
    let actual = normalize(actual.as_str().map(str::to_owned).unwrap_or_else(|| actual.to_string()).as_str());
    let expected = normalize(&c.value);
    if expected.is_empty() { return false; }
    match c.op.as_str() {
        "present" => !actual.is_empty(),
        "equals" => actual == expected,
        "contains" => actual.contains(&expected),
        "starts_with" => actual.starts_with(&expected),
        "range" => expected.split_once('-').and_then(|(a,b)| Some((a.trim().parse::<u16>().ok()?, b.trim().parse::<u16>().ok()?, actual.parse::<u16>().ok()?))).is_some_and(|(a,b,x)| a <= x && x <= b),
        _ => false,
    }
}

pub fn defaults() -> Vec<FailureRule> {
    serde_json::from_str(include_str!("builtin_plugins/scripts/failure-rules.json")).expect("valid default rules")
}

/// Visible fallback rules describing the old broad retry setting during migration.
pub fn legacy_defaults() -> Vec<FailureRule> {
    let mut rules=defaults();
    rules.push(FailureRule {id:"legacy-semantic".into(),name:"旧配置：其他结构化错误".into(),enabled:true,groups:vec![vec![FailureCondition {field:"message".into(),op:"present".into(),value:"true".into()}]],exclude:vec![],action:"retry".into(),class:RouteFailureClass::Semantic});
    rules.push(FailureRule {id:"legacy-status".into(),name:"旧配置：其他非成功状态".into(),enabled:true,groups:vec![vec![FailureCondition {field:"status".into(),op:"range".into(),value:"300-599".into()}]],exclude:vec![],action:"retry".into(),class:RouteFailureClass::Other});
    rules
}

pub fn validate(rules: &[FailureRule]) -> Result<(), String> {
    if rules.len() > 64 { return Err("最多 64 条错误规则".into()); }
    let mut ids = std::collections::HashSet::new();
    for r in rules {
        if r.id.is_empty() || !ids.insert(&r.id) || r.name.len() > 200 || !["retry","next_account","return","record","ignore"].contains(&r.action.as_str()) { return Err("规则 ID、名称或动作无效".into()); }
        if r.groups.is_empty() || r.groups.len() > 128 || r.groups.iter().any(|g| g.is_empty() || g.len() > 16) || r.exclude.len() > 64 { return Err("条件组不能为空或过多".into()); }
        for c in r.groups.iter().flatten().chain(r.exclude.iter()) {
            if !["status","code","type","code_or_type","message","transport"].contains(&c.field.as_str()) || !["equals","contains","equals_any","contains_any","starts_with","range","present"].contains(&c.op.as_str()) || c.value.trim().is_empty() || c.value.len() > 1024 { return Err("匹配字段、运算符或内容无效".into()); }
            if c.op == "range" && (c.field != "status" || !c.value.split_once('-').is_some_and(|(a,b)| match (a.trim().parse::<u16>(),b.trim().parse::<u16>()) { (Ok(a),Ok(b)) => (100..=599).contains(&a) && a <= b && b <= 599, _ => false })) { return Err("状态范围必须是 100–599 之间，例如 500-599".into()); }
        }
    }
    Ok(())
}

pub fn evaluate(rules: &[FailureRule], status: Option<u16>, failure: Option<&SemanticResponseFailure>, transport: bool) -> FailureDecision {
    let observed = json!({"status":status,"code":failure.and_then(|f| f.code.as_deref()),"type":failure.and_then(|f| f.error_type.as_deref()),"message":failure.map(|f| f.message.as_str()),"transport":transport});
    let found = rules.iter().find(|r| r.enabled && !r.exclude.iter().any(|c| matches(c,&observed)) && r.groups.iter().any(|g| !g.is_empty() && g.iter().all(|c| matches(c,&observed))));
    match found {
        Some(r) => FailureDecision { rule_id:Some(r.id.clone()),rule_name:r.name.clone(),action:r.action.clone(),class:r.class,observed },
        None => FailureDecision { rule_id:None,rule_name:"未匹配错误规则".into(),action:"return".into(),class:RouteFailureClass::Other,observed },
    }
}

pub fn rules_for(config: &Value) -> Vec<FailureRule> {
    match config.get("rules") {
        Some(raw) => serde_json::from_value(raw.clone()).ok().filter(|r: &Vec<FailureRule>| validate(r).is_ok()).unwrap_or_default(),
        None if config.get("legacy_matching").and_then(Value::as_bool) == Some(true) => legacy_defaults(),
        None => defaults(),
    }
}

/// Protocol constraints cannot be overridden by a broad custom retry rule.
fn permanent(status: Option<u16>, f: Option<&SemanticResponseFailure>) -> bool {
    use crate::services::response_failure_service::*;
    matches!(status,Some(401|403)) || f.is_some_and(|f| is_quota_exhaustion_failure(f) || is_insufficient_permissions_failure(f) || is_content_rejection(f) || is_context_length_failure(f) || is_too_many_images_failure(f)
        || (status.is_some_and(|s| (400..500).contains(&s) && s != 429) && ["invalid_parameter","invalid_request","bad request"].iter().any(|s| normalize(&f.message).contains(s))))
}

pub fn decide(config: &Value, status: Option<u16>, body: Option<&[u8]>, transport: bool) -> FailureDecision {
    let failure = body.and_then(detect_response_failed).or_else(|| {
        status.filter(|s| *s >= 400).and_then(|_| body).and_then(|b| std::str::from_utf8(b).ok()).map(|message| SemanticResponseFailure {code:None,error_type:None,message:message.to_owned()})
    });
    let mut decision = evaluate(&rules_for(config),status,failure.as_ref(),transport);
    if permanent(status,failure.as_ref()) {
        decision.rule_id=Some("protocol-terminal".into()); decision.rule_name="明确的账号或请求错误，不能原样重试".into();decision.action="return".into();
    }
    decision
}

pub fn event(status:Option<u16>,body:Option<&[u8]>,transport:bool,phase:&str)->Value {
    let failure=body.and_then(detect_response_failed).or_else(||status.filter(|s|*s>=400).and_then(|_|body).and_then(|b|std::str::from_utf8(b).ok()).map(|message|SemanticResponseFailure{code:None,error_type:None,message:message.into()}));
    json!({"status":status,"code":failure.as_ref().and_then(|f|f.code.as_deref()),"type":failure.as_ref().and_then(|f|f.error_type.as_deref()),"message":failure.as_ref().map(|f|f.message.as_str()),"transport":transport,"phase":phase,"terminal":permanent(status,failure.as_ref())})
}

pub fn configured_decision(pipeline: &PipelineConfig, phase: &str, status: Option<u16>, body: Option<&[u8]>, transport: bool) -> Option<FailureDecision> {
    let event=event(status,body,transport,phase);
    let outcomes=crate::services::plugin_host::dispatch(pipeline,"onFailure",&event,&json!({"output_committed":phase=="stream"}));
    for (plugin_id,outcome) in outcomes {
        for action in outcome.actions {
            let kind=action["type"].as_str().unwrap_or("");
            if !["retry","next_account","return","continue"].contains(&kind){continue;}
            let terminal=event["terminal"].as_bool().unwrap_or(false);
            crate::services::plugin_host::log(&plugin_id,"info",&json!({"event":"failure_decision","phase":phase,"rule_id":action["rule_id"],"action":if terminal{"return"}else{kind},"class":action["class"]}).to_string());
            return Some(FailureDecision {
                rule_id:action["rule_id"].as_str().map(str::to_owned),
                rule_name:if terminal {"明确的账号或请求错误，不能原样重试".into()} else {action["rule_name"].as_str().unwrap_or("插件恢复动作").into()},
                action:if terminal {"return".into()} else if kind=="continue" {"retry".into()} else {kind.into()},
                class:serde_json::from_value(action.get("class").cloned().unwrap_or(json!("other"))).unwrap_or(RouteFailureClass::Other),observed:event.clone()
            });
        }
    }
    None
}

pub fn constrain(policy: &mut crate::models::route_credential::RouteCredentialFailurePolicy, pipeline: &PipelineConfig, status: Option<u16>, body: Option<&[u8]>, transport: bool) -> Option<FailureDecision> {
    let decision=configured_decision(pipeline,"before_output",status,body,transport)?;
    if decision.action!="retry" {policy.retry_count=0;policy.retry_class_limits=Default::default();}
    if decision.action=="return" {policy.failover_enabled=false;}
    Some(decision)
}

pub fn health_plan(config_json:&str,status:Option<u16>,body:Option<&[u8]>,message:&str)->(crate::models::route_credential::RouteCredentialFailurePolicy,bool) {
    let pipeline=crate::models::pipeline::resolve_pipeline(config_json);
    let mut policy=crate::services::builtin_plugins::effective_policy(config_json);
    policy.cooldown_enabled=false;policy.error_status_enabled=false;
    let mut record=false;
    let mut evt=event(status,body,body.is_none(),"account_failure");
    if evt["message"].is_null(){evt["message"]=json!(message);}
    for (_,outcome) in crate::services::plugin_host::dispatch(&pipeline,"onFailure",&evt,&json!({})) {
        for action in outcome.actions {
            if action["type"]!="account.failure" {continue;}
            record=true;
            if let Some(seconds)=action["cooldown_seconds"].as_u64(){policy.cooldown_enabled=true;policy.cooldown_seconds=seconds.min(86400) as u32;}
            policy.error_status_enabled=action["error_status_enabled"].as_bool().unwrap_or(false);
            policy.semantic_error_threshold=action["threshold"].as_u64().unwrap_or(10).min(1000) as u32;
        }
    }
    (policy,record)
}
pub fn health_policy(config_json:&str,status:Option<u16>,body:Option<&[u8]>,message:&str)->crate::models::route_credential::RouteCredentialFailurePolicy {
    health_plan(config_json,status,body,message).0
}

pub fn health_defaults() -> Vec<FailureRule> {
    vec![FailureRule { id:"health-failure".into(),name:"记录经过代理分类的失败".into(),enabled:true,
        groups: vec![vec![FailureCondition {field:"message".into(),op:"present".into(),value:"true".into()}]],exclude:vec![],action:"record".into(),class:RouteFailureClass::Semantic }]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rule() -> FailureRule { serde_json::from_value(json!({"id":"busy","name":"暂时繁忙","enabled":true,"groups":[[{"field":"code","op":"equals","value":"UPSTREAM_BUSY"},{"field":"message","op":"contains","value":"try again"}],[{"field":"status","op":"range","value":"500-599"}]],"exclude":[{"field":"message","op":"contains","value":"quota"}],"action":"retry","class":"capacity"})).unwrap() }
    #[test]
    fn http_200_plain_text_is_not_error_but_plain_503_can_match() {
        let config=json!({"rules":[{"id":"words","name":"words","enabled":true,"groups":[[{"field":"message","op":"contains","value":"繁忙"}]],"action":"retry","class":"capacity"}]});
        assert_eq!(decide(&config,Some(200),Some("繁忙".as_bytes()),false).action,"return");
        assert_eq!(decide(&config,Some(503),Some("服务繁忙".as_bytes()),false).action,"retry");
        assert_eq!(decide(&config,Some(401),Some("服务繁忙".as_bytes()),false).action,"return");
    }
    #[test]
    fn numeric_error_codes_and_sse_use_same_matcher() {
        let config=json!({"rules":[{"id":"numeric","name":"gateway","groups":[[{"field":"code","op":"equals","value":"12345"}]],"action":"retry","class":"server_error"}]});
        let body=b"data: {\"error\":{\"code\":12345,\"message\":\"busy\"}}\n\n";
        assert_eq!(decide(&config,Some(200),Some(body),false).action,"retry");
    }

    #[test]
    fn matches_status_or_code_and_message_with_exclusions() {
        let f = SemanticResponseFailure {code:Some("upstream_busy".into()),error_type:None,message:"Please TRY  AGAIN later".into()};
        assert_eq!(evaluate(&[rule()],Some(200),Some(&f),false).action,"retry");
        assert_eq!(evaluate(&[rule()],Some(503),None,false).rule_id.as_deref(),Some("busy"));
        let quota = SemanticResponseFailure{message:"quota exhausted".into(),..f};
        assert_eq!(evaluate(&[rule()],Some(503),Some(&quota),false).action,"return");
    }
    #[test]
    fn empty_or_disabled_rule_never_matches() {
        let mut r=rule(); r.groups=vec![vec![]];
        assert_eq!(evaluate(&[r],Some(503),None,false).action,"return");
        let mut r=rule();r.enabled=false;
        assert_eq!(evaluate(&[r],Some(503),None,false).action,"return");
    }
}
