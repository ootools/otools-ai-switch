"use strict";
var RESERVED = /^(authorization|proxy-authorization|x-api-key|api-key|host|content-length|content-encoding|transfer-encoding|connection|accept-encoding)$/i;
function onRequestHeaders(event, ctx) {
  var config = ctx.config || {};
  var headers = Object.assign({}, event.headers || {});
  function existing(name) { return Object.keys(headers).filter(function(k){return k.toLowerCase() === name.toLowerCase();}); }
  var patch={};
  function put(name, value) { existing(name).forEach(function(k){delete headers[k];});headers[name]=value;patch[name]=value; }
  (Array.isArray(config.headers) ? config.headers : []).forEach(function(row) {
    if (!row || typeof row.name !== "string" || row.enabled === false) return;
    var name=row.name.trim(),value=typeof row.value === "string" ? row.value.trim() : "";
    if (!name || /^(host|content-length|content-encoding|transfer-encoding|connection|accept-encoding)$/i.test(name)) return;
    var operation=row.operation || "fill";
    if (event.phase === "before_auth") {
      if (operation === "fill" && !existing(name).length && value) put(name,value);
    } else if (!RESERVED.test(name)) {
      if (operation === "remove") put(name,null);
      if (operation === "set" && value) put(name,value);
    }
  });
  if (typeof config.user_agent === "string" && config.user_agent.trim()) put("user-agent",config.user_agent.trim());
  return {value:{headers:patch}};
}
