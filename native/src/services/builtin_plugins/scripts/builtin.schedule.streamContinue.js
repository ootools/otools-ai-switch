// Schedule plugin: stream continuation after disconnect.
"use strict";
function policy(config, ctx) {
  config = config || {};
  var out = { stream_continue_enabled: true };
  if (typeof config.max === "number") out.stream_continue_max = config.max;
  return out;
}
