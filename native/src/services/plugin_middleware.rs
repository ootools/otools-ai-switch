//! Gateway middleware plugins (Magpie-style request/response hooks).
//!
//! Each enabled pack may ship a `middleware.js` defining any of:
//!
//! ```js
//! function onRequest(body, ctx) { return body; }
//! function onResponse(body, ctx) { return body; }
//! function onEvent(event, ctx) { return event; }
//! ```
//!
//! Hooks receive and return plain JSON values. They run in an embedded QuickJS
//! runtime with **no host functions**: no filesystem, no network, no `require`.
//! The runtime is interrupted after a short deadline and any error is
//! fail-open — the original body is forwarded unchanged, exactly like Magpie.

use crate::error::AppError;
use crate::models::settings::AppSettings;
use crate::paths::AppPaths;
use rquickjs::{Context, Runtime};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const HOOK_TIMEOUT: Duration = Duration::from_millis(50);
const MIDDLEWARE_FILE: &str = "middleware.js";
const MAX_SCRIPT_BYTES: usize = 256 * 1024;

static MIDDLEWARE: OnceLock<Mutex<PluginMiddleware>> = OnceLock::new();
static CONFIGURED: AtomicBool = AtomicBool::new(false);
static PATHS: OnceLock<AppPaths> = OnceLock::new();

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
            AppPaths::resolve().unwrap_or_else(|_| {
                AppPaths::from_data_dir(std::path::PathBuf::from("."))
            })
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
}

impl PluginMiddleware {
    fn empty() -> Result<Self, AppError> {
        let runtime = Runtime::new()
            .map_err(|error| app_error("plugin.js_runtime_failed", error.to_string()))?;
        let context = Context::full(&runtime)
            .map_err(|error| app_error("plugin.js_context_failed", error.to_string()))?;
        Ok(Self { runtime, context })
    }

    fn read_sources(paths: &AppPaths) -> Vec<(String, String)> {
        let settings: AppSettings = std::fs::read_to_string(&paths.settings_file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| {
                AppSettings::defaults_for_data_dir(paths.data_dir.display().to_string())
            });
        let enabled: std::collections::BTreeSet<String> = settings
            .plugins_enabled_json
            .as_deref()
            .and_then(|text| serde_json::from_str::<Vec<String>>(text).ok())
            .map(|ids| ids.into_iter().collect())
            .unwrap_or_default();

        let mut sources = Vec::new();
        for id in enabled {
            // The id is validated at install time; still re-check here because
            // settings may have been edited by hand.
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

    /// Rebuild the JS context from all enabled middleware scripts. Hooks are
    /// stored per pack id so an account chain can invoke only selected packs.
    pub fn configure(paths: &AppPaths) -> Result<(), AppError> {
        let mut next = Self::empty()?;
        let sources = Self::read_sources(paths);
        if !sources.is_empty() {
            next.context
                .with(|ctx| {
                    ctx.eval::<(), _>("globalThis.__aiSwitchHooks = {};")?;
                    for (id, script) in sources {
                        let wrapped = format!(
                            "globalThis.__aiSwitchHooks[{id:?}] = (function() {{\n\
                             \"use strict\";\n{script}\n\
                             return {{\n\
                             onRequest: typeof onRequest === \"function\" ? onRequest : null,\n\
                             onResponse: typeof onResponse === \"function\" ? onResponse : null,\n\
                             onEvent: typeof onEvent === \"function\" ? onEvent : null\n\
                             }};\n}} )();"
                        );
                        ctx.eval::<(), _>(wrapped)?;
                    }
                    Ok::<(), rquickjs::Error>(())
                })
                .map_err(|error| app_error("plugin.js_load_failed", error.to_string()))?;
        }
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

    fn call_plugin_hook(&self, id: &str, hook: &str, body: &Value, ctx: &Value) -> Option<Value> {
        let body_json = serde_json::to_string(body).ok()?;
        let ctx_json = serde_json::to_string(ctx).ok()?;
        let deadline = Arc::new(Instant::now() + HOOK_TIMEOUT);
        self.runtime
            .set_interrupt_handler(Some(Box::new(move || Instant::now() >= *deadline)));
        let script = format!(
            r#"(function(){{
                var body = JSON.parse({body_json:?});
                var meta = JSON.parse({ctx_json:?});
                var hooks = globalThis.__aiSwitchHooks || {{}};
                var h = hooks[{id:?}];
                if (!h || typeof h.{hook} !== "function") return null;
                var next = h.{hook}(body, meta);
                return (next === undefined || next === null) ? null : JSON.stringify(next);
            }})()"#
        );
        let result = self
            .context
            .with(|ctx| ctx.eval::<Option<String>, _>(script.as_str()));
        self.runtime.set_interrupt_handler(None);
        result
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    }
}

/// Invoke one installed pack's hook for an account-chain step. Fail-open:
/// any error, timeout, non-JSON body or missing hook leaves the body untouched.
pub fn apply_pack_hook(id: &str, hook: &str, body: &[u8], platform: &str, path: &str) -> Vec<u8> {
    PluginMiddleware::ensure_configured(current_paths());
    let Ok(original) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    let meta = json!({ "platform": platform, "path": path, "phase": hook });
    let guard = middleware().lock().unwrap_or_else(|error| error.into_inner());
    match guard.call_plugin_hook(id, hook, &original, &meta) {
        Some(next) => serde_json::to_vec(&next).unwrap_or_else(|_| body.to_vec()),
        None => body.to_vec(),
    }
}
