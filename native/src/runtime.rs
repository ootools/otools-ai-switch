use crate::app_state::AppState;
use crate::paths::AppPaths;
use crate::services::{route_proxy_service::RouteProxyService, settings_service::SettingsService};
use crate::web::event_bridge::{EventEmitter, WebEventBroadcaster};
use serde_json::{json, Value};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinHandle;

static PLUGIN: OnceLock<(Mutex<Slot>, Condvar)> = OnceLock::new();
static CANCELLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) fn is_cancelled() -> bool {
    CANCELLED.load(std::sync::atomic::Ordering::Acquire)
}
enum Slot {
    Empty,
    Running(PluginRuntime),
    Closing,
    Retired,
}
struct PluginRuntime {
    runtime: Runtime,
    state: Arc<AppState>,
    tasks: Vec<JoinHandle<()>>,
    commands: Vec<JoinHandle<()>>,
    log_writer: Option<JoinHandle<()>>,
    active: Arc<(Mutex<usize>, Condvar)>,
}
struct ActiveCommand(Arc<(Mutex<usize>, Condvar)>);
impl Drop for ActiveCommand {
    fn drop(&mut self) {
        let (lock, wake) = &*self.0;
        let mut count = lock.lock().unwrap_or_else(|e| e.into_inner());
        *count -= 1;
        wake.notify_all();
    }
}

impl PluginRuntime {
    fn new() -> Result<Self, String> {
        CANCELLED.store(false, std::sync::atomic::Ordering::Release);
        let runtime = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("otools-ai-switch")
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let (state, tasks, log_writer) = runtime.block_on(async {
            let paths = AppPaths::resolve()?;
            paths.ensure().await?;
            let pool = crate::database::open_migrated_pool(&paths.database_file, &paths.backups_dir).await?;
            let settings = SettingsService::load(&paths).await?;
            crate::services::proxy_service::apply_startup(&settings);
            let route_proxy = crate::services::route_proxy_service::RouteProxyRuntimeState::default();
            route_proxy.set_request_body_limit_mib(settings.route_proxy_request_body_limit_mib);
            let state = Arc::new(AppState {
                paths, pool, route_proxy,
                // Exists before any task or event emitter can run.
                event_broadcaster: Arc::new(WebEventBroadcaster::new()),
                config_writes: Default::default(), deeplink_protocols: Default::default(), close_to_tray: Default::default(),
            });
            let emitter = EventEmitter::Web(state.event_broadcaster.clone());
            state.route_proxy.activity().set_emitter(emitter.clone());
            state.route_proxy.live_log().set_emitter(emitter);
            let log_writer = state.route_proxy.live_log().persist_to(state.paths.logs_dir.clone()).map(|writer| tokio::spawn(writer.run()));
            let mut tasks = Vec::new();
            let pool = state.pool.clone();
            let activity = state.route_proxy.activity();
            tasks.push(tokio::spawn(async move { crate::services::route_recovery_service::RouteRecoveryService::run_loop(pool, activity).await; }));
            let video_state = (*state).clone();
            tasks.push(tokio::spawn(async move { crate::videogen::VideoGenerationService::run_loop(video_state).await; }));
            tasks.push(tokio::spawn(crate::services::usage_response_id_backfill_service::backfill_upstream_response_ids_after_startup(state.pool.clone())));
            tasks.push(tokio::spawn(crate::services::usage_history_compaction_service::compress_stored_previews_after_startup(state.pool.clone())));
            if let Ok(contents) = tokio::fs::read_to_string(state.paths.data_dir.join("model-prices.json")).await {
                let _ = crate::services::model_pricing::load_overrides_from_str(&contents);
            }
            let config = crate::services::route_proxy_https_service::RouteProxyHttpsService::load_config(&state.paths).await?;
            if config.auto_start {
                if let Err(error) = crate::services::route_proxy_https_service::RouteProxyHttpsService::start_proxy(&state).await {
                    state.event_broadcaster.send("route-proxy-error", &json!({"message": error.to_string(), "details": error.details()}));
                }
            }
            Ok::<_, crate::error::AppError>((state, tasks, log_writer))
        }).map_err(|e| match e.details() { Some(detail) => format!("{e}: {detail}"), None => e.to_string() })?;
        Ok(Self {
            runtime,
            state,
            tasks,
            log_writer,
            commands: Vec::new(),
            active: Arc::new((Mutex::new(0), Condvar::new())),
        })
    }

    fn shutdown(self) {
        CANCELLED.store(true, std::sync::atomic::Ordering::Release);
        let Self {
            runtime,
            state,
            tasks,
            commands,
            log_writer,
            active,
        } = self;
        runtime.block_on(async {
            // Cancel commands first, including ones waiting on network or a
            // listener lock; join them before shutting down owned resources.
            for task in &commands {
                task.abort();
            }
            for task in commands {
                let _ = task.await;
            }
            let _ = RouteProxyService::force_stop(&state.route_proxy).await;
            crate::web::handlers::shutdown_oauth().await;
            for task in &tasks {
                task.abort();
            }
            for task in tasks {
                let _ = task.await;
            }
            state.route_proxy.live_log().stop_persisting();
            if let Some(writer) = log_writer {
                let _ = writer.await;
            }
            state.pool.close().await;
        });
        drop(state);
        // Wait for runtime workers and blocking work before DLL unload.
        drop(runtime);
        let (lock, wake) = &*active;
        let mut count = lock.lock().unwrap_or_else(|e| e.into_inner());
        while *count != 0 {
            count = wake.wait(count).unwrap_or_else(|e| e.into_inner());
        }
        crate::services::proxy_service::clear_proxy_env();
    }
}

pub(crate) fn invoke(method: &str, payload: Value) -> Result<Value, String> {
    let (lock, wake) = PLUGIN.get_or_init(|| (Mutex::new(Slot::Empty), Condvar::new()));
    // Startup migration may be lengthy. A page polling events must never wait
    // behind initialization, and shutdown must not pretend it has completed.
    if method == "poll_events" {
        return Ok(
            json!({"events": lock.try_lock().ok().and_then(|slot| match &*slot {
            Slot::Running(plugin) => Some(plugin.state.event_broadcaster.drain()), _ => None,
        }).unwrap_or_default()}),
        );
    }
    let mut guard = if method == "__otools_shutdown__" {
        lock.try_lock().map_err(|_| {
            "Plugin is initializing or changing lifecycle; retry shutdown".to_string()
        })?
    } else {
        lock.lock().unwrap_or_else(|e| e.into_inner())
    };
    if method == "__otools_shutdown__" {
        while matches!(*guard, Slot::Closing) {
            guard = wake.wait(guard).unwrap_or_else(|e| e.into_inner());
        }
        let previous = std::mem::replace(&mut *guard, Slot::Closing);
        drop(guard);
        if let Slot::Running(plugin) = previous {
            plugin.shutdown();
        }
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = Slot::Retired;
        wake.notify_all();
        return Ok(Value::Null);
    }
    if matches!(*guard, Slot::Closing) {
        return Err("Native plugin is shutting down".into());
    }
    if method == "__otools_initialize__" {
        if !matches!(*guard, Slot::Running(_)) {
            *guard = Slot::Running(PluginRuntime::new()?);
        }
        return Ok(Value::Null);
    }
    if matches!(*guard, Slot::Retired) && method != "poll_events" {
        return Err("Native plugin is shut down; explicit initialization is required".into());
    }
    if method == "poll_events" {
        let events = match &*guard {
            Slot::Running(plugin) => plugin.state.event_broadcaster.drain(),
            _ => Vec::new(),
        };
        return Ok(json!({"events": events}));
    }
    if matches!(*guard, Slot::Empty) {
        *guard = Slot::Running(PluginRuntime::new()?);
    }
    let Slot::Running(plugin) = &mut *guard else {
        unreachable!()
    };
    let state = plugin.state.clone();
    let method = method.to_string();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    plugin.commands.retain(|task| !task.is_finished());
    plugin.commands.push(plugin.runtime.spawn(async move {
        let result = if method == "plugin_http_port" {
            Ok(json!({"port": RouteProxyService::status(&state.route_proxy).await.port}))
        } else {
            crate::web::handlers::dispatch_command(state, &method, payload)
                .await
                .map_err(|error| {
                    serde_json::to_string(&error).unwrap_or_else(|_| "Native command failed".into())
                })
        };
        let _ = sender.send(result);
    }));
    *plugin.active.0.lock().unwrap_or_else(|e| e.into_inner()) += 1;
    let _active = ActiveCommand(plugin.active.clone());
    // A long model/network operation never blocks event polling or shutdown.
    drop(guard);
    receiver
        .recv()
        .unwrap_or_else(|_| Err("Native command cancelled or panicked".into()))
}
