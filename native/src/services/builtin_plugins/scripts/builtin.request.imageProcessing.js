// Port of inline_remote_image_urls_in_body in route_proxy_service.rs
// Network fetch is a restricted host capability available to all plugins.
"use strict";
var MAX_COUNT = 16;

function isRemote(url) {
  if (typeof url !== "string") return false;
  var trimmed = url.trim();
  return trimmed.indexOf("http://") === 0 || trimmed.indexOf("https://") === 0;
}

function collect(value, out) {
  if (!value || typeof value !== "object") return;
  if (!Array.isArray(value)) {
    var imageUrl = value.image_url;
    if (typeof imageUrl === "string" && isRemote(imageUrl)) {
      out.push(imageUrl.trim());
    } else if (imageUrl && typeof imageUrl === "object" && typeof imageUrl.url === "string" && isRemote(imageUrl.url)) {
      out.push(imageUrl.url.trim());
    }
  }
  var children = Array.isArray(value) ? value : Object.keys(value).map(function (key) { return value[key]; });
  children.forEach(function (child) { collect(child, out); });
}

function replace(value, replacements) {
  if (!value || typeof value !== "object") return;
  if (!Array.isArray(value)) {
    var imageUrl = value.image_url;
    if (typeof imageUrl === "string" && replacements[imageUrl.trim()]) {
      value.image_url = replacements[imageUrl.trim()];
    } else if (imageUrl && typeof imageUrl === "object" && typeof imageUrl.url === "string" && replacements[imageUrl.url.trim()]) {
      imageUrl.url = replacements[imageUrl.url.trim()];
    }
  }
  var children = Array.isArray(value) ? value : Object.keys(value).map(function (key) { return value[key]; });
  children.forEach(function (child) { replace(child, replacements); });
}

function inlineImages(body, ctx) {
  if (!body || typeof body !== "object") return null;
  if (typeof __host !== "object" || typeof __host.fetchAsDataUrl !== "function") return null;
  var urls = [];
  collect(body, urls);
  urls.sort();
  var unique = [];
  urls.forEach(function (url) { if (unique.indexOf(url) < 0) unique.push(url); });
  unique = unique.slice(0, MAX_COUNT);
  var replacements = {};
  unique.forEach(function (url) {
    var data = __host.fetchAsDataUrl(url);
    if (typeof data === "string" && data !== "") replacements[url] = data;
  });
  if (Object.keys(replacements).length === 0) return null;
  replace(body, replacements);
  return body;
}

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

function trimImages(body, ctx) {
  if (!body || typeof body !== "object" || Array.isArray(body)) return null;
  var total = 0;
  ["messages", "input"].forEach(function (key) {
    if (Array.isArray(body[key])) body[key].forEach(function (entry) { total += countInParts(entry); });
  });
  var limit=ctx && ctx.config && typeof ctx.config.max_images === "number" ? Math.max(1,Math.min(1000,Math.floor(ctx.config.max_images))) : KEEP_LATEST;
  if (total <= limit) return null;
  var state = { budget: total - limit, changed: false };
  ["messages", "input"].forEach(function (key) {
    if (Array.isArray(body[key])) body[key].forEach(function (entry) { replaceInParts(entry, state); });
  });
  return state.changed ? body : null;
}


function onRequest(body,ctx) {
  var config=ctx.config || {};
  if (ctx.stage === "inlineRemote" && config.inline_remote) return inlineImages(body,ctx);
  if (ctx.stage === "imageLimit" && config.trim_old_images) return trimImages(body,ctx);
  return null;
}
function onFailure(event,ctx) {
  if(event.phase!=="request_recovery" || !ctx.config.trim_old_images)return null;
  var message=(event.message||"").toLowerCase();
  if(message.indexOf("maximum number of images")<0 && message.indexOf("too many images")<0)return null;
  var body=trimImages(ctx.request,ctx);
  return body ? {actions:[host.request.replace(body)]}:null;
}
