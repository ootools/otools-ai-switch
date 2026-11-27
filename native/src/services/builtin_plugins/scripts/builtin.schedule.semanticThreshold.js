// Schedule plugin: semantic failure threshold.
"use strict";
function policy(config, ctx) {
  config = config || {};
  if (typeof config.value !== "number") return null;
  return { semantic_error_threshold: config.value };
}
