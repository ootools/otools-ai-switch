// Schedule plugin: mid-stream failure salvage.
"use strict";
function policy(config, ctx) { return { stream_salvage_enabled: true }; }
