// Port of force_reasoning_content_on_chat_body in responses_chat.rs
"use strict";
var PLACEHOLDER = "tool call";

function onRequest(body, ctx) {
  if (!body || typeof body !== "object" || !Array.isArray(body.messages)) return null;
  var changed = false;
  body.messages.forEach(function (message) {
    if (!message || typeof message !== "object" || message.role !== "assistant") return;
    if (typeof message.reasoning_content === "string" && message.reasoning_content.trim() !== "") return;
    var fallback = (typeof message.reasoning === "string" && message.reasoning.trim() !== "")
      ? message.reasoning : PLACEHOLDER;
    message.reasoning_content = fallback;
    changed = true;
  });
  return changed ? body : null;
}
