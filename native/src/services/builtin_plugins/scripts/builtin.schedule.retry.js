// Schedule plugin: retry policy declaration.
"use strict";
function policy(config, ctx) {
  config = config || {};
  var out = {};
  if (typeof config.retry_count === "number") out.retry_count = config.retry_count;
  if (typeof config.retry_interval_ms === "number") out.retry_interval_ms = config.retry_interval_ms;
  if (typeof config.max_retry_elapsed_seconds === "number") out.max_retry_elapsed_seconds = config.max_retry_elapsed_seconds;
  if (typeof config.capacity_retry_interval_ms === "number") out.capacity_retry_interval_ms = config.capacity_retry_interval_ms;
  if (config.retry_class_limits && typeof config.retry_class_limits === "object") out.retry_class_limits = config.retry_class_limits;
  return Object.keys(out).length ? out : null;
}
