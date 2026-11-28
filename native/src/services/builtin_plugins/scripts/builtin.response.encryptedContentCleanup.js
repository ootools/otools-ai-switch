// Port of native/src/services/responses_encrypted_content.rs
// Conservative mode keeps well-formed Fernet envelopes; aggressive strips all.
"use strict";
var MAX_LEN = 32 * 1024 * 1024;
var B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

function decodeBase64Url(input) {
  var out = [];
  var buffer = 0;
  var bits = 0;
  for (var i = 0; i < input.length; i++) {
    if (input[i] === "=") break;
    var value = B64.indexOf(input[i]);
    if (value < 0) return null;
    buffer = (buffer << 6) | value;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      out.push((buffer >> bits) & 0xff);
    }
  }
  return out;
}

function hasValidCiphertextShape(content) {
  if (typeof content !== "string") return false;
  if (content.length === 0 || content.length > MAX_LEN) return false;
  if (content.substring(0, 5) !== "gAAAA") return false;
  if (content.trim() !== content) return false;
  var decoded = decodeBase64Url(content);
  if (!decoded || decoded.length < 73) return false;
  return decoded[0] === 0x80 && ((decoded.length - 57) % 16) === 0;
}

function sanitize(body, aggressive) {
  if (!body || typeof body !== "object" || !Array.isArray(body.input)) return null;
  var stripOrphanIds = body.store !== true;
  var changed = false;
  body.input.forEach(function (item) {
    if (!item || typeof item !== "object" || Array.isArray(item)) return;
    var type = typeof item.type === "string" ? item.type.trim() : "";
    if (type !== "reasoning") return;
    if (!aggressive && hasValidCiphertextShape(item.encrypted_content)) return;
    if (Object.prototype.hasOwnProperty.call(item, "encrypted_content")) {
      delete item.encrypted_content;
      changed = true;
    }
    if (stripOrphanIds && Object.prototype.hasOwnProperty.call(item, "id")) {
      delete item.id;
      changed = true;
    }
  });
  return changed ? body : null;
}

function onRequest(body, ctx) {
  var config = (ctx && ctx.config) || {};
  return sanitize(body, config.aggressive_strip === true);
}

function onResponse(body, ctx) { return onRequest(body, ctx); }

function onFailure(event,ctx) {
  if(event.phase!=="request_recovery")return null;
  var code=(event.code||"").toLowerCase().replace(/[_ :\-]/g,"");
  var text=(event.message||"").toLowerCase().replace(/[_`]/g," ");
  if(code!=="invalidencryptedcontent" && !(text.indexOf("encrypted content")>=0 && /verif|decrypt|invalid/.test(text)))return null;
  var body=onRequest(ctx.request,ctx);
  return body ? {actions:[host.request.replace(body)]}:null;
}
