// Port of native/src/services/request_compression.rs
// JS decides applicability; the Brotli codec is a restricted host capability.
"use strict";
var AUTO_HOSTS = ["ps.air-outer.com"];
var AUTO_PATHS = ["/v1/responses", "/v1/chat/completions", "/v1/messages"];

function parseUrl(raw) {
  if (typeof raw !== "string") return null;
  var match = /^([a-zA-Z][a-zA-Z0-9+.-]*):\/\/([^/?#]+)([^?#]*)(\?[^#]*)?(#.*)?$/.exec(raw.trim());
  if (!match) return null;
  var scheme = match[1].toLowerCase();
  var authority = match[2];
  var at = authority.lastIndexOf("@");
  if (at >= 0) authority = authority.substring(at + 1);
  var host = authority.split(":")[0].toLowerCase();
  var portMatch = /:(\d+)$/.exec(authority);
  var port = portMatch ? parseInt(portMatch[1], 10) : (scheme === "https" ? 443 : (scheme === "http" ? 80 : null));
  var path = match[3] || "/";
  while (path.length > 1 && path.charAt(path.length - 1) === "/") path = path.substring(0, path.length - 1);
  return { scheme: scheme, host: host, port: port, path: path };
}

function onRequest(body, ctx) {
  if (!body || typeof body !== "object" || Array.isArray(body)) return null;
  var config = (ctx && ctx.config) || {};
  var mode = config.mode;
  if (mode !== "on" && mode !== "auto") return null;
  var target = parseUrl(ctx && ctx.base_url);
  if (!target || (target.scheme !== "http" && target.scheme !== "https")) return null;
  if (mode === "auto") {
    if (target.scheme !== "https" || target.port !== 443) return null;
    if (AUTO_PATHS.indexOf(target.path) < 0) return null;
    if (AUTO_HOSTS.indexOf(target.host) < 0) return null;
  }
  var headers = (ctx && ctx.headers) || {};
  var contentType = "";
  Object.keys(headers).forEach(function (key) {
    if (key.toLowerCase() === "content-type") contentType = String(headers[key]);
  });
  if (contentType.split(";")[0].trim().toLowerCase() !== "application/json") return null;
  var encodings = [];
  Object.keys(headers).forEach(function (key) {
    if (key.toLowerCase() === "content-encoding") {
      var value = String(headers[key]);
      if (Array.isArray(value)) encodings = encodings.concat(value);
      else encodings.push(value);
    }
  });
  for (var i = 0; i < encodings.length; i++) {
    if (encodings[i].trim().toLowerCase() !== "identity") return null;
  }
  if (typeof ctx.rawBodyBase64 !== "string" || typeof __host !== "object" || typeof __host.compressBrotli !== "function") return null;
  var result = __host.compressBrotli(ctx.rawBodyBase64);
  if (typeof result === "string") { try { result = JSON.parse(result); } catch (e) { return null; } }
  if (!result || typeof result !== "object" || typeof result.__aiSwitchBytes !== "string") return null;
  return result;
}
