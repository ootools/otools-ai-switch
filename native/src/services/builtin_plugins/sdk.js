"use strict";
(function () {
  function command(type, options) { return Object.assign({}, options || {}, {type:type}); }
  globalThis.host = Object.freeze({
    apiVersion:1,
    transform:function(body,headers){return {__middlewareResult:true,body:body,headers:headers||{}};},
    request:Object.freeze({replace:function(body){return command("request.replace",{body:body});}}),
    codec:Object.freeze({compressBrotli:function(value){return JSON.parse(__host.compressBrotli(value));}}),
    images:Object.freeze({fetchAsDataUrl:function(url){return __host.fetchAsDataUrl(url);}}),
    accounts:Object.freeze({snapshot:function(ctx){return ctx.account || null;},update:function(v){return command("account.update",v);},recordFailure:function(v){return command("account.failure",v);},recordSuccess:function(v){return command("account.success",v);}}),
    retry:Object.freeze({request:function(v){return command("retry",v);},nextAccount:function(v){return command("next_account",v);},stop:function(v){return command("return",v);}}),
    streams:Object.freeze({continueResponse:function(v){return command("continue",v);}}),
    state:Object.freeze({get:function(ctx,key){return ctx.state ? ctx.state[key] : undefined;},set:function(key,value){return command("state.set",{key:key,value:value});},increment:function(key,amount){return command("state.increment",{key:key,amount:amount===undefined?1:amount});}}),
    diagnostics:Object.freeze({log:function(level,message){return command("log",{level:level,message:message});}}),
    errors:Object.freeze({match:function(rules,event){return JSON.parse(__host.matchRules(JSON.stringify(rules),JSON.stringify(event)));}})
  });
  globalThis.__aiSwitchLifecycle = function(id, hook, payload) {
    var input=JSON.parse(payload), fn=(globalThis.__aiSwitchHooks[id]||{})[hook];
    if(typeof fn!=="function") return "{}";
    return JSON.stringify(fn(input.event,input.context)||{});
  };
})();
