// Schedule plugin: write failure status back to account state.
"use strict";
function policy(config, ctx) { return { error_status_enabled: true }; }
