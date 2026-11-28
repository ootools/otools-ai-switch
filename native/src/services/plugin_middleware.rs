//! Gateway middleware plugins (Magpie-style request/response hooks).
//!
//! Every plugin — built-in or third-party pack — is a `middleware.js` script
//! loaded into isolated QuickJS contexts and keyed by plugin id:
//!
//! ```js
//! function onRequest(body, ctx) { return body; }
//! function onResponse(body, ctx) { return body; }
//! function onStreamEvent(event, ctx) { return {value:event}; }
//! function policy(config, ctx) { return { retry_count: 3 }; }
//! ```
//!
//! Hooks exchange plain JSON. A hook may return a tagged object
//! (`{ __aiSwitchBytes, __aiSwitchHeaders }`) to hand back raw wire bytes and
//! header changes; every other return value is treated as the new JSON body.
//! A small `__host` API exposes only capabilities JS cannot provide itself
//! (Brotli codec, image fetch, logging). Errors and timeouts fail open.

use crate::error::AppError;
use crate::models::pipeline::PipelineConfig;
use crate::paths::AppPaths;
use base64::Engine as _;
use rquickjs::{Context, Function, Object, Runtime};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

const HOOK_TIMEOUT: Duration = Duration::from_millis(50);
const MIDDLEWARE_FILE: &str = "middleware.js";
const MAX_SCRIPT_BYTES: usize = 256 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);
const FETCH_MAX_BYTES: usize = 16 * 1024 * 1024;

static MIDDLEWARE: OnceLock<RwLock<(u64, PluginMiddleware)>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(1);
static BUILD_LOCK: Mutex<()> = Mutex::new(());
static PACKAGE_LOCK: Mutex<()> = Mutex::new(());
pub fn with_package_lock<T>(work: impl FnOnce() -> T) -> T {
    let _guard = PACKAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    work()
}
static PATHS: OnceLock<AppPaths> = OnceLock::new();

/// Result of one hook invocation. `bytes` overrides the JSON body on the wire;
/// `headers` are applied by the caller (None means remove the header).
#[derive(Debug, Default)]
pub struct HookResult {
    pub body: Value,
    pub bytes: Option<Vec<u8>>,
    pub headers: Vec<(String, Option<String>)>,
}

/// Result of running one chain stage.
#[derive(Debug, Default)]
pub struct ChainOutcome {
    pub bytes: Vec<u8>,
    pub headers: Vec<(String, Option<String>)>,
}

#[derive(Clone, Default)]
pub struct PluginMiddleware {
    plugins: std::collections::HashMap<String, Arc<LoadedPlugin>>,
}
pub struct LoadedPlugin {
    pub descriptor: Option<crate::services::builtin_plugins::BuiltinPluginDescriptor>,
    engine: Mutex<PluginEngine>,
}
struct PluginEngine {
    context: Context,
    runtime: Runtime,
}
impl LoadedPlugin {
    fn invoke(&self, name: &str, a: String, b: String, c: String) -> Option<String> {
        blocking_host(|| self.invoke_blocking(name, a, b, c))
    }
    fn invoke_blocking(&self, name: &str, a: String, b: String, c: String) -> Option<String> {
        let engine = self.engine.lock().unwrap_or_else(|e| e.into_inner());
        let deadline = Instant::now() + HOOK_TIMEOUT;
        engine
            .runtime
            .set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
        let result = engine.context.with(|ctx| {
            let function: Function = ctx.globals().get(name).ok()?;
            function.call((a, b, c)).ok()
        });
        engine.runtime.set_interrupt_handler(None);
        result
    }
}

fn app_error(code: &'static str, message: String) -> AppError {
    AppError::Validation {
        code,
        message,
        details: None,
        recoverable: true,
    }
}

/// Resolve the plugin data dir once; proxy state does not carry paths.
pub fn current_paths() -> &'static AppPaths {
    PATHS.get_or_init(|| {
        AppPaths::resolve()
            .unwrap_or_else(|_| AppPaths::from_data_dir(std::path::PathBuf::from(".")))
    })
}

pub fn ensure_ready() {
    PluginMiddleware::ensure_configured(current_paths());
}

/// Relinquish a multithread Tokio worker before acquiring blocking JS locks or
/// entering synchronous host calls. The runtime can schedule unrelated futures.
fn blocking_host<T>(work: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(tokio::runtime::RuntimeFlavor::MultiThread) => tokio::task::block_in_place(work),
        _ => work(),
    }
}

fn registry() -> &'static RwLock<(u64, PluginMiddleware)> {
    MIDDLEWARE.get_or_init(|| RwLock::new((0, PluginMiddleware::default())))
}
pub fn middleware() -> PluginMiddleware {
    registry()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .1
        .clone()
}
pub fn loaded(id: &str) -> Option<Arc<LoadedPlugin>> {
    ensure_ready();
    middleware().plugins.get(id).cloned()
}
pub fn descriptor(id: &str) -> Option<crate::services::builtin_plugins::BuiltinPluginDescriptor> {
    loaded(id).and_then(|p| p.descriptor.clone())
}
/// Serialized with publication: an older build can never mark a newer generation ready.
pub fn mark_dirty() {
    let _publish = registry().write().unwrap_or_else(|e| e.into_inner());
    GENERATION.fetch_add(1, Ordering::AcqRel);
    crate::services::builtin_plugins::mark_policy_dirty();
}

fn publish_generation(generation: u64, next: PluginMiddleware) -> bool {
    let mut registry = registry().write().unwrap_or_else(|e| e.into_inner());
    if generation != GENERATION.load(Ordering::Acquire) {
        return false;
    }
    *registry = (generation, next);
    true
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_multicast()
                || v.is_documentation()
                || o[0] == 0
                || o[0] >= 240
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 198 && (18..=19).contains(&o[1])))
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            !(v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || v.is_unique_local()
                || v.is_unicast_link_local()
                || v.segments()[0] == 0x2001 && v.segments()[1] == 0xdb8)
        }
    }
}
fn approved_addresses(addresses: Vec<SocketAddr>) -> Option<Vec<SocketAddr>> {
    (!addresses.is_empty() && addresses.iter().all(|a| is_public_ip(a.ip()))).then_some(addresses)
}
async fn read_image_limited(mut response: reqwest::Response, limit: usize) -> Option<String> {
    if !response.status().is_success()
        || response.content_length().is_some_and(|n| n > limit as u64)
    {
        return None;
    }
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").trim().to_lowercase());
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return None;
    }
    let mime = sniff_image_mime(&bytes).or_else(|| mime.filter(|m| m.starts_with("image/")))?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}
/// DNS, connection and streaming are all bounded by one deadline. Connect only
/// to the validated IP snapshot; redirects and ambient proxies are disabled.
async fn fetch_public_image(raw: String) -> Option<String> {
    let url = url::Url::parse(&raw).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let host = url.host_str()?.trim_matches(['[', ']']).to_owned();
    let port = url.port_or_known_default()?;
    let addresses = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::net::lookup_host((host.as_str(), port))
            .await
            .ok()?
            .collect()
    };
    let addresses = approved_addresses(addresses)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(FETCH_TIMEOUT)
        .resolve_to_addrs(&host, &addresses)
        .build()
        .ok()?;
    read_image_limited(client.get(url).send().await.ok()?, FETCH_MAX_BYTES).await
}
fn fetch_image_data_url(url: String) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()
            .and_then(|rt| {
                rt.block_on(async {
                    tokio::time::timeout(FETCH_TIMEOUT, fetch_public_image(url))
                        .await
                        .ok()
                        .flatten()
                })
            });
        let _ = tx.send(result);
    });
    rx.recv_timeout(FETCH_TIMEOUT).ok().flatten()
}

fn sniff_image_mime(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Some("image/png".into());
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg".into());
    }
    if bytes.starts_with(b"GIF8") {
        return Some("image/gif".into());
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp".into());
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp".into());
    }
    None
}

impl PluginMiddleware {
    fn read_sources(
        paths: &AppPaths,
    ) -> (
        Vec<(String, String)>,
        Vec<crate::services::builtin_plugins::BuiltinPluginDescriptor>,
    ) {
        let enabled: BTreeSet<String> = std::fs::read_to_string(&paths.settings_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| {
                v.get("plugins_enabled_json")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut descriptors = crate::services::builtin_plugins::list();
        let mut sources: Vec<(String, String)> =
            crate::services::builtin_plugins::embedded_sources()
                .into_iter()
                .map(|(id, script)| (id.to_string(), script.to_string()))
                .collect();
        for id in enabled {
            if crate::models::plugin::validate_pack_id(&id).is_err() {
                continue;
            }
            let path = paths.plugins_dir.join(&id).join(MIDDLEWARE_FILE);
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if bytes.len() > MAX_SCRIPT_BYTES {
                continue;
            }
            if let Ok(script) = String::from_utf8(bytes) {
                let manifest = std::fs::read(paths.plugins_dir.join(&id).join("plugin.json"))
                    .ok()
                    .and_then(|b| crate::models::plugin::parse_manifest(&b).ok());
                if let Some(d) = manifest
                    .filter(|m| m.id == id)
                    .and_then(|m| m.middleware_descriptor())
                {
                    sources.push((d.id.clone(), script));
                    descriptors.push(d);
                }
            }
        }
        sources.sort_by(|a, b| a.0.cmp(&b.0));
        (sources, descriptors)
    }

    /// Rebuild the JS context from embedded builtins plus enabled third-party
    /// packs. Hooks are stored per plugin id so an account chain can invoke
    /// only the selected ones.
    pub fn configure(paths: &AppPaths) -> Result<(), AppError> {
        blocking_host(|| Self::configure_locked(paths, false))
    }
    fn configure_locked(paths: &AppPaths, lazy: bool) -> Result<(), AppError> {
        let _build = BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if lazy
            && registry().read().unwrap_or_else(|e| e.into_inner()).0
                == GENERATION.load(Ordering::Acquire)
        {
            return Ok(());
        }
        loop {
            let generation = GENERATION.load(Ordering::Acquire);
            let next = Self::build(paths)?;
            if !publish_generation(generation, next) {
                continue;
            }
            crate::services::builtin_plugins::mark_policy_dirty();
            return Ok(());
        }
    }
    fn build(paths: &AppPaths) -> Result<Self, AppError> {
        let mut next = Self::default();
        let (sources, descriptors) = with_package_lock(|| Self::read_sources(paths));
        for (id, script) in sources {
            let runtime = Runtime::new()
                .map_err(|error| app_error("plugin.js_runtime_failed", error.to_string()))?;
            runtime.set_memory_limit(64 * 1024 * 1024);
            let context = Context::full(&runtime)
                .map_err(|error| app_error("plugin.js_context_failed", error.to_string()))?;
            let capabilities = descriptors
                .iter()
                .find(|d| d.id == id)
                .map(|d| d.capabilities.clone())
                .unwrap_or_default();
            let can_compress = capabilities.iter().any(|c| c == "codec.brotli");
            let can_fetch = capabilities.iter().any(|c| c == "network.image");
            let log_id = id.clone();
            let can_log = capabilities.iter().any(|c| c == "diagnostics.log");
            let deadline = Instant::now() + Duration::from_millis(200);
            runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
            let loaded=context.with(|ctx| {
                let host = Object::new(ctx.clone())?;
                host.set(
                    "compressBrotli",
                    Function::new(ctx.clone(), move |raw_base64: String| -> Result<String, rquickjs::Error> {
                        if !can_compress {return Ok("null".into());}
                        let result = (|| -> Option<Value> {
                            let bytes = base64::engine::general_purpose::STANDARD.decode(raw_base64).ok()?;
                            let compressed = crate::services::brotli_codec::compress(&bytes).ok()?;
                            Some(json!({
                                "__aiSwitchBytes": base64::engine::general_purpose::STANDARD.encode(compressed),
                                "__aiSwitchHeaders": { "content-encoding": "br", "content-length": null }
                            }))
                        })();
                        Ok(result.map(|value| value.to_string()).unwrap_or_else(|| "null".into()))
                    })?,
                )?;
                host.set(
                    "fetchAsDataUrl",
                    Function::new(ctx.clone(), move |url: String| -> Option<String> { if can_fetch {fetch_image_data_url(url)} else {None} })?,
                )?;
                host.set(
                    "log",
                    Function::new(ctx.clone(), move |level: String, message: String| {
                        if can_log {crate::services::plugin_host::log(&log_id,&level,&message);}
                    })?,
                )?;
                host.set("matchRules",Function::new(ctx.clone(),|rules:String,event:String|->String {
                    let rules=serde_json::from_str::<Vec<crate::services::failure_rules::FailureRule>>(&rules).unwrap_or_default();
                    let event:Value=serde_json::from_str(&event).unwrap_or_else(|_|json!({}));
                    let failure=crate::services::response_failure_service::SemanticResponseFailure {code:event["code"].as_str().map(str::to_owned),error_type:event["type"].as_str().map(str::to_owned),message:event["message"].as_str().unwrap_or("").into()};
                    serde_json::to_string(&crate::services::failure_rules::evaluate(&rules,event["status"].as_u64().and_then(|v|u16::try_from(v).ok()),Some(&failure),event["transport"].as_bool().unwrap_or(false))).unwrap_or_else(|_|"{}".into())
                })?)?;
                ctx.globals().set("__host", host)?;
                ctx.eval::<(), _>("globalThis.__aiSwitchHooks = {};")?;
                ctx.eval::<(), _>(RUN_CHAIN_SCRIPT)?;
                ctx.eval::<(), _>(RUN_POLICIES_SCRIPT)?;
                ctx.eval::<(), _>(include_str!("builtin_plugins/sdk.js"))?;
                    let wrapped = format!(
                        "globalThis.__aiSwitchHooks[{id:?}] = (function() {{\n\
                         \"use strict\";\n{script}\n\
                         return {{\n\
                         onRequest: typeof onRequest === \"function\" ? onRequest : null,\n\
                         onRequestHeaders: typeof onRequestHeaders === \"function\" ? onRequestHeaders : null,\n\
                         onResponse: typeof onResponse === \"function\" ? onResponse : null,\n\
                         onFailure: typeof onFailure === \"function\" ? onFailure : null,\n\
                         onComplete: typeof onComplete === \"function\" ? onComplete : null,\n\
                         onResponseHeaders: typeof onResponseHeaders === \"function\" ? onResponseHeaders : null,\n\
                         onStreamEvent: typeof onStreamEvent === \"function\" ? onStreamEvent : null,\n\
                         policy: typeof policy === \"function\" ? policy : null\n\
                         }};\n}} )();"
                    );
                    ctx.eval::<(), _>(wrapped)?;
                Ok::<(), rquickjs::Error>(())
            })
            ;
            runtime.set_interrupt_handler(None);
            match loaded {
                Ok(()) => {
                    let descriptor = descriptors.iter().find(|d| d.id == id).cloned();
                    next.plugins.insert(
                        id,
                        Arc::new(LoadedPlugin {
                            descriptor,
                            engine: Mutex::new(PluginEngine { context, runtime }),
                        }),
                    );
                }
                Err(error) => crate::services::plugin_host::log(
                    &id,
                    "error",
                    &format!("Plugin load failed: {error}"),
                ),
            }
        }
        Ok(next)
    }
    pub fn validate_package(
        manifest: &crate::models::plugin::PluginManifest,
        script: &[u8],
    ) -> Result<(), AppError> {
        if script.len() > MAX_SCRIPT_BYTES {
            return Err(app_error(
                "plugin.script_too_large",
                "中间件脚本不能超过 256 KiB".into(),
            ));
        }
        let text = std::str::from_utf8(script)
            .map_err(|e| app_error("plugin.script_utf8", e.to_string()))?;
        let runtime =
            Runtime::new().map_err(|e| app_error("plugin.js_runtime_failed", e.to_string()))?;
        runtime.set_memory_limit(64 * 1024 * 1024);
        let ctx = Context::full(&runtime)
            .map_err(|e| app_error("plugin.js_context_failed", e.to_string()))?;
        let deadline = Instant::now() + Duration::from_millis(200);
        runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
        let result=ctx.with(|ctx|{
            ctx.eval::<(),_>("globalThis.__host=Object.freeze({compressBrotli:function(){return 'null'},fetchAsDataUrl:function(){return null},log:function(){},matchRules:function(){return '{}'}});")?;
            ctx.eval::<(),_>(include_str!("builtin_plugins/sdk.js"))?;
            ctx.eval::<(),_>(format!("(function(){{'use strict';\n{text}\n}})();"))
        });
        runtime.set_interrupt_handler(None);
        result.map_err(|e| app_error("plugin.js_load_failed", format!("{}: {e}", manifest.id)))
    }
    fn ensure_configured(paths: &AppPaths) {
        if registry().read().unwrap_or_else(|e| e.into_inner()).0
            == GENERATION.load(Ordering::Acquire)
        {
            return;
        }
        if let Err(error) = blocking_host(|| Self::configure_locked(paths, true)) {
            crate::services::plugin_host::log("host", "error", &error.to_string());
        }
    }
}

/// Precompiled JS runner. Compiled once in `configure`, invoked via
/// `Function::call` with no script compilation at request time. It parses the
/// body/ctx/entries once, walks the whole chain, and returns one tagged result.
const RUN_CHAIN_SCRIPT: &str = r#"
globalThis.__aiSwitchRunChain = function (bodyJson, ctxJson, entriesJson) {
  "use strict";
  var body = JSON.parse(bodyJson);
  var baseCtx = JSON.parse(ctxJson);
  var entries = JSON.parse(entriesJson);
  var hooks = globalThis.__aiSwitchHooks || {};
  var headers = {};
  var wireBytes = null;
  var changed = false;
  for (var i = 0; i < entries.length; i++) {
    var entry = entries[i];
    var h = hooks[entry.id];
    if (!h || typeof h[entry.hook] !== "function") continue;
    var ctx = {};
    for (var k in baseCtx) ctx[k] = baseCtx[k];
    ctx.config = entry.config || {};
    var next = h[entry.hook](body, ctx);
    if (next === undefined || next === null) continue;
    if (next && typeof next === "object" && Array.isArray(next.actions)) throw Error("Persistent/recovery actions require onFailure/onComplete");
    if (next && typeof next === "object" && next.__middlewareResult === true) {
      var hh=next.headers||{}; for(var hn in hh) headers[hn]=hh[hn];
      next=next.body;
    }
    if (next && typeof next === "object" && typeof next.__aiSwitchBytes === "string") {
      wireBytes = next.__aiSwitchBytes;
      var wh = next.__aiSwitchHeaders || {};
      for (var hk in wh) headers[hk] = wh[hk];
      break;
    }
    body = next;
    changed = true;
  }
  return JSON.stringify({
    bytes: wireBytes,
    headers: headers,
    body: (wireBytes || !changed) ? null : JSON.stringify(body)
  });
};
"#;

/// Precompiled policy runner: merges every schedule plugin's patch in JS.
const RUN_POLICIES_SCRIPT: &str = r#"
globalThis.__aiSwitchRunPolicies = function (entriesJson, ctxJson) {
  "use strict";
  var entries = JSON.parse(entriesJson);
  var baseCtx = JSON.parse(ctxJson);
  var hooks = globalThis.__aiSwitchHooks || {};
  var patch = {};
  for (var i = 0; i < entries.length; i++) {
    var entry = entries[i];
    var h = hooks[entry.id];
    if (!h || typeof h.policy !== "function") continue;
    var partial = h.policy(entry.config || {}, baseCtx);
    if (!partial || typeof partial !== "object") continue;
    for (var k in partial) {
      if (partial[k] !== null && partial[k] !== undefined) patch[k] = partial[k];
    }
  }
  return JSON.stringify(patch);
};
"#;

#[derive(Serialize)]
struct ChainEntry {
    id: String,
    hook: String,
    config: Value,
}

#[derive(Serialize)]
struct PolicyEntry {
    id: String,
    config: Value,
}

struct ChainCallResult {
    body: Value,
    bytes: Option<Vec<u8>>,
    headers: Vec<(String, Option<String>)>,
}

impl PluginMiddleware {
    /// Call an already-compiled global runner: function lookup + arg conversion,
    /// never script compilation.
    fn invoke_runner(
        &self,
        id: &str,
        name: &str,
        a: String,
        b: String,
        c: String,
    ) -> Option<String> {
        self.plugins.get(id)?.invoke(name, a, b, c)
    }

    fn run_chain(
        &self,
        body: &Value,
        ctx: &Value,
        entries: &[ChainEntry],
    ) -> Option<ChainCallResult> {
        let mut result = ChainCallResult {
            body: body.clone(),
            bytes: None,
            headers: Vec::new(),
        };
        for entry in entries {
            match self.run_one(&result.body, ctx, std::slice::from_ref(entry)) {
                Some(next) => {
                    result.body = next.body;
                    result.headers.extend(next.headers);
                    result.bytes = next.bytes;
                    if result.bytes.is_some() {
                        break;
                    }
                }
                None => crate::services::plugin_host::log(
                    &entry.id,
                    "error",
                    "Body hook failed; original payload retained",
                ),
            }
        }
        Some(result)
    }

    fn run_one(
        &self,
        body: &Value,
        ctx: &Value,
        entries: &[ChainEntry],
    ) -> Option<ChainCallResult> {
        let body_json = serde_json::to_string(body).ok()?;
        let ctx_json = serde_json::to_string(ctx).ok()?;
        let entries_json = serde_json::to_string(entries).ok()?;
        let text = self.invoke_runner(
            &entries.first()?.id,
            "__aiSwitchRunChain",
            body_json,
            ctx_json,
            entries_json,
        )?;
        let value: Value = serde_json::from_str(&text).ok()?;
        let bytes = match value.get("bytes").and_then(Value::as_str) {
            Some(encoded) => Some(
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .ok()?,
            ),
            None => None,
        };
        let mut headers = Vec::new();
        if let Some(map) = value.get("headers").and_then(Value::as_object) {
            for (key, header_value) in map {
                headers.push((
                    key.clone(),
                    match header_value {
                        Value::Null => None,
                        Value::String(text) => Some(text.clone()),
                        other => Some(other.to_string()),
                    },
                ));
            }
        }
        let body = match value.get("body").and_then(Value::as_str) {
            Some(text) => serde_json::from_str(text).unwrap_or_else(|_| body.clone()),
            None => body.clone(),
        };
        Some(ChainCallResult {
            body,
            bytes,
            headers,
        })
    }

    fn run_policies(&self, entries: &[PolicyEntry], ctx: &Value) -> Option<Value> {
        let mut result = json!({});
        for entry in entries {
            let Some(text) = self.invoke_runner(
                &entry.id,
                "__aiSwitchRunPolicies",
                serde_json::to_string(&[entry]).ok()?,
                ctx.to_string(),
                String::new(),
            ) else {
                continue;
            };
            if let Ok(Value::Object(patch)) = serde_json::from_str::<Value>(&text) {
                result.as_object_mut()?.extend(patch);
            }
        }
        Some(result)
    }
}

/// Invoke one plugin's body hook as a single-entry chain.
pub fn apply_plugin_hook(id: &str, hook: &str, body: &[u8], ctx: &Value) -> HookResult {
    PluginMiddleware::ensure_configured(current_paths());
    let Ok(original) = serde_json::from_slice::<Value>(body) else {
        return HookResult {
            body: Value::Null,
            ..Default::default()
        };
    };
    let config = ctx.get("config").cloned().unwrap_or_else(|| json!({}));
    let entry = ChainEntry {
        id: id.into(),
        hook: hook.into(),
        config,
    };
    let guard = middleware();
    match guard.run_chain(&original, ctx, &[entry]) {
        Some(result) => HookResult {
            body: result.body,
            bytes: result.bytes,
            headers: result.headers,
        },
        None => HookResult {
            body: original,
            ..Default::default()
        },
    }
}

pub fn lifecycle_bound(
    plugin: &LoadedPlugin,
    id: &str,
    hook: &str,
    event: &Value,
    context: &Value,
) -> Option<Value> {
    let result = plugin.invoke(
        "__aiSwitchLifecycle",
        id.into(),
        hook.into(),
        json!({"event":event,"context":context}).to_string(),
    )?;
    serde_json::from_str(&result).ok()
}

pub fn lifecycle_hook(id: &str, hook: &str, event: &Value, context: &Value) -> Option<Value> {
    ensure_ready();
    let guard = middleware();
    let result = guard.invoke_runner(
        id,
        "__aiSwitchLifecycle",
        id.into(),
        hook.into(),
        json!({"event":event,"context":context}).to_string(),
    )?;
    serde_json::from_str(&result).ok()
}

/// Invoke one schedule plugin's `policy()` hook.
pub fn policy_bound(
    binding: &LoadedPlugin,
    id: &str,
    config: &Value,
    ctx: &Value,
) -> Option<Value> {
    let entry = PolicyEntry {
        id: id.into(),
        config: config.clone(),
    };
    let text = binding.invoke(
        "__aiSwitchRunPolicies",
        serde_json::to_string(&[entry]).ok()?,
        ctx.to_string(),
        String::new(),
    )?;
    serde_json::from_str(&text).ok()
}
pub fn policy_hook(id: &str, config: &Value, ctx: &Value) -> Option<Value> {
    PluginMiddleware::ensure_configured(current_paths());
    let entry = PolicyEntry {
        id: id.into(),
        config: config.clone(),
    };
    let guard = middleware();
    guard.run_policies(&[entry], ctx)
}

/// Resolve all enabled schedule plugins in one call.
pub fn policy_chain(entries: &[(String, Value)], ctx: &Value) -> Option<Value> {
    if entries.is_empty() {
        return Some(json!({}));
    }
    PluginMiddleware::ensure_configured(current_paths());
    let refs: Vec<PolicyEntry> = entries
        .iter()
        .map(|(id, config)| PolicyEntry {
            id: id.clone(),
            config: config.clone(),
        })
        .collect();
    let guard = middleware();
    guard.run_policies(&refs, ctx)
}

/// Run every chain entry whose plugin stage matches `stage`, in one JS call.
/// Shipped and installed descriptors both declare stages, hooks and capabilities.
pub fn apply_chain(
    pipeline: &PipelineConfig,
    body: &[u8],
    stage: &str,
    base_ctx: &Value,
) -> ChainOutcome {
    if stage == "policy" {
        return ChainOutcome {
            bytes: body.to_vec(),
            ..Default::default()
        };
    }
    let hook = if stage == "response" {
        "onResponse"
    } else {
        "onRequest"
    };
    let mut entries = Vec::new();
    ensure_ready();
    let guard = middleware();
    let request = stage != "response";
    let directions = [(
        request,
        if request {
            &pipeline.request
        } else {
            &pipeline.response
        },
    )];
    for (request, direction) in directions {
        if pipeline.is_passthrough(request) {
            continue;
        }
        for plugin in direction.chain.iter().filter(|p| p.enabled) {
            let Some(d) = guard
                .plugins
                .get(&plugin.id)
                .and_then(|p| p.descriptor.as_ref())
            else {
                continue;
            };
            if d.stage != stage && !d.stages.iter().any(|s| s == stage) {
                continue;
            }
            if !d.hooks.iter().any(|h| h == hook) {
                continue;
            }
            let capability = if stage == "response" {
                "response.transform"
            } else {
                "request.transform"
            };
            if !d.capabilities.iter().any(|c| c == capability) {
                continue;
            }
            entries.push(ChainEntry {
                id: plugin.id.clone(),
                hook: hook.into(),
                config: crate::services::plugin_contract::merge_config(&d.defaults, &plugin.config),
            });
        }
    }
    if entries.is_empty() {
        return ChainOutcome {
            bytes: body.to_vec(),
            ..Default::default()
        };
    }
    let Ok(original) = serde_json::from_slice::<Value>(body) else {
        return ChainOutcome {
            bytes: body.to_vec(),
            ..Default::default()
        };
    };
    let mut ctx = base_ctx.clone();
    ctx["stage"] = json!(stage);
    match guard.run_chain(&original, &ctx, &entries) {
        Some(result) => ChainOutcome {
            bytes: result.bytes.unwrap_or_else(|| {
                if result.body == original {
                    body.to_vec()
                } else {
                    serde_json::to_vec(&result.body).unwrap_or_else(|_| body.to_vec())
                }
            }),
            headers: result.headers,
        },
        None => ChainOutcome {
            bytes: body.to_vec(),
            ..Default::default()
        },
    }
}

/// Build the rich hook context from the data available at a proxy stage.
pub fn hook_context(
    platform: &str,
    path: &str,
    stage: &str,
    base_url: Option<&str>,
    dialect: Option<&str>,
    headers: Option<&std::collections::HashMap<String, String>>,
    raw_body_base64: Option<&str>,
    attempt: Option<usize>,
) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("platform".into(), json!(platform));
    map.insert("path".into(), json!(path));
    map.insert(
        "phase".into(),
        json!(if stage == "response" {
            "onResponse"
        } else {
            "onRequest"
        }),
    );
    map.insert("stage".into(), json!(stage));
    map.insert("base_url".into(), json!(base_url));
    map.insert("dialect".into(), json!(dialect));
    map.insert("attempt".into(), json!(attempt));
    map.insert("rawBodyBase64".into(), json!(raw_body_base64));
    let mut h = serde_json::Map::new();
    if let Some(headers) = headers {
        for (key, value) in headers {
            h.insert(key.clone(), json!(value));
        }
    }
    map.insert("headers".into(), Value::Object(h));
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::pipeline::{
        DirectionConfig, PipelineConfig, PipelineMode, PluginInstance, PIPELINE_VERSION,
    };
    use serde_json::json;

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn setup() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(dir.path().to_path_buf());
        std::fs::write(&paths.settings_file, r#"{"plugins_enabled_json":"[]"}"#).unwrap();
        PluginMiddleware::configure(&paths).unwrap();
        dir
    }

    #[test]
    fn installed_pack_uses_its_real_id_stage_and_defaults() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(dir.path().into());
        let pack = paths.plugins_dir.join("arbitrary-transform");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(pack.join("plugin.json"),json!({"schema_version":1,"id":"arbitrary-transform","version":"1","name":"external","description":"test","author":"test","license":"MIT","contributes":{"middleware":{"file":"middleware.js","descriptor":{"id":"ignored","name":"external","description":"test","lane":"request","kind":"transform","stage":"postBridgeReminder","hooks":["onRequest"],"capabilities":["request.transform"],"defaults":{"value":"from-default"},"config_fields":[]}}}}).to_string()).unwrap();
        std::fs::write(
            pack.join("middleware.js"),
            "function onRequest(body,ctx){body.external=ctx.config.value;return body;}",
        )
        .unwrap();
        std::fs::write(
            &paths.settings_file,
            r#"{"plugins_enabled_json":"[\"arbitrary-transform\"]"}"#,
        )
        .unwrap();
        PluginMiddleware::configure(&paths).unwrap();
        let pipeline:PipelineConfig=serde_json::from_value(json!({"version":2,"request":{"mode":"process","chain":[{"id":"pack:arbitrary-transform","enabled":true,"config":{}}]}})).unwrap();
        let result = apply_chain(
            &pipeline,
            br#"{"input":"hi"}"#,
            "postBridgeReminder",
            &json!({}),
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&result.bytes).unwrap()["external"],
            "from-default"
        );
    }

    #[test]
    fn external_failure_hook_receives_context_and_returns_actions() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(dir.path().into());
        let pack = paths.plugins_dir.join("random-health");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(pack.join("plugin.json"),json!({"schema_version":1,"id":"random-health","version":"1","name":"health","description":"test","author":"test","license":"MIT","contributes":{"middleware":{"file":"middleware.js","descriptor":{"id":"ignored","name":"health","description":"test","lane":"recovery","kind":"schedule","stage":"policy","hooks":["onFailure"],"capabilities":["account.write"],"defaults":{"seconds":17}}}}}).to_string()).unwrap();
        std::fs::write(pack.join("middleware.js"),"function onFailure(event,ctx){return {actions:[host.accounts.update({status:'error',cooldown_seconds:ctx.config.seconds})]};}").unwrap();
        std::fs::write(
            &paths.settings_file,
            r#"{"plugins_enabled_json":"[\"random-health\"]"}"#,
        )
        .unwrap();
        PluginMiddleware::configure(&paths).unwrap();
        let outcome = crate::services::plugin_host::invoke(
            "pack:random-health",
            "onFailure",
            &json!({"message":"busy","phase":"account_event"}),
            &json!({"account":{"id":"bound"}}),
            &json!({}),
        );
        assert_eq!(outcome.actions[0]["type"], "account.update");
        assert_eq!(outcome.actions[0]["cooldown_seconds"], 17);
    }

    #[tokio::test]
    async fn random_pack_account_actions_are_transactional_scoped_and_idempotent() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(dir.path().into());
        let pack = paths.plugins_dir.join("random-writer");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(pack.join("plugin.json"),json!({"schema_version":1,"id":"random-writer","version":"1","name":"writer","description":"test","author":"test","license":"MIT","contributes":{"middleware":{"file":"middleware.js","descriptor":{"id":"ignored","name":"writer","description":"test","lane":"recovery","kind":"schedule","stage":"policy","hooks":["onFailure"],"capabilities":["account.read","account.write","state.write"],"defaults":{}}}}}).to_string()).unwrap();
        std::fs::write(pack.join("middleware.js"),"function onFailure(event,ctx){if(!ctx.account.id)throw Error('no scope');return {actions:[host.accounts.update({status:'error',cooldown_seconds:12}),host.state.increment('failures',1)]};}").unwrap();
        std::fs::write(
            &paths.settings_file,
            r#"{"plugins_enabled_json":"[\"random-writer\"]"}"#,
        )
        .unwrap();
        PluginMiddleware::configure(&paths).unwrap();
        let pool = crate::database::create_memory_pool().await.unwrap();
        crate::database::run_migrations(&pool).await.unwrap();
        let config=json!({"pipeline":{"version":2,"recovery":{"mode":"process","chain":[{"id":"pack:random-writer","enabled":true,"config":{}}]}}}).to_string();
        let a=crate::database::repositories::route_credential_repository::RouteCredentialRepository::create(&pool,"codex","api","writer",None,"ok",None,"{}",&config,"{}").await.unwrap();
        let other=crate::database::repositories::route_credential_repository::RouteCredentialRepository::create(&pool,"codex","api","other",None,"ok",None,"{}","{}","{}").await.unwrap();
        for _ in 0..2 {
            crate::services::plugin_actions::dispatch_account_event(
                &pool,
                &a.id,
                &config,
                "attempt-1",
                "onFailure",
                &json!({"message":"busy","phase":"account_event"}),
            )
            .await
            .unwrap();
        }
        let row=crate::database::repositories::route_credential_repository::RouteCredentialRepository::get(&pool,&a.id).await.unwrap();
        assert_eq!(row.status, "error");
        assert!(row.cooldown_until.is_some());
        let value:String=sqlx::query_scalar("SELECT value_json FROM plugin_scoped_state WHERE account_id=? AND plugin_id='pack:random-writer' AND state_key='failures'").bind(&a.id).fetch_one(&pool).await.unwrap();
        assert_eq!(value, "1");
        assert_eq!(crate::database::repositories::route_credential_repository::RouteCredentialRepository::get(&pool,&other.id).await.unwrap().status,"ok");
        crate::database::repositories::route_credential_repository::RouteCredentialRepository::update_status(&pool,&a.id,"paused").await.unwrap();
        crate::services::plugin_actions::dispatch_account_event(
            &pool,
            &a.id,
            &config,
            "attempt-2",
            "onFailure",
            &json!({"phase":"account_event"}),
        )
        .await
        .unwrap();
        assert_eq!(crate::database::repositories::route_credential_repository::RouteCredentialRepository::get(&pool,&a.id).await.unwrap().status,"paused");
        let invalid = crate::services::plugin_actions::execute(
            &pool,
            &a.id,
            "pack:random-writer",
            "invalid",
            "onFailure",
            &[
                json!({"type":"state.increment","key":"failures","amount":1}),
                json!({"type":"account.update","status":"ok","ignored_field":true}),
            ],
        )
        .await;
        assert!(invalid.is_err());
        sqlx::query("UPDATE route_credentials SET config_json=? WHERE id=?")
            .bind(
                json!({"pipeline":{"version":2,"recovery":{"mode":"process","chain":[]}}})
                    .to_string(),
            )
            .bind(&a.id)
            .execute(&pool)
            .await
            .unwrap();
        let changed = crate::services::plugin_actions::dispatch_account_event(
            &pool,
            &a.id,
            &config,
            "attempt-removed",
            "onFailure",
            &json!({"phase":"account_event"}),
        )
        .await
        .unwrap();
        assert!(!changed);
        let value:String=sqlx::query_scalar("SELECT value_json FROM plugin_scoped_state WHERE account_id=? AND plugin_id='pack:random-writer' AND state_key='failures'").bind(&a.id).fetch_one(&pool).await.unwrap();
        assert_eq!(value, "2");
    }

    fn install_test_scripts(paths: &AppPaths, scripts: &[(&str, &str)]) {
        for (id, script) in scripts {
            let folder = paths.plugins_dir.join(id);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("middleware.js"), script).unwrap();
            std::fs::write(folder.join("plugin.json"),json!({"schema_version":1,"id":id,"version":"1","name":id,"description":"test","author":"test","license":"MIT","contributes":{"middleware":{"file":"middleware.js","descriptor":{"id":id,"name":id,"description":"test","lane":"recovery","kind":"schedule","stage":"policy","hooks":["policy","onFailure","onStreamEvent"],"capabilities":["account.write","stream.transform","policy.configure"]}}}}).to_string()).unwrap();
        }
        std::fs::write(&paths.settings_file,json!({"plugins_enabled_json":serde_json::to_string(&scripts.iter().map(|(id,_)|id).collect::<Vec<_>>()).unwrap()}).to_string()).unwrap();
    }

    #[test]
    fn isolated_scripts_compose_stream_values_and_discard_failed_effects() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(dir.path().into());
        install_test_scripts(&paths,&[
            ("one","globalThis.leak='private';function onStreamEvent(e){e.text+=' A';return {value:e};} function onFailure(e){var a=host.accounts.update({status:'error'});throw Error('abort');}"),
            ("two","function onStreamEvent(e){if(globalThis.leak)throw Error('cross-plugin leak');e.text+=' B';return {value:e};}function onFailure(e){return {actions:[host.accounts.update({status:'error'}),{type:'unknown'}]};}")
        ]);
        PluginMiddleware::configure(&paths).unwrap();
        let p:PipelineConfig=serde_json::from_value(json!({"version":2,"recovery":{"mode":"process","chain":[{"id":"pack:one","enabled":true,"config":{}},{"id":"pack:two","enabled":true,"config":{}}]}})).unwrap();
        let values = crate::services::plugin_host::dispatch(
            &p,
            "onStreamEvent",
            &json!({"text":"start"}),
            &json!({}),
        );
        assert_eq!(
            values.last().unwrap().1.value.as_ref().unwrap()["text"],
            "start A B"
        );
        let effects = crate::services::plugin_host::dispatch(
            &p,
            "onFailure",
            &json!({"phase":"account_event"}),
            &json!({}),
        );
        assert!(effects
            .iter()
            .all(|(_, v)| v.actions.is_empty() && v.error.is_some()));
    }

    #[test]
    fn script_policy_cannot_expand_host_retry_ceiling() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(dir.path().into());
        install_test_scripts(
            &paths,
            &[(
                "oversized",
                "function policy(c,ctx){return {retry_count:4294967295,failover_enabled:true};}",
            )],
        );
        PluginMiddleware::configure(&paths).unwrap();
        let p:PipelineConfig=serde_json::from_value(json!({"version":2,"recovery":{"mode":"process","chain":[{"id":"pack:oversized","enabled":true,"config":{}}]}})).unwrap();
        let policy = crate::services::builtin_plugins::policy_for_domain(&p, "request");
        assert_eq!(policy.retry_count, 0);
        assert!(!policy.failover_enabled);
    }

    #[test]
    fn review_direction_isolation_and_noop_preserves_bytes() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(dir.path().into());
        install_test_scripts(&paths,&[("dual","function onRequest(b){b.requests=(b.requests||0)+1;return b;}function onResponse(b){b.responses=(b.responses||0)+1;return b;}")]);
        let file = paths.plugins_dir.join("dual/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        manifest["contributes"]["middleware"]["descriptor"] = json!({"id":"dual","name":"dual","description":"dual","lane":"both","kind":"transform","stage":"perAttempt","stages":["perAttempt","response"],"hooks":["onRequest","onResponse"],"capabilities":["request.transform","response.transform"]});
        std::fs::write(&file, manifest.to_string()).unwrap();
        PluginMiddleware::configure(&paths).unwrap();
        let mut p:PipelineConfig=serde_json::from_value(json!({"version":2,"request":{"mode":"process","chain":[{"id":"pack:dual","enabled":true,"config":{}}]}})).unwrap();
        let raw = br#"{ "x" : 1 }"#;
        assert_eq!(apply_chain(&p, raw, "response", &json!({})).bytes, raw);
        p.response.chain = p.request.chain.clone();
        assert_eq!(
            serde_json::from_slice::<Value>(&apply_chain(&p, raw, "response", &json!({})).bytes)
                .unwrap()["responses"],
            1
        );
        std::fs::write(
            paths.plugins_dir.join("dual/middleware.js"),
            "function onRequest(){return null;}function onResponse(){throw Error('oops');}",
        )
        .unwrap();
        PluginMiddleware::configure(&paths).unwrap();
        assert_eq!(apply_chain(&p, raw, "perAttempt", &json!({})).bytes, raw);
        assert_eq!(apply_chain(&p, raw, "response", &json!({})).bytes, raw);
    }

    #[test]
    fn review_unavailable_saved_instances_can_be_retained() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _dir = setup();
        let p:PipelineConfig=serde_json::from_value(json!({"version":2,"request":{"mode":"process","chain":[{"id":"pack:removed","enabled":false,"config":{"kept":17}}]}})).unwrap();
        assert!(crate::models::pipeline::validate_pipeline(&p).is_ok());
    }

    #[test]
    fn review_stale_generation_cannot_publish_after_disable() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = setup();
        let paths = AppPaths::from_data_dir(dir.path().into());
        let old = GENERATION.load(Ordering::Acquire);
        let snapshot = PluginMiddleware::build(&paths).unwrap();
        mark_dirty();
        assert!(!publish_generation(old, snapshot));
        assert_ne!(
            registry().read().unwrap().0,
            GENERATION.load(Ordering::Acquire)
        );
        PluginMiddleware::configure(&paths).unwrap();
        assert_eq!(
            registry().read().unwrap().0,
            GENERATION.load(Ordering::Acquire)
        );
    }
    #[test]
    fn review_slow_host_call_does_not_lock_other_plugin() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(dir.path().into());
        install_test_scripts(
            &paths,
            &[
                ("slow", "function onFailure(){__host.wait();return {}; }"),
                ("fast", "function onFailure(){return {value:42};}"),
            ],
        );
        PluginMiddleware::configure(&paths).unwrap();
        let snapshot = middleware();
        let slow = snapshot.plugins.get("pack:slow").unwrap();
        let (entered, rx) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        {
            let engine = slow.engine.lock().unwrap();
            engine.context.with(|ctx| {
                let host: Object = ctx.globals().get("__host").unwrap();
                host.set(
                    "wait",
                    Function::new(ctx.clone(), move || {
                        let _ = entered.send(());
                        let _ = wait.recv_timeout(Duration::from_secs(2));
                    })
                    .unwrap(),
                )
                .unwrap();
            });
        }
        let worker = std::thread::spawn(move || {
            snapshot.invoke_runner(
                "pack:slow",
                "__aiSwitchLifecycle",
                "pack:slow".into(),
                "onFailure".into(),
                json!({"event":{},"context":{}}).to_string(),
            )
        });
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let (done, ready) = std::sync::mpsc::channel();
        let next = std::thread::spawn(move || {
            let result = middleware().invoke_runner(
                "pack:fast",
                "__aiSwitchLifecycle",
                "pack:fast".into(),
                "onFailure".into(),
                json!({"event":{},"context":{}}).to_string(),
            );
            let _ = done.send(result);
        });
        let result = ready.recv_timeout(Duration::from_millis(500));
        release.send(()).unwrap();
        worker.join().unwrap();
        next.join().unwrap();
        assert!(result.unwrap().is_some());
    }
    #[test]
    fn review_network_addresses_require_all_public_and_reject_mapped_private() {
        for value in [
            "127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::1",
        ] {
            assert!(!is_public_ip(value.parse().unwrap()), "{value}");
        }
        assert!(is_public_ip("108.1.1.1".parse().unwrap()));
        assert!(approved_addresses(vec![
            "8.8.8.8:443".parse().unwrap(),
            "127.0.0.1:443".parse().unwrap()
        ])
        .is_none());
    }
    #[tokio::test]
    async fn review_chunked_image_limit_is_enforced_during_read() {
        use axum::{body::Body, routing::get, Router};
        let app = Router::new().route(
            "/",
            get(|| async {
                axum::response::Response::builder()
                    .header("content-type", "image/png")
                    .body(Body::from_stream(futures_util::stream::iter([
                        Ok::<_, std::io::Error>(vec![0u8; 600]),
                        Ok(vec![0u8; 600]),
                    ])))
                    .unwrap()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let response = reqwest::Client::new().get(url).send().await.unwrap();
        assert_eq!(response.content_length(), None);
        assert!(read_image_limited(response, 1024).await.is_none());
        server.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn review_blocking_hooks_do_not_starve_tokio_timers() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(dir.path().into());
        install_test_scripts(
            &paths,
            &[("blocking", "function onFailure(){__host.wait();return {};}")],
        );
        PluginMiddleware::configure(&paths).unwrap();
        let binding = loaded("pack:blocking").unwrap();
        {
            let engine = binding.engine.lock().unwrap();
            engine.context.with(|ctx| {
                let host: Object = ctx.globals().get("__host").unwrap();
                host.set(
                    "wait",
                    Function::new(ctx.clone(), || {
                        std::thread::sleep(Duration::from_millis(350))
                    })
                    .unwrap(),
                )
                .unwrap();
            });
        }
        let start = Instant::now();
        let (ready, rx) = tokio::sync::oneshot::channel();
        let timer = tokio::spawn(async move {
            let _ = ready.send(());
            tokio::time::sleep(Duration::from_millis(50)).await;
            start.elapsed()
        });
        rx.await.unwrap();
        let mut tasks = Vec::new();
        for _ in 0..2 {
            tasks.push(tokio::spawn(async {
                lifecycle_hook("pack:blocking", "onFailure", &json!({}), &json!({}))
            }));
        }
        let elapsed = timer.await.unwrap();
        for t in tasks {
            t.await.unwrap();
        }
        assert!(
            elapsed < Duration::from_millis(250),
            "timer delayed by {elapsed:?}"
        );
    }
    #[test]
    fn review_lazy_initialization_reuses_already_published_generation() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = setup();
        let paths = AppPaths::from_data_dir(dir.path().into());
        let before = loaded("builtin.recovery.retry").unwrap();
        PluginMiddleware::configure_locked(&paths, true).unwrap();
        assert!(Arc::ptr_eq(
            &before,
            &loaded("builtin.recovery.retry").unwrap()
        ));
    }

    #[test]
    fn header_image_plugins_apply_config_without_hidden_defaults(){
        let _guard=TEST_LOCK.lock().unwrap_or_else(|e|e.into_inner());let _dir=setup();
        let event=json!({"phase":"after_auth","headers":{"User-Agent":"Default/1","X-Remove":"old","authorization":"Bearer real"}});
        let config=json!({"user_agent":"Custom/2","headers":[{"name":"X-Remove","operation":"remove","value":""},{"name":"X-New","operation":"set","value":"new"},{"name":"authorization","operation":"set","value":"wrong"}]});
        let result=crate::services::plugin_host::invoke("builtin.request.requestHeaders","onRequestHeaders",&event,&json!({}),&config).value.unwrap();
        assert_eq!(result["headers"]["user-agent"],"Custom/2");assert!(result["headers"]["X-Remove"].is_null());assert_eq!(result["headers"]["X-New"],"new");assert!(result["headers"].get("authorization").is_none());
        let body=json!({"input":[{"role":"user","content":[{"type":"input_image","image_url":"data:one"},{"type":"input_image","image_url":"data:two"},{"type":"input_image","image_url":"data:three"}]}]}).to_string().into_bytes();
        let ctx=json!({"stage":"imageLimit","config":{"inline_remote":false,"trim_old_images":true,"max_images":2}});
        let changed=apply_plugin_hook("builtin.request.imageProcessing","onRequest",&body,&ctx).body;
        assert_eq!(changed["input"][0]["content"][0]["type"],"input_text");
        assert_eq!(changed["input"][0]["content"][1]["image_url"],"data:two");
        let ctx=json!({"stage":"imageLimit","config":{"inline_remote":true,"trim_old_images":false,"max_images":1}});
        assert_eq!(apply_plugin_hook("builtin.request.imageProcessing","onRequest",&body,&ctx).body,serde_json::from_slice::<Value>(&body).unwrap());
    }

    #[test]
    fn policy_chain_merges_multiple_plugins_in_one_call() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let _dir = setup();
        let ctx = json!({ "platform": "codex", "path": "", "phase": "policy", "stage": "policy" });
        let patch = policy_chain(
            &[
                ("builtin.schedule.retry".into(), json!({ "retry_count": 7 })),
                ("builtin.schedule.cooldown".into(), json!({ "seconds": 30 })),
                ("builtin.schedule.failover".into(), json!({})),
            ],
            &ctx,
        )
        .expect("policy chain");
        assert_eq!(patch["retry_count"], 7);
        assert_eq!(patch["cooldown_enabled"], true);
        assert_eq!(patch["cooldown_seconds"], 30);
        assert_eq!(patch["failover_enabled"], true);
    }

    fn plugin(id: &str, config: Value) -> PluginInstance {
        PluginInstance {
            id: id.into(),
            enabled: true,
            config,
        }
    }

    #[test]
    fn builtin_policy_hook_returns_declared_patch() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let _dir = setup();
        let patch = policy_hook(
            "builtin.schedule.retry",
            &json!({ "retry_count": 7, "retry_interval_ms": 25 }),
            &json!({}),
        )
        .expect("retry policy hook");
        assert_eq!(patch["retry_count"], 7);
        assert_eq!(patch["retry_interval_ms"], 25);
    }

    #[test]
    fn builtin_transform_hook_elides_old_tool_outputs() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let _dir = setup();
        let mut inputs = Vec::new();
        for index in 0..8 {
            let mut output = String::from("x");
            output.push_str(&"y".repeat(160));
            output.push_str(&index.to_string());
            inputs.push(json!({ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "x" }] }));
            inputs.push(json!({ "type": "function_call_output", "call_id": format!("c{index}"), "output": output }));
        }
        let body = serde_json::to_vec(&json!({ "model": "m", "input": inputs })).unwrap();
        let ctx = hook_context(
            "codex",
            "/v1/responses",
            "retryRecovery",
            None,
            None,
            None,
            None,
            None,
        );
        let result =
            apply_plugin_hook("builtin.request.contentRejection", "onRequest", &body, &ctx);
        assert!(result.bytes.is_none());
        let value: Value =
            serde_json::from_slice(&serde_json::to_vec(&result.body).unwrap()).unwrap();
        let outputs: Vec<_> = value["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .collect();
        assert_eq!(outputs.len(), 8);
        assert!(outputs[0]["output"].as_str().unwrap().contains("omitted"));
        assert!(outputs[7]["output"].as_str().unwrap().starts_with("x"));
    }

    #[test]
    fn compression_hook_emits_tagged_wire_bytes_and_headers() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let _dir = setup();
        let body = serde_json::to_vec(&json!({ "model": "m", "input": "hi" })).unwrap();
        let mut headers = std::collections::HashMap::new();
        headers.insert("content-type".into(), "application/json".into());
        let ctx = hook_context(
            "codex",
            "/v1/responses",
            "wire",
            Some("https://ps.air-outer.com/v1/responses"),
            None,
            Some(&headers),
            Some(&base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &body,
            )),
            Some(1),
        );
        let pipeline = PipelineConfig {
            version: PIPELINE_VERSION,
            catalog_revision: 1,
            request: DirectionConfig {
                mode: PipelineMode::Process,
                chain: vec![plugin(
                    "builtin.request.compression",
                    json!({ "mode": "on" }),
                )],
            },
            response: DirectionConfig::default(),
            recovery: DirectionConfig::default(),
        };
        let outcome = apply_chain(&pipeline, &body, "wire", &ctx);
        assert_ne!(outcome.bytes, body);
        assert!(outcome
            .headers
            .iter()
            .any(|(key, value)| key == "content-encoding" && value.as_deref() == Some("br")));
    }

    #[test]
    fn chain_only_runs_plugins_for_its_stage() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let _dir = setup();
        let body = serde_json::to_vec(&json!({ "model": "m", "input": "hi" })).unwrap();
        let ctx = hook_context(
            "codex",
            "/v1/responses",
            "wire",
            None,
            None,
            None,
            None,
            None,
        );
        let pipeline = PipelineConfig {
            version: PIPELINE_VERSION,
            catalog_revision: 1,
            request: DirectionConfig {
                mode: PipelineMode::Process,
                chain: vec![plugin(
                    "builtin.request.compression",
                    json!({ "mode": "off" }),
                )],
            },
            response: DirectionConfig::default(),
            recovery: DirectionConfig::default(),
        };
        let outcome = apply_chain(&pipeline, &body, "postBridgeReminder", &ctx);
        assert_eq!(outcome.bytes, body);
        assert!(outcome.headers.is_empty());
    }
}
