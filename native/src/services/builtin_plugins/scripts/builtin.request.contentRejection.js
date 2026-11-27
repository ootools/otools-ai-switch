// Port of native/src/services/request_content_rejection.rs
// Elides old tool outputs after an upstream content-filter rejection.
"use strict";
var PLACEHOLDER = "[ai-switch: older tool output omitted after upstream content filter; re-run the command if needed]";
var KEEP_LATEST = 6;
var MIN_BYTES = 128;
var RESPONSES_TYPES = { function_call_output: 1, custom_tool_call_output: 1, local_shell_call_output: 1 };

function elideValue(output, responsesShape) {
  if (typeof output === "string") {
    if (output.length > MIN_BYTES) return PLACEHOLDER;
    return undefined;
  }
  if (Array.isArray(output) && output.length > 0) {
    var total = 0;
    for (var i = 0; i < output.length; i++) {
      var part = output[i];
      if (!part || typeof part !== "object" || Array.isArray(part)) return undefined;
      var kind = part.type;
      var text = part.text;
      if (!(kind === "text" || kind === "input_text" || kind === "output_text")) return undefined;
      if (typeof text !== "string") return undefined;
      total += text.length;
    }
    if (total > MIN_BYTES) {
      return [{ type: responsesShape ? "input_text" : "text", text: PLACEHOLDER }];
    }
  }
  return undefined;
}

function onRequest(body, ctx) {
  if (!body || typeof body !== "object" || Array.isArray(body)) return null;
  var slots = [];
  if (Array.isArray(body.input)) {
    for (var i = 0; i < body.input.length; i++) {
      var item = body.input[i];
      if (item && typeof item === "object" && RESPONSES_TYPES[item.type]) {
        slots.push({ kind: "responses", index: i });
      }
    }
  }
  if (Array.isArray(body.messages)) {
    for (var j = 0; j < body.messages.length; j++) {
      var message = body.messages[j];
      if (message && typeof message === "object" && message.role === "tool") {
        slots.push({ kind: "chat", index: j });
      }
    }
  }
  var elideUntil = slots.length - KEEP_LATEST;
  if (elideUntil <= 0) return null;
  var changed = false;
  for (var s = 0; s < elideUntil; s++) {
    var slot = slots[s];
    if (slot.kind === "responses") {
      var item = body.input[slot.index];
      if (!item || typeof item.output === "undefined") continue;
      var next = elideValue(item.output, true);
      if (next !== undefined) { item.output = next; changed = true; }
    } else {
      var message = body.messages[slot.index];
      if (!message || typeof message.content === "undefined") continue;
      var nextChat = elideValue(message.content, false);
      if (nextChat !== undefined) { message.content = nextChat; changed = true; }
    }
  }
  return changed ? body : null;
}
