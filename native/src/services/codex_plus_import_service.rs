//! Reads provider entries out of Codex++'s own configuration
//! (`~/.codex-session-delete/`).
//!
//! Clean-room: this reads Codex++'s public on-disk formats only and shares no
//! code with it. Three files contribute entries, each missing file contributes
//! zero, all three missing means there is nothing to import:
//!
//! - `settings.json` — `relayProfiles[]`, the direct-connection relays
//!   (camelCase `RelayProfile` objects; `authContents` holds the Codex
//!   `auth.json` snapshot of official-mode relays).
//! - `gateway.json` — `providers[].relay`, the gateway pool. Relays whose id
//!   already came from `settings.json` are skipped: the gateway pool normally
//!   mirrors it, and both use the same `relay:<id>` source-id namespace so a
//!   relay moving between the two files does not orphan its binding.
//! - `claude-providers.json` — the Claude tool's providers; their
//!   `settingsConfig` is Claude `settings.json`-shaped, so cc-switch's Claude
//!   extraction handles them unchanged.

use crate::error::AppError;
use crate::models::external_client_import::EXTERNAL_CLIENT_MAX_ITEMS;
use crate::models::platform::{ApiDialect, PlatformId};
use crate::models::route_credential::ModelMapping;
use crate::services::cc_switch_import_service::{self as cc_switch};
use crate::services::cpa_import_service::parse_cpa_text;
use crate::services::external_client_import_service::{
    PreparedBody, PreparedExternalEntry, SkipBucket,
};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

fn home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|base| base.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Codex++'s state directory (its legacy name predates the manager app).
pub fn default_state_dir() -> PathBuf {
    match std::env::var("CODEX_PLUS_STATE_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        Some(value) => PathBuf::from(value),
        None => default_state_base_dir(),
    }
}

fn default_state_base_dir() -> PathBuf {
    home_dir().join(".codex-session-delete")
}

/// Whether the `settings.path` pointer may redirect settings.json for `dir`.
///
/// Mirrors Codex++: the pointer belongs to the default single-instance layout —
/// an explicit directory choice (or a `CODEX_PLUS_STATE_DIR` override) is
/// authoritative and never follows it.
fn pointer_allowed_for(dir: &Path) -> bool {
    let overridden = std::env::var("CODEX_PLUS_STATE_DIR")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    !overridden && dir == default_state_base_dir()
}

pub fn resolve_source_dir(source_path: Option<&str>) -> Result<PathBuf, AppError> {
    match source_path.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_dir() {
                return Err(AppError::Validation {
                    code: "external_import.source_missing",
                    message: "The selected Codex++ state directory does not exist".to_string(),
                    details: Some(path.display().to_string()),
                    recoverable: true,
                });
            }
            Ok(path)
        }
        None => Ok(default_state_dir()),
    }
}

struct CodexPlusSources {
    settings: Option<PathBuf>,
    gateway: Option<PathBuf>,
    claude_providers: Option<PathBuf>,
}

fn resolve_sources(dir: &Path, pointer_allowed: bool) -> CodexPlusSources {
    let mut settings = dir.join("settings.json");
    if !settings.is_file() && pointer_allowed {
        // settings.path 是状态目录的兄弟文件，内容为 settings.json 的绝对路径。
        if let Ok(content) = std::fs::read_to_string(dir.with_file_name("settings.path")) {
            let target = PathBuf::from(content.trim());
            if target.is_absolute() {
                settings = target;
            }
        }
    }
    CodexPlusSources {
        settings: settings.is_file().then_some(settings),
        gateway: dir
            .join("gateway.json")
            .is_file()
            .then(|| dir.join("gateway.json")),
        claude_providers: dir
            .join("claude-providers.json")
            .is_file()
            .then(|| dir.join("claude-providers.json")),
    }
}

async fn read_json_file(path: &Path) -> Result<Option<Value>, AppError> {
    let text = match tokio::fs::read_to_string(path).await {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(read_error(path, error));
        }
    };
    serde_json::from_str::<Value>(&text)
        .map(Some)
        .map_err(|error| AppError::Validation {
            code: "external_import.source_invalid_json",
            message: "The Codex++ configuration file is not valid JSON".to_string(),
            details: Some(format!("{}: {error}", path.display())),
            recoverable: true,
        })
}

fn read_error(path: &Path, error: std::io::Error) -> AppError {
    AppError::Filesystem {
        code: "external_import.source_read_failed",
        message: "Could not read the Codex++ configuration file".to_string(),
        details: Some(format!("{}: {error}", path.display())),
        recoverable: true,
    }
}

pub async fn prepare_entries(dir: &Path) -> Result<Vec<PreparedExternalEntry>, AppError> {
    let sources = resolve_sources(dir, pointer_allowed_for(dir));
    if sources.settings.is_none()
        && sources.gateway.is_none()
        && sources.claude_providers.is_none()
    {
        return Err(AppError::Validation {
            code: "external_import.source_not_found",
            message: "Could not find a Codex++ configuration on this machine".to_string(),
            details: Some(dir.display().to_string()),
            recoverable: true,
        });
    }
    let settings = match &sources.settings {
        Some(path) => read_json_file(path).await?,
        None => None,
    };
    let gateway = match &sources.gateway {
        Some(path) => read_json_file(path).await?,
        None => None,
    };
    let claude_providers = match &sources.claude_providers {
        Some(path) => read_json_file(path).await?,
        None => None,
    };

    let settings_relays = settings
        .as_ref()
        .and_then(|value| value.get("relayProfiles"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // Gateway relays whose id a settings relay already claimed are skipped:
    // the direct-connection copy is the authority.
    let settings_relay_ids: HashSet<String> = settings_relays
        .iter()
        .filter_map(|relay| value_string_field(relay, "id"))
        .collect();

    let gateway_relays: Vec<Value> = gateway
        .as_ref()
        .and_then(|value| value.get("providers"))
        .and_then(Value::as_array)
        .map(|providers| {
            providers
                .iter()
                .filter_map(|provider| provider.get("relay").cloned())
                .filter(|relay| {
                    value_string_field(relay, "id")
                        .map(|id| !settings_relay_ids.contains(&id))
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();

    // A settings relay may reference the gateway's auth snapshot via
    // officialAccountRef; index gateway relays by id for that lookup.
    let mut gateway_by_id: HashMap<String, Map<String, Value>> = HashMap::new();
    for relay in settings_relays.iter().chain(gateway_relays.iter()) {
        if let (Some(id), Some(object)) = (
            value_string_field(relay, "id"),
            relay.as_object().cloned(),
        ) {
            gateway_by_id.entry(id).or_insert(object);
        }
    }

    let mut entries = Vec::new();
    for relay in &settings_relays {
        prepare_relay(relay, &gateway_by_id, &mut entries);
    }
    for relay in &gateway_relays {
        prepare_relay(relay, &gateway_by_id, &mut entries);
    }
    if let Some(providers) = claude_providers
        .as_ref()
        .and_then(|value| value.get("providers"))
        .and_then(Value::as_array)
    {
        for provider in providers {
            prepare_claude_provider(provider, &mut entries);
        }
    }

    if entries.len() > EXTERNAL_CLIENT_MAX_ITEMS {
        return Err(AppError::Validation {
            code: "external_import.too_many_items",
            message: "The Codex++ configuration contains too many providers".to_string(),
            details: Some(entries.len().to_string()),
            recoverable: true,
        });
    }
    Ok(entries)
}

fn prepare_relay(
    relay: &Value,
    gateway_by_id: &HashMap<String, Map<String, Value>>,
    entries: &mut Vec<PreparedExternalEntry>,
) {
    let object = match relay.as_object() {
        Some(object) => object,
        None => return,
    };
    let id = string_field(object, "id").unwrap_or_default();
    if id.is_empty() {
        return;
    }
    let display_name = string_field(object, "name").unwrap_or_else(|| id.clone());
    let relay_mode = string_field(object, "relayMode").unwrap_or_else(|| "mixedApi".to_string());

    if relay_mode.eq_ignore_ascii_case("aggregate") {
        entries.push(PreparedExternalEntry {
            source_id: format!("relay:{id}"),
            body: PreparedBody::Skipped(SkipBucket::Unsupported("aggregate".to_string())),
        });
        return;
    }

    let api_key = extract_api_key(object);
    let auth_snapshot = extract_auth_snapshot(object, gateway_by_id);
    let official_mix = object.get("officialMixApiKey").and_then(Value::as_bool) == Some(true);

    match relay_mode.as_str() {
        "official" => {
            push_official(
                format!("relay:{id}:official"),
                &display_name,
                auth_snapshot.as_deref(),
                entries,
            );
        }
        _ => {
            if api_key.is_some() {
                push_api(format!("relay:{id}:api"), &display_name, object, entries);
            } else {
                // 无 key（noAuth 也在内）：官方快照是唯一可导入的凭据。
                push_official(
                    format!("relay:{id}:official"),
                    &display_name,
                    auth_snapshot.as_deref(),
                    entries,
                );
            }
            if official_mix && api_key.is_some() {
                push_official(
                    format!("relay:{id}:official"),
                    &display_name,
                    auth_snapshot.as_deref(),
                    entries,
                );
            }
        }
    }
}

/// `apiKey` → `apiKeys[activeApiKeyId]` → first key carrying a value.
fn extract_api_key(relay: &Map<String, Value>) -> Option<String> {
    if let Some(key) = string_field(relay, "apiKey") {
        return Some(key);
    }
    let keys = relay.get("apiKeys").and_then(Value::as_array)?;
    let active_id = string_field(relay, "activeApiKeyId");
    let carrying = |key: &Value| value_string_field(key, "apiKey").is_some();
    let selected = active_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .and_then(|id| {
            keys.iter()
                .find(|key| value_string_field(key, "id").as_deref() == Some(id) && carrying(key))
        })
        .or_else(|| keys.iter().find(|key| carrying(key)));
    selected.and_then(|key| value_string_field(key, "apiKey"))
}

/// The relay's own `authContents`, or the gateway relay's when
/// `officialAccountRef` points at one.
fn extract_auth_snapshot(
    relay: &Map<String, Value>,
    gateway_by_id: &HashMap<String, Map<String, Value>>,
) -> Option<String> {
    if let Some(own) = string_field(relay, "authContents") {
        return Some(own);
    }
    let reference = string_field(relay, "officialAccountRef").filter(|value| !value.is_empty())?;
    gateway_by_id
        .get(&reference)
        .and_then(|gateway_relay| string_field(gateway_relay, "authContents"))
}

fn push_api(
    source_id: String,
    display_name: &str,
    relay: &Map<String, Value>,
    entries: &mut Vec<PreparedExternalEntry>,
) {
    let no_auth = relay.get("noAuth").and_then(Value::as_bool) == Some(true);
    let Some(api_key) = extract_api_key(relay).filter(|_| !no_auth) else {
        entries.push(PreparedExternalEntry {
            source_id,
            body: PreparedBody::Broken {
                kind: "api".to_string(),
                display_name: display_name.to_string(),
                issue_code: "external_import.api_key_missing",
                platform_hint: Some(protocol_platform(relay)),
            },
        });
        return;
    };

    let base_url = string_field(relay, "upstreamBaseUrl")
        .or_else(|| string_field(relay, "baseUrl"))
        .unwrap_or_default();
    if !is_importable_base_url(&base_url) {
        entries.push(PreparedExternalEntry {
            source_id,
            body: PreparedBody::Broken {
                kind: "api".to_string(),
                display_name: display_name.to_string(),
                issue_code: "external_import.upstream_missing",
                platform_hint: Some(protocol_platform(relay)),
            },
        });
        return;
    }

    let mut extra_headers = Vec::new();
    if let Some(headers) = relay.get("customHeaders").and_then(Value::as_array) {
        for header in headers {
            if let (Some(name), Some(value)) = (
                value_string_field(header, "key"),
                value_string_field(header, "value"),
            ) {
                extra_headers.push((name, value));
            }
        }
    }
    let model_mappings = string_field(relay, "modelList")
        .map(|list| {
            list.lines()
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(|model| ModelMapping {
                    from: model.to_string(),
                    to: model.to_string(),
                    ..Default::default()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    entries.push(PreparedExternalEntry {
        source_id,
        body: PreparedBody::Api(cc_switch::ExtractedApiCredential {
            platform: protocol_platform(relay),
            display_name: display_name.to_string(),
            api_key,
            base_url,
            interface_format: protocol_dialect(relay),
            api_key_field: None,
            model_mappings,
            user_agent: string_field(relay, "userAgent"),
            extra_headers,
        }),
    });
}

fn push_official(
    source_id: String,
    display_name: &str,
    auth_snapshot: Option<&str>,
    entries: &mut Vec<PreparedExternalEntry>,
) {
    let mut broken = |issue_code: &'static str| {
        entries.push(PreparedExternalEntry {
            source_id: source_id.clone(),
            body: PreparedBody::Broken {
                kind: "official".to_string(),
                display_name: display_name.to_string(),
                issue_code,
                platform_hint: Some(PlatformId::Codex),
            },
        });
    };

    let Some(snapshot) = auth_snapshot else {
        broken("external_import.official_snapshot_missing");
        return;
    };
    let Some(normalized) = normalize_codex_auth(&snapshot) else {
        broken("external_import.official_snapshot_missing");
        return;
    };
    match parse_cpa_text(&normalized) {
        Ok(credentials) => {
            for credential in credentials {
                entries.push(PreparedExternalEntry {
                    source_id: source_id.clone(),
                    body: PreparedBody::Official(ParsedOfficialCredential {
                        display_name: display_name.to_string(),
                        ..credential
                    }),
                });
            }
        }
        Err(_) => broken("external_import.official_snapshot_invalid"),
    }
}

/// Flattens Codex's native `auth.json` (`{auth_mode, tokens:{...}}`) into the
/// flat CPA shape `parse_cpa_text` reads, tagging it as Codex.
fn normalize_codex_auth(snapshot: &str) -> Option<String> {
    let value: Value = serde_json::from_str(snapshot).ok()?;
    let mut object = value.as_object()?.clone();
    if let Some(tokens) = object.remove("tokens") {
        if let Some(token_map) = tokens.as_object() {
            for (key, token) in token_map {
                object.entry(key.clone()).or_insert_with(|| token.clone());
            }
        }
    }
    if !object.contains_key("type") {
        object.insert("type".to_string(), Value::String("codex".to_string()));
    }
    let has_token = ["access_token", "refresh_token", "id_token"].iter().any(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    });
    has_token.then(|| Value::Object(object).to_string())
}

fn prepare_claude_provider(provider: &Value, entries: &mut Vec<PreparedExternalEntry>) {
    let object = match provider.as_object() {
        Some(object) => object,
        None => return,
    };
    let id = string_field(object, "id").unwrap_or_default();
    if id.is_empty() {
        return;
    }
    let display_name = string_field(object, "name").unwrap_or_else(|| id.clone());
    let settings_config = object.get("settingsConfig").cloned().unwrap_or(Value::Null);
    let synthesized = cc_switch::ExternalClientProvider {
        source_id: format!("claude:{id}"),
        app_type: "claude".to_string(),
        display_name: display_name.clone(),
        category: None,
        sort_index: 0,
        settings_config,
        meta: Value::Null,
    };
    let body = match cc_switch::extract_api_credential(&synthesized) {
        Ok(extracted) => PreparedBody::Api(extracted),
        Err(issue) => PreparedBody::Broken {
            kind: "api".to_string(),
            display_name,
            issue_code: issue.code(),
            platform_hint: Some(PlatformId::Claude),
        },
    };
    entries.push(PreparedExternalEntry {
        source_id: format!("claude:{id}"),
        body,
    });
}

fn protocol_platform(relay: &Map<String, Value>) -> PlatformId {
    protocol_dialect_platform(relay).1
}

fn protocol_dialect(relay: &Map<String, Value>) -> ApiDialect {
    protocol_dialect_platform(relay).0
}

/// `responses` → (Responses, Codex), `chatCompletions` → (Chat, Codex),
/// `anthropicMessages` → (Messages, Claude); the Codex CLI speaks Responses
/// natively, so an unstated protocol is Responses.
fn protocol_dialect_platform(relay: &Map<String, Value>) -> (ApiDialect, PlatformId) {
    match string_field(relay, "protocol").as_deref() {
        Some("chatCompletions") => (ApiDialect::OpenAi, PlatformId::Codex),
        Some("anthropicMessages") => (ApiDialect::Anthropic, PlatformId::Claude),
        _ => (ApiDialect::OpenAiResponses, PlatformId::Codex),
    }
}

/// Empty, non-HTTP or loopback-only URLs point at Codex++'s own protocol proxy
/// rather than a real upstream: without `upstreamBaseUrl` there is nothing to
/// import.
fn is_importable_base_url(base_url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(base_url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str().map(|host| {
        host.trim_matches(|bracket| bracket == '[' || bracket == ']')
            .to_ascii_lowercase()
    }) else {
        return false;
    };
    let loopback = host == "localhost"
        || host
            .parse::<std::net::Ipv4Addr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
        || host
            .parse::<std::net::Ipv6Addr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false);
    !loopback
}

fn string_field(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn value_string_field(value: &Value, key: &str) -> Option<String> {
    value.as_object().and_then(|map| string_field(map, key))
}

use crate::services::cpa_import_service::ParsedOfficialCredential;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::external_client_import::{
        EXTERNAL_CLIENT_CODEX_PLUS, EXTERNAL_CLIENT_MAX_ITEMS,
    };
    use serde_json::json;
    use std::sync::Mutex;

    /// Env and pointer lookups read process/global state; serialise the tests
    /// that touch them.
    static STATE_GUARD: Mutex<()> = Mutex::new(());

    async fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        tokio::fs::write(&path, content).await.expect("write fixture");
        path
    }

    async fn prepare_with(dir: &Path) -> Vec<PreparedExternalEntry> {
        prepare_entries(dir).await.expect("prepare entries")
    }

    #[tokio::test]
    async fn extracts_relay_chain_url_headers_and_model_list() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_file(
            dir.path(),
            "settings.json",
            r#"{
                "relayProfiles": [
                    {
                        "id": "r1", "name": "DeepSeek relay",
                        "upstreamBaseUrl": "https://api.deepseek.com/v1",
                        "baseUrl": "http://127.0.0.1:8642/v1",
                        "apiKeys": [
                            {"id": "k1", "name": "primary", "apiKey": "sk-primary"},
                            {"id": "k2", "name": "backup", "apiKey": "sk-backup"}
                        ],
                        "activeApiKeyId": "k2",
                        "protocol": "chatCompletions",
                        "relayMode": "pureApi",
                        "modelList": "deepseek-chat\ndeepseek-reasoner",
                        "customHeaders": [{"key": "X-Debug", "value": "1"}],
                        "userAgent": "agent/1"
                    },
                    {
                        "id": "r2", "name": "Responses relay",
                        "upstreamBaseUrl": "https://responses.example/v1",
                        "apiKey": "sk-direct",
                        "protocol": "responses",
                        "relayMode": "mixedApi"
                    }
                ]
            }"#,
        )
        .await;

        let entries = prepare_with(dir.path()).await;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source_id, "relay:r1:api");
        let PreparedBody::Api(first) = &entries[0].body else {
            panic!("r1 must be an API entry");
        };
        assert_eq!(first.platform, PlatformId::Codex);
        assert_eq!(first.interface_format, ApiDialect::OpenAi);
        // upstreamBaseUrl outranks baseUrl even when baseUrl is filled in.
        assert_eq!(first.base_url, "https://api.deepseek.com/v1");
        // key chain: apiKey absent → apiKeys[activeApiKeyId].
        assert_eq!(first.api_key, "sk-backup");
        assert_eq!(
            first.model_mappings,
            vec![
                ModelMapping {
                    from: "deepseek-chat".to_string(),
                    to: "deepseek-chat".to_string(),
                    ..Default::default()
                },
                ModelMapping {
                    from: "deepseek-reasoner".to_string(),
                    to: "deepseek-reasoner".to_string(),
                    ..Default::default()
                },
            ]
        );
        assert_eq!(
            first.extra_headers,
            vec![("X-Debug".to_string(), "1".to_string())]
        );
        assert_eq!(first.user_agent.as_deref(), Some("agent/1"));

        let PreparedBody::Api(second) = &entries[1].body else {
            panic!("r2 must be an API entry");
        };
        assert_eq!(entries[1].source_id, "relay:r2:api");
        assert_eq!(second.interface_format, ApiDialect::OpenAiResponses);
        assert_eq!(second.api_key, "sk-direct");
        assert!(second.model_mappings.is_empty());
    }

    #[tokio::test]
    async fn official_relay_becomes_a_codex_official_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_file(
            dir.path(),
            "settings.json",
            r#"{
                "relayProfiles": [{
                    "id": "of1", "name": "ChatGPT 订阅",
                    "relayMode": "official",
                    "authContents": "{\"auth_mode\":\"chatgpt\",\"tokens\":{\"id_token\":\"id-token\",\"access_token\":\"at-token\",\"refresh_token\":\"rt-token\",\"account_id\":\"acc-1\"}}"
                }]
            }"#,
        )
        .await;

        let entries = prepare_with(dir.path()).await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source_id, "relay:of1:official");
        let PreparedBody::Official(credential) = &entries[0].body else {
            panic!("official relay must produce an Official entry");
        };
        assert_eq!(credential.platform, "codex");
        assert_eq!(credential.display_name, "ChatGPT 订阅");
        let secret: serde_json::Value =
            serde_json::from_str(&credential.secret_payload_json).expect("secret payload");
        // 快照原样落库：不刷新、不改写。
        assert_eq!(secret["access_token"], "at-token");
        assert_eq!(secret["refresh_token"], "rt-token");
        assert_eq!(secret["account_id"], "acc-1");
        assert_eq!(secret["id_token"], "id-token");
    }

    #[tokio::test]
    async fn official_mix_produces_both_api_and_official_entries() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_file(
            dir.path(),
            "settings.json",
            r#"{
                "relayProfiles": [{
                    "id": "mix1", "name": "混合订阅",
                    "relayMode": "mixedApi",
                    "officialMixApiKey": true,
                    "upstreamBaseUrl": "https://relay.example/v1",
                    "apiKey": "sk-mix",
                    "authContents": "{\"tokens\":{\"access_token\":\"at\",\"refresh_token\":\"rt\"}}"
                }]
            }"#,
        )
        .await;

        let entries = prepare_with(dir.path()).await;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source_id, "relay:mix1:api");
        assert!(matches!(&entries[0].body, PreparedBody::Api(_)));
        assert_eq!(entries[1].source_id, "relay:mix1:official");
        assert!(matches!(&entries[1].body, PreparedBody::Official(_)));
    }

    #[tokio::test]
    async fn aggregate_relays_are_skipped_as_unsupported() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_file(
            dir.path(),
            "settings.json",
            r#"{
                "relayProfiles": [{
                    "id": "agg1", "name": "聚合", "relayMode": "aggregate"
                }]
            }"#,
        )
        .await;

        let entries = prepare_with(dir.path()).await;
        assert_eq!(entries.len(), 1);
        match &entries[0].body {
            PreparedBody::Skipped(SkipBucket::Unsupported(label)) => {
                assert_eq!(label, "aggregate");
            }
            other => panic!("aggregate must be skipped, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn gateway_providers_are_appended_without_duplicating_settings_relays() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_file(
            dir.path(),
            "settings.json",
            r#"{
                "relayProfiles": [{
                    "id": "g1", "name": "直连", "relayMode": "pureApi",
                    "upstreamBaseUrl": "https://direct.example/v1", "apiKey": "sk-a"
                }]
            }"#,
        )
        .await;
        write_file(
            dir.path(),
            "gateway.json",
            r#"{
                "providers": [
                    {"relay": {"id": "g1", "name": "网关镜像", "relayMode": "pureApi", "upstreamBaseUrl": "https://mirror.example/v1", "apiKey": "sk-mirror"}},
                    {"relay": {"id": "g2", "name": "网关独有", "relayMode": "pureApi", "upstreamBaseUrl": "https://gateway.example/v1", "apiKey": "sk-g2"}}
                ]
            }"#,
        )
        .await;

        let entries = prepare_with(dir.path()).await;
        let ids: Vec<&str> = entries
            .iter()
            .map(|entry| entry.source_id.as_str())
            .collect();
        // settings.json 的 g1 赢，gateway 的 g1 不重复；g2 以同一 relay: 命名空间追加。
        assert_eq!(ids, vec!["relay:g1:api", "relay:g2:api"]);
        let PreparedBody::Api(kept) = &entries[0].body else {
            panic!("settings relay must win");
        };
        assert_eq!(kept.base_url, "https://direct.example/v1");
    }

    #[tokio::test]
    async fn claude_tool_providers_become_claude_api_entries() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_file(
            dir.path(),
            "claude-providers.json",
            r#"{
                "version": 1,
                "currentId": "c1",
                "providers": [{
                    "id": "c1",
                    "name": "Claude relay",
                    "settingsConfig": {
                        "env": {
                            "ANTHROPIC_AUTH_TOKEN": "sk-claude",
                            "ANTHROPIC_BASE_URL": "https://claude.example",
                            "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-opus-5[1M]"
                        }
                    }
                }]
            }"#,
        )
        .await;

        let entries = prepare_with(dir.path()).await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source_id, "claude:c1");
        let PreparedBody::Api(extracted) = &entries[0].body else {
            panic!("claude provider must be an API entry");
        };
        assert_eq!(extracted.platform, PlatformId::Claude);
        assert_eq!(extracted.interface_format, ApiDialect::Anthropic);
        assert_eq!(extracted.api_key, "sk-claude");
        assert_eq!(extracted.base_url, "https://claude.example");
        assert_eq!(extracted.model_mappings.len(), 1);
    }

    #[tokio::test]
    async fn loopback_base_url_without_upstream_is_broken() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_file(
            dir.path(),
            "settings.json",
            r#"{
                "relayProfiles": [{
                    "id": "shell", "name": "代理壳",
                    "relayMode": "pureApi",
                    "baseUrl": "http://127.0.0.1:8642/v1",
                    "apiKey": "sk-x"
                }]
            }"#,
        )
        .await;

        let entries = prepare_with(dir.path()).await;
        match &entries[0].body {
            PreparedBody::Broken { issue_code, .. } => {
                assert_eq!(*issue_code, "external_import.upstream_missing");
            }
            other => panic!("loopback shell must be broken, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn state_dir_env_and_settings_pointer_are_honoured() {
        let _guard = STATE_GUARD.lock().expect("guard");
        let outer = tempfile::tempdir().expect("temp dir");
        let state = outer.path().join("state");
        tokio::fs::create_dir_all(&state).await.expect("state dir");
        let real_settings = outer.path().join("codex-settings.json");
        tokio::fs::write(
            &real_settings,
            r#"{"relayProfiles":[{"id":"p1","name":"指针","relayMode":"pureApi","upstreamBaseUrl":"https://pointer.example/v1","apiKey":"sk-p"}]}"#,
        )
        .await
        .expect("write pointed settings");
        // settings.path 指针是状态目录的兄弟文件（与 Codex++ 一致）。
        tokio::fs::write(
            outer.path().join("settings.path"),
            real_settings.to_string_lossy().as_bytes(),
        )
        .await
        .expect("write pointer");

        // 环境变量覆盖缺省状态目录：settings.json 直接放在覆盖目录里。
        tokio::fs::write(
            state.join("settings.json"),
            r#"{"relayProfiles":[{"id":"p1","name":"环境变量","relayMode":"pureApi","upstreamBaseUrl":"https://env.example/v1","apiKey":"sk-env"}]}"#,
        )
        .await
        .expect("write state settings");
        std::env::set_var("CODEX_PLUS_STATE_DIR", &state);
        let resolved = resolve_source_dir(None).expect("default dir");
        assert_eq!(resolved, state);
        let entries = prepare_entries(&resolved).await.expect("default dir");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source_id, "relay:p1:api");
        std::env::remove_var("CODEX_PLUS_STATE_DIR");
    }

    #[test]
    fn settings_pointer_redirects_only_when_allowed() {
        // settings.path 指针是状态目录的兄弟文件（与 Codex++ 一致）。
        let outer = tempfile::tempdir().expect("temp dir");
        let state = outer.path().join("state");
        std::fs::create_dir_all(&state).expect("state dir");
        let real_settings = outer.path().join("codex-settings.json");
        std::fs::write(
            &real_settings,
            r#"{"relayProfiles":[{"id":"p1","name":"指针","relayMode":"pureApi","upstreamBaseUrl":"https://pointer.example/v1","apiKey":"sk-p"}]}"#,
        )
        .expect("write pointed settings");
        std::fs::write(
            outer.path().join("settings.path"),
            real_settings.to_string_lossy().as_bytes(),
        )
        .expect("write pointer");

        // 指针被允许时：settings.json 缺失则读指针目标。
        let sources = resolve_sources(&state, true);
        let pointed = sources.settings.expect("pointer honoured");
        assert_eq!(pointed, real_settings);
        // 指针被禁止时（显式目录 / 多开覆盖）：三文件全缺就是全缺。
        let sources = resolve_sources(&state, false);
        assert!(sources.settings.is_none());
        assert!(sources.gateway.is_none());
        assert!(sources.claude_providers.is_none());
    }

    #[tokio::test]
    async fn missing_files_report_not_found_and_invalid_json_is_named() {
        let empty = tempfile::tempdir().expect("temp dir");
        let error = prepare_entries(empty.path()).await.expect_err("no files");
        assert!(matches!(
            error,
            AppError::Validation {
                code: "external_import.source_not_found",
                ..
            }
        ));

        let dir = tempfile::tempdir().expect("temp dir");
        write_file(dir.path(), "settings.json", "{ not json").await;
        let error = prepare_entries(dir.path()).await.expect_err("invalid json");
        assert!(matches!(
            error,
            AppError::Validation {
                code: "external_import.source_invalid_json",
                ..
            }
        ));
        let details = match &error {
            AppError::Validation { details, .. } => details.clone().unwrap_or_default(),
            other => panic!("unexpected error {other:?}"),
        };
        assert!(details.contains("settings.json"));
    }

    #[tokio::test]
    async fn more_than_two_thousand_entries_are_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut relays = String::new();
        for index in 0..EXTERNAL_CLIENT_MAX_ITEMS + 1 {
            relays.push_str(&format!(
                r#"{{"id":"r{index}","name":"n{index}","relayMode":"pureApi","upstreamBaseUrl":"https://h{index}.example/v1","apiKey":"sk-{index}"}}"#
            ));
            if index < EXTERNAL_CLIENT_MAX_ITEMS {
                relays.push(',');
            }
        }
        write_file(
            dir.path(),
            "settings.json",
            &format!(r#"{{"relayProfiles":[{relays}]}}"#),
        )
        .await;

        let error = prepare_entries(dir.path()).await.expect_err("too many");
        assert!(matches!(
            error,
            AppError::Validation {
                code: "external_import.too_many_items",
                ..
            }
        ));
    }

    #[test]
    fn resolve_source_dir_rejects_a_missing_explicit_directory() {
        let error =
            resolve_source_dir(Some("C:/definitely/not/here/dir")).expect_err("missing dir");
        assert!(matches!(
            error,
            AppError::Validation {
                code: "external_import.source_missing",
                ..
            }
        ));
    }

    #[test]
    fn client_constant_is_pinned() {
        assert_eq!(EXTERNAL_CLIENT_CODEX_PLUS, "codex-plus");
    }
}
