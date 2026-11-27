// Schedule plugin: failure cooldown.
"use strict";
function policy(config, ctx) {
  config = config || {};
  var out = { cooldown_enabled: true };
  if (typeof config.seconds === "number") out.cooldown_seconds = config.seconds;
  return out;
}
