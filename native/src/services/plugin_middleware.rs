//! Gateway middleware plugins (Magpie-style request/response hooks).
//!
//! Every plugin — built-in or third-party pack — is a `middleware.js` script
//! loaded into this one embedded QuickJS runtime and keyed by plugin id:
//!
//! ```js
//! function onRequest(body, ctx) { return body; }
//! function onResponse(body, ctx) { return body; }
//! function onEvent(event, ctx) { return event; }
//! function policy(config, ctx) { return { retry_count: 3 }; }
//! ```
//!
//! Hooks exchange plain JSON. A hook may return a tagged object
//! (`{ __aiSwitchBytes, __aiSwitchHeaders }`) to hand back raw wire bytes and
//! header changes; every other return value is treated as the new JSON body.
//! A small `__host` API exposes only capabilities JS cannot provide itself
//! (Brotli codec, image fetch, logging). Errors and timeouts fail open.

use crate::error::AppError;
use crate::models::settings::AppSettings;
use crate::models::pipeline::PipelineConfig;
use crate::paths::AppPaths;
use serde::Serialize;
use base64::Engine as _;
use rquickjs::{Context, Function, Object, Runtime};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const HOOK_TIMEOUT: Duration = Duration::from_millis(50);
const MIDDLEWARE_FILE: &str = "middleware.js";
const MAX_SCRIPT_BYTES: usize = 256 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);
const FETCH_MAX_BYTES: usize = 16 * 1024 * 1024;

static MIDDLEWARE: OnceLock<Mutex<PluginMiddleware>> = OnceLock::new();
static CONFIGURED: AtomicBool = AtomicBool::new(false);
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

pub struct PluginMiddleware {
    runtime: Runtime,
    context: Context,
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
        AppPaths::resolve().unwrap_or_else(|_| AppPaths::from_data_dir(std::path::PathBuf::from(".")))
    })
}

pub fn middleware() -> &'static Mutex<PluginMiddleware> {
    MIDDLEWARE.get_or_init(|| {
        Mutex::new(PluginMiddleware::empty().expect("failed to create the JS middleware runtime"))
    })
}

/// Force a rebuild on the next gateway request (install/uninstall/enable change).
pub fn mark_dirty() {
    CONFIGURED.store(false, Ordering::Relaxed);
    crate::services::builtin_plugins::mark_policy_dirty();
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || matches!(v4.octets()[0], 100..=107))
        }
        IpAddr::V6(v6) => {
            !(v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() || v6.is_unique_local()
                || v6.is_unicast_link_local())
        }
    }
}

fn url_is_public(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else { return false };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else { return false };
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_public_ip(ip);
    }
    let port = url.port_or_known_default().unwrap_or(if url.scheme() == "https" { 443 } else { 80 });
    let Ok(addrs) = (host, port).to_socket_addrs() else { return false };
    (host, port).to_socket_addrs().map(|mut a| a.any(|addr| is_public_ip(addr.ip()))).unwrap_or(false)
}

/// Fetch an image on a bounded OS thread. The QuickJS interrupt cannot fire
/// while a Rust host function blocks, so this call self-bounds with its own
/// timeout, redirect policy, size cap and SSRF guards.
fn fetch_image_data_url(url: String) -> Option<String> {
    if !url_is_public(&url) {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| -> Option<String> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()?;
            runtime.block_on(async {
                let client = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(FETCH_TIMEOUT)
                    .build()
                    .ok()?;
                let response = client.get(&url).send().await.ok()?;
                if !response.status().is_success() {
                    return None;
                }
                if response.content_length().unwrap_or(0) as usize > FETCH_MAX_BYTES {
                    return None;
                }
                let header_mime = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.split(';').next().unwrap_or(value).trim().to_lowercase());
                let bytes = response.bytes().await.ok()?;
                if bytes.is_empty() || bytes.len() > FETCH_MAX_BYTES {
                    return None;
                }
                let mime = sniff_image_mime(&bytes)
                    .or_else(|| header_mime.filter(|mime| mime.starts_with("image/")))?;
                Some(format!(
                    "data:{mime};base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                ))
            })
        })();
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
    fn empty() -> Result<Self, AppError> {
        let runtime = Runtime::new().map_err(|error| app_error("plugin.js_runtime_failed", error.to_string()))?;
        let context = Context::full(&runtime).map_err(|error| app_error("plugin.js_context_failed", error.to_string()))?;
        Ok(Self { runtime, context })
    }

    fn read_sources(paths: &AppPaths) -> Vec<(String, String)> {
        let settings: AppSettings = std::fs::read_to_string(&paths.settings_file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| AppSettings::defaults_for_data_dir(paths.data_dir.display().to_string()));
        let enabled: BTreeSet<String> = settings
            .plugins_enabled_json
            .as_deref()
            .and_then(|text| serde_json::from_str::<Vec<String>>(text).ok())
            .map(|ids| ids.into_iter().collect())
            .unwrap_or_default();

        let mut sources: Vec<(String, String)> = crate::services::builtin_plugins::embedded_sources()
            .into_iter()
            .map(|(id, script)| (id.to_string(), script.to_string()))
            .collect();
        for id in enabled {
            if crate::models::plugin::validate_pack_id(&id).is_err() {
                continue;
            }
            let path = paths.plugins_dir.join(&id).join(MIDDLEWARE_FILE);
            let Ok(bytes) = std::fs::read(&path) else { continue };
            if bytes.len() > MAX_SCRIPT_BYTES {
                continue;
            }
            if let Ok(script) = String::from_utf8(bytes) {
                sources.push((id, script));
            }
        }
        sources.sort_by(|a, b| a.0.cmp(&b.0));
        sources
    }

    /// Rebuild the JS context from embedded builtins plus enabled third-party
    /// packs. Hooks are stored per plugin id so an account chain can invoke
    /// only the selected ones.
    pub fn configure(paths: &AppPaths) -> Result<(), AppError> {
        let mut next = Self::empty()?;
        let sources = Self::read_sources(paths);
        next.context
            .with(|ctx| {
                let host = Object::new(ctx.clone())?;
                host.set(
                    "compressBrotli",
                    Function::new(ctx.clone(), |raw_base64: String| -> Result<String, rquickjs::Error> {
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
                    Function::new(ctx.clone(), |url: String| -> Option<String> { fetch_image_data_url(url) })?,
                )?;
                host.set(
                    "log",
                    Function::new(ctx.clone(), |level: String, message: String| {
                        let _ = (level, message);
                    })?,
                )?;
                ctx.globals().set("__host", host)?;
                ctx.eval::<(), _>("globalThis.__aiSwitchHooks = {};")?;
                ctx.eval::<(), _>(RUN_CHAIN_SCRIPT)?;
                ctx.eval::<(), _>(RUN_POLICIES_SCRIPT)?;
                for (id, script) in sources {
                    let wrapped = format!(
                        "globalThis.__aiSwitchHooks[{id:?}] = (function() {{\n\
                         \"use strict\";\n{script}\n\
                         return {{\n\
                         onRequest: typeof onRequest === \"function\" ? onRequest : null,\n\
                         onResponse: typeof onResponse === \"function\" ? onResponse : null,\n\
                         onEvent: typeof onEvent === \"function\" ? onEvent : null,\n\
                         policy: typeof policy === \"function\" ? policy : null\n\
                         }};\n}} )();"
                    );
                    ctx.eval::<(), _>(wrapped)?;
                }
                Ok::<(), rquickjs::Error>(())
            })
            .map_err(|error| app_error("plugin.js_load_failed", error.to_string()))?;
        *middleware().lock().unwrap_or_else(|error| error.into_inner()) = next;
        CONFIGURED.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn ensure_configured(paths: &AppPaths) {
        if CONFIGURED.load(Ordering::Relaxed) {
            return;
        }
        let _ = Self::configure(paths);
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
    fn invoke_runner(&self, name: &str, a: String, b: String, c: String) -> Option<String> {
        let deadline = Arc::new(Instant::now() + HOOK_TIMEOUT);
        self.runtime
            .set_interrupt_handler(Some(Box::new(move || Instant::now() >= *deadline)));
        let result: Option<String> = self.context.with(|ctx| {
            let func: Function = ctx.globals().get(name).ok()?;
            func.call((a, b, c)).ok()
        });
        self.runtime.set_interrupt_handler(None);
        result
    }

    fn run_chain(&self, body: &Value, ctx: &Value, entries: &[ChainEntry]) -> Option<ChainCallResult> {
        let body_json = serde_json::to_string(body).ok()?;
        let ctx_json = serde_json::to_string(ctx).ok()?;
        let entries_json = serde_json::to_string(entries).ok()?;
        let text = self.invoke_runner("__aiSwitchRunChain", body_json, ctx_json, entries_json)?;
        let value: Value = serde_json::from_str(&text).ok()?;
        let bytes = match value.get("bytes").and_then(Value::as_str) {
            Some(encoded) => Some(base64::engine::general_purpose::STANDARD.decode(encoded).ok()?),
            None => None,
        };
        let mut headers = Vec::new();
        if let Some(map) = value.get("headers").and_then(Value::as_object) {
            for (key, header_value) in map {
                headers.push((key.clone(), match header_value {
                    Value::Null => None,
                    Value::String(text) => Some(text.clone()),
                    other => Some(other.to_string()),
                }));
            }
        }
        let body = match value.get("body").and_then(Value::as_str) {
            Some(text) => serde_json::from_str(text).unwrap_or_else(|_| body.clone()),
            None => body.clone(),
        };
        Some(ChainCallResult { body, bytes, headers })
    }

    fn run_policies(&self, entries: &[PolicyEntry], ctx: &Value) -> Option<Value> {
        let entries_json = serde_json::to_string(entries).ok()?;
        let ctx_json = serde_json::to_string(ctx).ok()?;
        let text = self.invoke_runner("__aiSwitchRunPolicies", entries_json, ctx_json, String::new())?;
        serde_json::from_str(&text).ok()
    }
}

/// Invoke one plugin's body hook as a single-entry chain.
pub fn apply_plugin_hook(id: &str, hook: &str, body: &[u8], ctx: &Value) -> HookResult {
    PluginMiddleware::ensure_configured(current_paths());
    let Ok(original) = serde_json::from_slice::<Value>(body) else {
        return HookResult { body: Value::Null, ..Default::default() };
    };
    let config = ctx.get("config").cloned().unwrap_or_else(|| json!({}));
    let entry = ChainEntry { id: id.into(), hook: hook.into(), config };
    let guard = middleware().lock().unwrap_or_else(|error| error.into_inner());
    match guard.run_chain(&original, ctx, &[entry]) {
        Some(result) => HookResult { body: result.body, bytes: result.bytes, headers: result.headers },
        None => HookResult { body: original, ..Default::default() },
    }
}

/// Invoke one schedule plugin's `policy()` hook.
pub fn policy_hook(id: &str, config: &Value, ctx: &Value) -> Option<Value> {
    PluginMiddleware::ensure_configured(current_paths());
    let entry = PolicyEntry { id: id.into(), config: config.clone() };
    let guard = middleware().lock().unwrap_or_else(|error| error.into_inner());
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
        .map(|(id, config)| PolicyEntry { id: id.clone(), config: config.clone() })
        .collect();
    let guard = middleware().lock().unwrap_or_else(|error| error.into_inner());
    guard.run_policies(&refs, ctx)
}

/// Run every chain entry whose plugin stage matches `stage`, in one JS call.
/// Built-in stages come from the embedded manifest; third-party packs run at
/// `perAttempt` on the request lane and `response` on the response lane.
pub fn apply_chain(pipeline: &PipelineConfig, body: &[u8], stage: &str, base_ctx: &Value) -> ChainOutcome {
    if stage == "policy" {
        return ChainOutcome { bytes: body.to_vec(), ..Default::default() };
    }
    let hook = if stage == "response" { "onResponse" } else { "onRequest" };
    let mut entries = Vec::new();
    let directions: [(bool, &crate::models::pipeline::DirectionConfig); 2] = [
        (true, &pipeline.request),
        (false, &pipeline.response),
    ];
    for (request, direction) in directions {
        if pipeline.is_passthrough(request) {
            continue;
        }
        for plugin in direction.chain.iter().filter(|p| p.enabled) {
            let plugin_stage = if plugin.id.starts_with("pack:") {
                if request { "perAttempt".to_string() } else { "response".to_string() }
            } else {
                crate::services::builtin_plugins::stage_for(&plugin.id)
            };
            if plugin_stage != stage {
                continue;
            }
            entries.push(ChainEntry {
                id: plugin.id.clone(),
                hook: hook.into(),
                config: plugin.config.clone(),
            });
        }
    }
    if entries.is_empty() {
        return ChainOutcome { bytes: body.to_vec(), ..Default::default() };
    }
    let Ok(original) = serde_json::from_slice::<Value>(body) else {
        return ChainOutcome { bytes: body.to_vec(), ..Default::default() };
    };
    PluginMiddleware::ensure_configured(current_paths());
    let guard = middleware().lock().unwrap_or_else(|error| error.into_inner());
    match guard.run_chain(&original, base_ctx, &entries) {
        Some(result) => ChainOutcome {
            bytes: result.bytes.unwrap_or_else(|| serde_json::to_vec(&result.body).unwrap_or_else(|_| body.to_vec())),
            headers: result.headers,
        },
        None => ChainOutcome { bytes: body.to_vec(), ..Default::default() },
    }
}

/// Build the rich hook context from the data available at a proxy stage.
pub fn hook_context(platform: &str, path: &str, stage: &str, base_url: Option<&str>,
                    dialect: Option<&str>, headers: Option<&std::collections::HashMap<String, String>>,
                    raw_body_base64: Option<&str>, attempt: Option<usize>) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("platform".into(), json!(platform));
    map.insert("path".into(), json!(path));
    map.insert("phase".into(), json!(if stage == "response" { "onResponse" } else { "onRequest" }));
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
    use crate::models::pipeline::{DirectionConfig, PipelineConfig, PipelineMode, PluginInstance, PIPELINE_VERSION};
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
        PluginInstance { id: id.into(), enabled: true, config }
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
        let ctx = hook_context("codex", "/v1/responses", "retryRecovery", None, None, None, None, None);
        let result = apply_plugin_hook("builtin.request.contentRejection", "onRequest", &body, &ctx);
        assert!(result.bytes.is_none());
        let value: Value = serde_json::from_slice(&serde_json::to_vec(&result.body).unwrap()).unwrap();
        let outputs: Vec<_> = value["input"].as_array().unwrap().iter()
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
            "codex", "/v1/responses", "wire", Some("https://ps.air-outer.com/v1/responses"),
            None, Some(&headers), Some(&base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &body)), Some(1),
        );
        let pipeline = PipelineConfig {
            version: PIPELINE_VERSION,
            request: DirectionConfig {
                mode: PipelineMode::Process,
                chain: vec![plugin("builtin.request.compression", json!({ "mode": "on" }))],
            },
            response: DirectionConfig::default(),
        };
        let outcome = apply_chain(&pipeline, &body, "wire", &ctx);
        assert_ne!(outcome.bytes, body);
        assert!(outcome.headers.iter().any(|(key, value)| key == "content-encoding" && value.as_deref() == Some("br")));
    }

    #[test]
    fn chain_only_runs_plugins_for_its_stage() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let _dir = setup();
        let body = serde_json::to_vec(&json!({ "model": "m", "input": "hi" })).unwrap();
        let ctx = hook_context("codex", "/v1/responses", "wire", None, None, None, None, None);
        let pipeline = PipelineConfig {
            version: PIPELINE_VERSION,
            request: DirectionConfig {
                mode: PipelineMode::Process,
                chain: vec![plugin("builtin.request.compression", json!({ "mode": "off" }))],
            },
            response: DirectionConfig::default(),
        };
        let outcome = apply_chain(&pipeline, &body, "postBridgeReminder", &ctx);
        assert_eq!(outcome.bytes, body);
        assert!(outcome.headers.is_empty());
    }
}


