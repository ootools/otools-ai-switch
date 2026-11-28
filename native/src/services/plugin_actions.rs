//! The only middleware account-write executor. Targets are bound by the caller;
//! a hook cannot choose an account or another plugin's storage namespace.
use crate::error::AppError;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
pub static LIFECYCLE: tokio::sync::RwLock<()> = tokio::sync::RwLock::const_new(());
fn db(e: sqlx::Error) -> AppError {
    AppError::Database {
        code: "plugin.action.database",
        message: "Could not apply middleware actions".into(),
        details: Some(e.to_string()),
        recoverable: true,
    }
}
pub async fn snapshot(pool: &SqlitePool, account: &str, plugin: &str) -> Result<Value, AppError> {
    let row=sqlx::query("SELECT id,status,transient_failure_count,cooldown_until,last_failure_message FROM route_credentials WHERE id=?").bind(account).fetch_one(pool).await.map_err(db)?;
    let mut state = serde_json::Map::new();
    for (key, value) in sqlx::query_as::<_, (String, String)>(
        "SELECT state_key,value_json FROM plugin_scoped_state WHERE account_id=? AND plugin_id=?",
    )
    .bind(account)
    .bind(plugin)
    .fetch_all(pool)
    .await
    .map_err(db)?
    {
        state.insert(key, serde_json::from_str(&value).unwrap_or(Value::Null));
    }
    Ok(
        json!({"account":{"id":row.get::<String,_>("id"),"status":row.get::<String,_>("status"),"failure_count":row.get::<i64,_>("transient_failure_count"),"cooldown_until":row.get::<Option<String>,_>("cooldown_until"),"last_failure_message":row.get::<Option<String>,_>("last_failure_message")},"state":state}),
    )
}
pub async fn execute(
    pool: &SqlitePool,
    account: &str,
    plugin: &str,
    event_id: &str,
    hook: &str,
    actions: &[Value],
) -> Result<bool, AppError> {
    let _lifecycle = LIFECYCLE.read().await;
    let Some(d) = crate::services::plugin_contract::descriptor(plugin) else {
        return Ok(false);
    };
    for action in actions {
        crate::services::plugin_host::validate_action(&d.capabilities, hook, action).map_err(
            |message| AppError::Validation {
                code: "plugin.action.invalid",
                message,
                details: Some(plugin.into()),
                recoverable: true,
            },
        )?;
    }
    let writes: Vec<_> = actions
        .iter()
        .filter(|a| {
            matches!(
                a["type"].as_str(),
                Some("account.update" | "account.success" | "state.set" | "state.increment")
            )
        })
        .collect();
    if writes.is_empty() {
        return Ok(false);
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.map_err(db)?;
    let current: Option<String> = sqlx::query_scalar(
        "SELECT config_json FROM route_credentials WHERE id=? AND archived_at IS NULL",
    )
    .bind(account)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;
    let Some(current) = current else {
        return Ok(false);
    };
    let pipeline = crate::models::pipeline::resolve_pipeline(&current);
    if !crate::services::plugin_host::enabled(&pipeline)
        .iter()
        .any(|p| p.id == plugin)
    {
        return Ok(false);
    }
    let inserted=sqlx::query("INSERT OR IGNORE INTO plugin_effect_receipts(account_id,plugin_id,event_id,created_at) VALUES(?,?,?,?)").bind(account).bind(plugin).bind(event_id).bind(chrono::Utc::now().to_rfc3339()).execute(&mut *tx).await.map_err(db)?.rows_affected();
    if inserted == 0 {
        return Ok(false);
    }
    let mut changed = false;
    for a in writes {
        match a["type"].as_str().unwrap_or("") {
            "account.update" => {
                let status = a["status"].as_str();
                let cooldown = a["cooldown_seconds"].as_u64().map(|s| {
                    (chrono::Utc::now() + chrono::Duration::seconds(s as i64)).to_rfc3339()
                });
                let result=sqlx::query("UPDATE route_credentials SET status=COALESCE(?,status),cooldown_until=COALESCE(?,cooldown_until),next_retry_at=COALESCE(?,next_retry_at),updated_at=? WHERE id=? AND status NOT IN ('paused','revoked')").bind(status).bind(&cooldown).bind(&cooldown).bind(chrono::Utc::now().to_rfc3339()).bind(account).execute(&mut *tx).await.map_err(db)?;
                changed |= result.rows_affected() > 0;
            }
            "account.success" => {
                changed |= sqlx::query("UPDATE route_credentials SET transient_failure_count=0,semantic_failure_streak_count=0,semantic_failure_streak_fingerprint=NULL,cooldown_until=NULL,next_retry_at=NULL,status='ok',updated_at=? WHERE id=? AND status NOT IN ('paused','revoked')").bind(chrono::Utc::now().to_rfc3339()).bind(account).execute(&mut *tx).await.map_err(db)?.rows_affected()>0;
            }
            "state.set" => {
                sqlx::query("INSERT INTO plugin_scoped_state(account_id,plugin_id,state_key,value_json) VALUES(?,?,?,?) ON CONFLICT(account_id,plugin_id,state_key) DO UPDATE SET value_json=excluded.value_json").bind(account).bind(plugin).bind(a["key"].as_str().unwrap()).bind(a["value"].to_string()).execute(&mut *tx).await.map_err(db)?;
            }
            "state.increment" => {
                sqlx::query("INSERT INTO plugin_scoped_state(account_id,plugin_id,state_key,value_json) VALUES(?,?,?,?) ON CONFLICT(account_id,plugin_id,state_key) DO UPDATE SET value_json=CAST(CAST(value_json AS INTEGER) + CAST(excluded.value_json AS INTEGER) AS TEXT)").bind(account).bind(plugin).bind(a["key"].as_str().unwrap()).bind(a["amount"].as_i64().unwrap_or(1).to_string()).execute(&mut *tx).await.map_err(db)?;
            }
            _ => {}
        }
    }
    // Bounded per-account receipts; request events only need recent duplicate suppression.
    sqlx::query("DELETE FROM plugin_effect_receipts WHERE account_id=? AND rowid NOT IN (SELECT rowid FROM plugin_effect_receipts WHERE account_id=? ORDER BY rowid DESC LIMIT 4096)").bind(account).bind(account).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    crate::services::plugin_host::log(plugin, "info", &format!("Committed actions for {event_id}"));
    Ok(changed)
}
pub async fn dispatch_account_event(
    pool: &SqlitePool,
    account: &str,
    config_json: &str,
    event_id: &str,
    hook: &str,
    event: &Value,
) -> Result<bool, AppError> {
    let pipeline = crate::models::pipeline::resolve_pipeline(config_json);
    let mut changed = false;
    for p in crate::services::plugin_host::enabled(&pipeline) {
        let Some(binding) = crate::services::plugin_middleware::loaded(&p.id) else {
            continue;
        };
        let Some(d) = binding.descriptor.as_ref() else {
            continue;
        };
        if !d.hooks.iter().any(|h| h == hook) {
            continue;
        }
        let mut context = snapshot(pool, account, &p.id).await?;
        context["event_id"] = json!(event_id);
        if !d.capabilities.iter().any(|c| c == "account.read") {
            context.as_object_mut().unwrap().remove("account");
        }
        if !d.capabilities.iter().any(|c| c == "state.read") {
            context.as_object_mut().unwrap().remove("state");
        }
        let outcome = crate::services::plugin_host::invoke_bound(
            &binding, &p.id, hook, event, &context, &p.config,
        );
        if outcome.error.is_some() {
            continue;
        }
        let result = execute(pool, account, &p.id, event_id, hook, &outcome.actions).await;
        match result {
            Ok(value) => changed |= value,
            Err(error) => crate::services::plugin_host::log(&p.id, "error", &error.to_string()),
        }
    }
    Ok(changed)
}
