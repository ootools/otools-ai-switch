// Port of native/src/services/responses_cross_resource_strip.rs::strip
// Removes resource-bound state from replayed Responses requests.
"use strict";

var AUTO_HOSTS = ["agentrouter.org", "ps.air-outer.com"];

function autoMatches(base) {
  if (typeof base !== "string") return false;
  var m = /^https?:\/\/([^\/?#:]+)/i.exec(base.trim());
  return !!m && AUTO_HOSTS.indexOf(m[1].toLowerCase()) >= 0;
}

function onRequest(body, ctx) {
  var config = (ctx && ctx.config) || {};
  var mode = config.mode || "auto";
  if (mode === "off") return null;
  if (mode === "auto" && !autoMatches(ctx && ctx.base_url)) return null;
  if (!body || typeof body !== "object" || Array.isArray(body)) return null;
  var changed = false;
  if (Object.prototype.hasOwnProperty.call(body, "previous_response_id")) {
    delete body.previous_response_id;
    changed = true;
  }
  if (Array.isArray(body.input)) {
    body.input = body.input.filter(function (item) {
      if (!item || typeof item !== "object" || Array.isArray(item)) return true;
      var hadId = Object.prototype.hasOwnProperty.call(item, "id");
      if (hadId) { delete item.id; changed = true; }
      Object.keys(item).forEach(function (key) {
        if (key.length >= 9 && key.substring(0, 9).toLowerCase() === "encrypted") {
          delete item[key];
          changed = true;
        }
      });
      var bareReference = hadId && Object.keys(item).length === 1 && item.type === "item_reference";
      if (bareReference) changed = true;
      return !bareReference;
    });
  }
  return changed ? body : null;
}

function onResponse(body, ctx) { return onRequest(body, ctx); }

