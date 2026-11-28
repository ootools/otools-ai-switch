"use strict";
function policy(config, ctx) {
  if (ctx && ctx.domain !== "request") return null;
  config = config || {};
  var out = {"cooldown_enabled": false, "cooldown_seconds": 10, "semantic_error_threshold": 10, "error_status_enabled": true};
  if (config["cooldown_enabled"] !== undefined) out["cooldown_enabled"] = config["cooldown_enabled"];
  if (config["cooldown_seconds"] !== undefined) out["cooldown_seconds"] = config["cooldown_seconds"];
  if (config["semantic_error_threshold"] !== undefined) out["semantic_error_threshold"] = config["semantic_error_threshold"];
  if (config["error_status_enabled"] !== undefined) out["error_status_enabled"] = config["error_status_enabled"];
  if (config.retry_class_limits) out.retry_class_limits=config.retry_class_limits;
  if (config.capacity_retry_interval_ms !== undefined) out.capacity_retry_interval_ms=config.capacity_retry_interval_ms;
  return out;
}

function onFailure(event,ctx) {
  if (event.phase !== "account_failure") return null;
  var match=ctx.config.rules ? host.errors.match(ctx.config.rules,event) : {action:"record"};
  if (match.action !== "record") return null;
  return {actions:[host.accounts.recordFailure({
    cooldown_seconds:ctx.config.cooldown_enabled ? ctx.config.cooldown_seconds : null,
    error_status_enabled:ctx.config.error_status_enabled,
    threshold:Math.max(1,ctx.config.semantic_error_threshold),
    rule_id:match.rule_id, rule_name:match.rule_name
  })]};
}
