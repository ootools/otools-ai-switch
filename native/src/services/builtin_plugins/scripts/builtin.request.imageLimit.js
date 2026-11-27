// Port of native/src/services/request_image_limit.rs
"use strict";
var PLACEHOLDER = "[ai-switch: an earlier image was omitted because this upstream limits images per request; send it again if it is still needed]";
var KEEP_LATEST = 10;
var IMAGE_TYPES = { input_image: 1, image_url: 1, image: 1 };

function isImagePart(part) {
  return part && typeof part === "object" && IMAGE_TYPES[part.type] === 1;
}

function countInParts(entry) {
  var total = 0;
  if (!entry || typeof entry !== "object") return 0;
  ["content", "output"].forEach(function (key) {
    if (Array.isArray(entry[key])) {
      entry[key].forEach(function (part) { if (isImagePart(part)) total++; });
    }
  });
  return total;
}

function replaceInParts(entry, state) {
  if (!entry || typeof entry !== "object") return;
  ["content", "output"].forEach(function (key) {
    if (state.budget === 0) return;
    if (!Array.isArray(entry[key])) return;
    entry[key].forEach(function (part, index) {
      if (state.budget === 0) return;
      if (!isImagePart(part)) return;
      entry[key][index] = {
        type: part.type === "input_image" ? "input_text" : "text",
        text: PLACEHOLDER
      };
      state.budget--;
      state.changed = true;
    });
  });
}

function onRequest(body, ctx) {
  if (!body || typeof body !== "object" || Array.isArray(body)) return null;
  var total = 0;
  ["messages", "input"].forEach(function (key) {
    if (Array.isArray(body[key])) body[key].forEach(function (entry) { total += countInParts(entry); });
  });
  if (total <= KEEP_LATEST) return null;
  var state = { budget: total - KEEP_LATEST, changed: false };
  ["messages", "input"].forEach(function (key) {
    if (Array.isArray(body[key])) body[key].forEach(function (entry) { replaceInParts(entry, state); });
  });
  return state.changed ? body : null;
}
