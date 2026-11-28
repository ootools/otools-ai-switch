"use strict";
function policy(config, ctx) {
  if (ctx && ctx.domain !== "stream") return {stream_continue_enabled:config.stream_continue_enabled,stream_continue_max:config.stream_continue_max,stream_salvage_enabled:config.stream_salvage_enabled};
  config = config || {};
  var out = {"stream_continue_enabled": true, "stream_continue_max": 2, "stream_salvage_enabled": true, "retry_count": 2, "retry_interval_ms": 500, "max_retry_elapsed_seconds": 60, "failover_enabled": true};
  if (config["stream_continue_enabled"] !== undefined) out["stream_continue_enabled"] = config["stream_continue_enabled"];
  if (config["stream_continue_max"] !== undefined) out["stream_continue_max"] = config["stream_continue_max"];
  if (config["stream_salvage_enabled"] !== undefined) out["stream_salvage_enabled"] = config["stream_salvage_enabled"];
  if (config["retry_count"] !== undefined) out["retry_count"] = config["retry_count"];
  if (config["retry_interval_ms"] !== undefined) out["retry_interval_ms"] = config["retry_interval_ms"];
  if (config["max_retry_elapsed_seconds"] !== undefined) out["max_retry_elapsed_seconds"] = config["max_retry_elapsed_seconds"];
  if (config["failover_enabled"] !== undefined) out["failover_enabled"] = config["failover_enabled"];
  if (config.retry_class_limits) out.retry_class_limits=config.retry_class_limits;
  if (config.capacity_retry_interval_ms !== undefined) out.capacity_retry_interval_ms=config.capacity_retry_interval_ms;
  return out;
}

function onFailure(event,ctx) {
  if (event.phase !== "stream") return null;
  if (event.transport && ctx.config.stream_continue_enabled) return {actions:[host.streams.continueResponse({class:"transport"})]};
  if (!ctx.config.stream_salvage_enabled) return null;
  var match=host.errors.match(ctx.config.rules || [],event);
  return match.action === "retry" ? {actions:[host.streams.continueResponse(match)]} : {actions:[host.retry.stop(match)]};
}
function onStreamEvent(event,ctx) { return {value:event}; }
