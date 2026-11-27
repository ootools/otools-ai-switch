// Schedule plugin: failover to another account in the pool.
"use strict";
function policy(config, ctx) { return { failover_enabled: true }; }
