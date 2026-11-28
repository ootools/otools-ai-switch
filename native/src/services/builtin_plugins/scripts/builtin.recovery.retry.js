"use strict";
function policy(config, ctx) {
  if (ctx && ctx.domain !== "request") return null;
  config = config || {};
  var out = {"retry_count": 2, "retry_interval_ms": 200, "max_retry_elapsed_seconds": 0, "capacity_retry_interval_ms": 0, "failover_enabled": true, "stream_prelude_wait_ms": 60000};
  if (config["retry_count"] !== undefined) out["retry_count"] = config["retry_count"];
  if (config["retry_interval_ms"] !== undefined) out["retry_interval_ms"] = config["retry_interval_ms"];
  if (config["max_retry_elapsed_seconds"] !== undefined) out["max_retry_elapsed_seconds"] = config["max_retry_elapsed_seconds"];
  if (config["capacity_retry_interval_ms"] !== undefined) out["capacity_retry_interval_ms"] = config["capacity_retry_interval_ms"];
  if (config["failover_enabled"] !== undefined) out["failover_enabled"] = config["failover_enabled"];
  if (config["stream_prelude_wait_ms"] !== undefined) out["stream_prelude_wait_ms"] = config["stream_prelude_wait_ms"];
  if (config.retry_class_limits) out.retry_class_limits=config.retry_class_limits;
  if (config.capacity_retry_interval_ms !== undefined) out.capacity_retry_interval_ms=config.capacity_retry_interval_ms;
  return out;
}

function onFailure(event, ctx) {
  if (event.phase !== "before_output") return null;
  if (ctx.config.legacy_matching && !ctx.config.rules) return null;
  var match = host.errors.match(ctx.config.rules || [], event);
  var action = match.action === "retry" ? host.retry.request(match) : match.action === "next_account" ? host.retry.nextAccount(match) : host.retry.stop(match);
  return {actions:[action]};
}
