// Port of native/src/services/responses_plaintext_reasoning.rs::normalize
// Host-name matching lives in createDefaultPipeline (frontend); this is the
// body rewrite used when the plugin is enabled.
"use strict";

function onResponse(body, ctx) {
  // This plugin rewrites the replayed request body, so it is invoked with the
  // onRequest hook at the postBridge stage; keep an alias for clarity.
  return normalize(body);
}

function normalize(body) {
  if (!body || typeof body !== "object" || !Array.isArray(body.input)) return null;
  var changed = false;
  body.input.forEach(function (item) {
    if (!item || typeof item !== "object" || item.type !== "reasoning") return;
    if (!Array.isArray(item.content) || item.content.length === 0) return;
    var texts = [];
    for (var i = 0; i < item.content.length; i++) {
      var part = item.content[i];
      if (!part || typeof part !== "object" || Array.isArray(part)) return;
      var keys = Object.keys(part);
      if (keys.length !== 2 || part.type !== "reasoning_text" || typeof part.text !== "string") return;
      texts.push(part.text);
    }
    var summary;
    if (!Object.prototype.hasOwnProperty.call(item, "summary")) {
      summary = [];
    } else if (Array.isArray(item.summary) && item.summary.every(function (part) {
      return part && typeof part === "object" && part.type === "summary_text" && typeof part.text === "string";
    })) {
      summary = item.summary.slice();
    } else {
      return;
    }
    var existing = {};
    summary.forEach(function (part) { existing[part.text] = true; });
    var fullText = texts.join("\n");
    var extra;
    if (existing[fullText]) {
      extra = "";
    } else {
      extra = texts.filter(function (text) { return !existing[text]; }).join("\n");
    }
    if (extra !== "") summary.push({ type: "summary_text", text: extra });
    item.summary = summary;
    delete item.content;
    changed = true;
  });
  return changed ? body : null;
}

function hostMatches(base) {
  if (typeof base !== "string") return false;
  var m = /^https?:\/\/([^\/?#:]+)/i.exec(base.trim());
  return !!m && m[1].toLowerCase() === "anyrouter.top";
}

function onRequest(body, ctx) {
  var config = (ctx && ctx.config) || {};
  var mode = config.mode || "auto";
  if (mode === "off") return null;
  if (mode === "auto" && !hostMatches(ctx && ctx.base_url)) return null;
  return normalize(body);
}

