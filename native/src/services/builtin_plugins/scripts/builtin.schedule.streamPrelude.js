// Schedule plugin: stream prelude wait.
"use strict";
function policy(config, ctx) {
  config = config || {};
  if (typeof config.ms !== "number") return null;
  return { stream_prelude_wait_ms: config.ms };
}
