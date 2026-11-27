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

function onRequest(body, ctx) {
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
