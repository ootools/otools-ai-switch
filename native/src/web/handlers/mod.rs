use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::core::sessions::{get_session_messages_core, list_sessions_core};
use crate::core::settings::{get_settings_core, save_settings_core};
use crate::core::usage_history::{
    clear_response_bodies_core, compact_usage_history_core, get_usage_history_storage_core,
};
use crate::core::usage_overview::get_usage_overview_core;
use crate::core::usage_stats::{
    get_model_price_configs_core, get_session_usage_stats_core, reload_model_price_overrides_core,
    save_model_price_configs_core,
};
use crate::database::repositories::config_snapshot_repository::ConfigSnapshotRepository;
use crate::error::{ApiError, AppError};
use crate::models::batch::NewBatch;
use crate::models::batch::{CreateAccountRequest, UpdateAccountRequest};
use crate::models::external_client_import::{
    ImportExternalClientAccountsInput, PreviewExternalClientImportInput,
};
use crate::models::notification::NotificationChannelKind;
use crate::models::platform::PlatformId;
use crate::models::route_credential::{
    CopyRouteCredentialInput, CreateApiRouteCredentialInput, ImportOfficialFilesInput,
    ImportOfficialTextInput, ReorderRouteCredentialInput, RouteCredentialPageRequest,
    UpdateRouteCredentialInput, MASKED_SECRET_PAYLOAD,
};
use crate::models::route_credential_transfer::{
    ExportRouteCredentialsInput, ImportRouteCredentialsInput, PreviewRouteCredentialImportInput,
};
use crate::models::route_pool::{
    CreateRoutePoolGroupInput, DeleteRoutePoolGroupInput, RouteModelsFetchRequest,
    RoutePoolModelTestRequest, RoutePoolRouteRequest, SetRoutePoolBaselineModelsInput,
    SetRoutePoolGroupMembersInput, SetRoutePoolMembersInput, SetRoutePoolModelModeInput,
    UpdateRoutePoolGroupInput,
};
use crate::models::settings::AppSettings;
use crate::services::batch_service::BatchService;
use crate::services::config_write_service::ConfigWriteCoordinator;
use crate::services::disk_space_service::DiskSpaceService;
use crate::services::external_client_import_service;
use crate::services::import_service::{ExampleJsonImportRequest, ImportService};
use crate::services::model_pricing::ModelPriceConfig;
use crate::services::notification_service;
use crate::services::platform_capability_service::PlatformCapabilityService;
use crate::services::route_config_service::RouteConfigService;
use crate::services::route_credential_service::RouteCredentialService;
use crate::services::route_credential_transfer_service;
use crate::services::route_model_fetch_service::RouteModelFetchService;
use crate::services::route_model_test_service::RouteModelTestService;
use crate::services::route_pool_service::RoutePoolService;
use crate::services::route_proxy_https_service::RouteProxyHttpsService;
use crate::services::route_proxy_service::RouteProxyService;
use crate::services::route_quota_service::RouteQuotaService;
use crate::services::route_recovery_service::{RecoveryRule, RouteRecoveryService};
use crate::services::route_relay_balance_service::RouteRelayBalanceService;
use crate::services::target_service::TargetService;
use std::collections::HashMap;

pub async fn dispatch_command(
    state: Arc<AppState>,
    command: &str,
    args: Value,
) -> Result<Value, ApiError> {
    match command {
        "health" => to_value(json!({ "ok": true })),
        "start_codex_oauth" => {
            let input = parse_arg(&args, "input")?;
            to_value(
                oauth_runtime()
                    .await
                    .start(state.pool.clone(), input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "get_codex_oauth_status" => {
            let session_id = required_string_arg(&args, "session_id")?;
            to_value(
                oauth_runtime()
                    .await
                    .status(&session_id)
                    .await
                    .map_err(to_error)?,
            )
        }
        "cancel_codex_oauth" => {
            let session_id = required_string_arg(&args, "session_id")?;
            to_value(
                oauth_runtime()
                    .await
                    .cancel(&session_id)
                    .await
                    .map_err(to_error)?,
            )
        }
        "get_codex_login_state" => codex_login_state(),
        "enable_client_direct_mode" => {
            let credential_id = required_string_arg(&args, "credentialId")?;
            to_value(
                crate::services::direct_mode_service::DirectModeService::enable(
                    &state.paths,
                    &state.pool,
                    &state.config_writes,
                    &credential_id,
                )
                .await
                .map_err(to_error)?,
            )
        }
        "get_claude_slot_plan" => to_value(
            RouteConfigService::claude_slot_plan(&state.paths, &state.pool)
                .await
                .map_err(to_error)?,
        ),
        "create_provider" => {
            let request: crate::models::batch::CreateProviderRequest = parse_arg(&args, "request")?;
            to_value(
                BatchService::create_provider(&state.pool, request.provider, request.batch_id)
                    .await
                    .map_err(to_error)?,
            )
        }
        "save_route_credential_export" => save_credential_export(&state, &args).await,
        "save_route_proxy_diagnostics_export" => save_diagnostics_export(&state, &args).await,
        "open_route_proxy_https_certificate_dir" => {
            open_certificate_directory(&state.paths.route_proxy_https_dir).await?;
            to_value(())
        }
        "list_route_proxy_models" => {
            let platform = required_string_arg(&args, "platform")?;
            list_route_proxy_models(&state, &platform).await
        }
        "imagegen_list_sessions" => to_value(
            crate::imagegen::repository::ImageGenerationRepository::list_sessions(
                &state.pool,
                args.get("includeArchived")
                    .or_else(|| args.get("include_archived"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
            .await
            .map_err(to_error)?,
        ),
        "imagegen_create_session" => {
            let input = parse_arg(&args, "input")?;
            to_value(
                crate::imagegen::repository::ImageGenerationRepository::create_session(
                    &state.pool,
                    input,
                )
                .await
                .map_err(to_error)?,
            )
        }
        "imagegen_update_session" => {
            let input = parse_arg(&args, "input")?;
            to_value(
                crate::imagegen::repository::ImageGenerationRepository::update_session(
                    &state.pool,
                    input,
                )
                .await
                .map_err(to_error)?,
            )
        }
        "imagegen_delete_session" => {
            let id = required_string_arg(&args, "id")?;
            crate::imagegen::repository::ImageGenerationRepository::delete_session(
                &state.pool,
                &id,
            )
            .await
            .map_err(to_error)?;
            crate::imagegen::storage::delete_session_assets(&state.paths.imagegen_dir, &id)
                .await
                .map_err(to_error)?;
            to_value(())
        }
        "imagegen_get_conversation" => {
            let id = required_string_arg(&args, "id")?;
            to_value(
                crate::imagegen::repository::ImageGenerationRepository::conversation(
                    &state.pool,
                    &id,
                )
                .await
                .map_err(to_error)?,
            )
        }
        "imagegen_list_models" => {
            let platform = required_string_arg(&args, "platform")?;
            to_value(
                crate::imagegen::service::ImageGenerationService::list_models(&state, &platform)
                    .await
                    .map_err(to_error)?,
            )
        }
        "imagegen_generate" => {
            let input = parse_arg(&args, "input")?;
            to_value(
                crate::imagegen::service::ImageGenerationService::generate(&state, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "imagegen_read_asset" => {
            let id = required_string_arg(&args, "id")?;
            to_value(
                crate::imagegen::service::ImageGenerationService::read_asset(&state, &id)
                    .await
                    .map_err(to_error)?,
            )
        }
        "mcp_scan_local" => to_value(crate::mcp::service::scan_local().map_err(ApiError::from)?),
        "mcp_list_marketplaces" => to_value(crate::mcp::marketplace::list_marketplaces().await),
        "mcp_search_marketplace" => {
            let provider_id = required_string_arg(&args, "providerId")?;
            let query = optional_string_arg(&args, "query")?;
            let limit = optional_i64_arg(&args, "limit")?
                .map(|value| {
                    u32::try_from(value).map_err(|_| {
                        invalid_argument("limit", Some("expected non-negative integer".to_string()))
                    })
                })
                .transpose()?;
            to_value(
                crate::mcp::marketplace::search(provider_id, query, limit)
                    .await
                    .map_err(ApiError::from)?,
            )
        }
        "mcp_get_marketplace_server_detail" => {
            let provider_id = required_string_arg(&args, "providerId")?;
            let server_id = required_string_arg(&args, "serverId")?;
            to_value(crate::mcp::marketplace::get_detail(provider_id, server_id).await?)
        }
        "mcp_install_from_marketplace" => {
            let provider_id = required_string_arg(&args, "providerId")?;
            let server_id = required_string_arg(&args, "serverId")?;
            let apps = parse_arg(&args, "apps")?;
            let option_id = optional_string_arg(&args, "optionId")?;
            let protocol = optional_string_arg(&args, "protocol")?;
            let parameter_values = args.get("parameterValues").cloned();
            to_value(
                crate::mcp::marketplace::install(
                    provider_id,
                    server_id,
                    apps,
                    option_id,
                    protocol,
                    parameter_values,
                )
                .await?,
            )
        }
        "mcp_upsert_local_server" => {
            let server_id = required_string_arg(&args, "serverId")?;
            let spec: Value = parse_arg(&args, "spec")?;
            let apps = parse_arg(&args, "apps")?;
            to_value(
                crate::mcp::service::upsert_local_server(server_id, spec, apps)
                    .map_err(ApiError::from)?,
            )
        }
        "mcp_set_server_apps" => {
            let server_id = required_string_arg(&args, "serverId")?;
            let apps = parse_arg(&args, "apps")?;
            to_value(crate::mcp::service::set_server_apps(server_id, apps).map_err(ApiError::from)?)
        }
        "mcp_remove_server" => {
            let server_id = required_string_arg(&args, "serverId")?;
            let apps = args
                .get("apps")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|error| invalid_argument("apps", Some(error.to_string())))?;
            to_value(crate::mcp::service::remove_server(server_id, apps).map_err(ApiError::from)?)
        }
        "skills_list_agents" => to_value(crate::skills::service::list_agents()),
        "skills_list" => {
            let agent_type = parse_arg(&args, "agentType")?;
            let scope = parse_arg(&args, "scope")?;
            let workspace_path = optional_string_arg(&args, "workspacePath")?;
            to_value(
                crate::skills::service::list_skills(
                    agent_type,
                    scope,
                    workspace_path_ref(&workspace_path),
                )
                .map_err(ApiError::from)?,
            )
        }
        "skills_list_packages" => {
            let agent_type = parse_arg(&args, "agentType")?;
            let scope = parse_arg(&args, "scope")?;
            let workspace_path = optional_string_arg(&args, "workspacePath")?;
            to_value(
                crate::skills::packages::list_skill_packages(
                    agent_type,
                    scope,
                    workspace_path_ref(&workspace_path),
                )
                .map_err(ApiError::from)?,
            )
        }
        "skills_read_package" => {
            let package_id = required_string_arg(&args, "packageId")?;
            let agent_type = parse_arg(&args, "agentType")?;
            let scope = parse_arg(&args, "scope")?;
            let workspace_path = optional_string_arg(&args, "workspacePath")?;
            to_value(
                crate::skills::packages::read_skill_package(
                    &package_id,
                    agent_type,
                    scope,
                    workspace_path_ref(&workspace_path),
                )
                .map_err(ApiError::from)?,
            )
        }
        "skills_install_package" => {
            let package_id = required_string_arg(&args, "packageId")?;
            let agent_type = parse_arg(&args, "agentType")?;
            let scope = parse_arg(&args, "scope")?;
            let workspace_path = optional_string_arg(&args, "workspacePath")?;
            let skill_ids = optional_string_array_arg(&args, "skillIds")?;
            to_value(
                crate::skills::packages::install_skill_package(
                    &package_id,
                    agent_type,
                    scope,
                    workspace_path_ref(&workspace_path),
                    skill_ids,
                )
                .map_err(ApiError::from)?,
            )
        }
        "skills_uninstall_package" => {
            let package_id = required_string_arg(&args, "packageId")?;
            let agent_type = parse_arg(&args, "agentType")?;
            let scope = parse_arg(&args, "scope")?;
            let workspace_path = optional_string_arg(&args, "workspacePath")?;
            let skill_ids = optional_string_array_arg(&args, "skillIds")?;
            to_value(
                crate::skills::packages::uninstall_skill_package(
                    &package_id,
                    agent_type,
                    scope,
                    workspace_path_ref(&workspace_path),
                    skill_ids,
                )
                .map_err(ApiError::from)?,
            )
        }
        "skills_read" => {
            let agent_type = parse_arg(&args, "agentType")?;
            let scope = parse_arg(&args, "scope")?;
            let skill_id = required_string_arg(&args, "skillId")?;
            let workspace_path = optional_string_arg(&args, "workspacePath")?;
            to_value(
                crate::skills::service::read_skill(
                    agent_type,
                    scope,
                    &skill_id,
                    workspace_path_ref(&workspace_path),
                )
                .map_err(ApiError::from)?,
            )
        }
        "skills_save" => {
            let agent_type = parse_arg(&args, "agentType")?;
            let scope = parse_arg(&args, "scope")?;
            let skill_id = required_string_arg(&args, "skillId")?;
            let content = required_raw_string_arg(&args, "content")?;
            let layout = args
                .get("layout")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|error| invalid_argument("layout", Some(error.to_string())))?;
            let workspace_path = optional_string_arg(&args, "workspacePath")?;
            to_value(
                crate::skills::service::save_skill(
                    agent_type,
                    scope,
                    skill_id,
                    content,
                    layout,
                    workspace_path_ref(&workspace_path),
                )
                .map_err(ApiError::from)?,
            )
        }
        "skills_delete" => {
            let agent_type = parse_arg(&args, "agentType")?;
            let scope = parse_arg(&args, "scope")?;
            let skill_id = required_string_arg(&args, "skillId")?;
            let workspace_path = optional_string_arg(&args, "workspacePath")?;
            to_value(
                crate::skills::service::delete_skill(
                    agent_type,
                    scope,
                    skill_id,
                    workspace_path_ref(&workspace_path),
                )
                .map_err(ApiError::from)?,
            )
        }
        "list_batch_groups" => {
            let search = optional_string_arg(&args, "search")?;
            to_value(
                BatchService::list_groups(&state.pool, search)
                    .await
                    .map_err(to_error)?,
            )
        }
        "create_batch" => {
            let input: NewBatch = parse_arg(&args, "input")?;
            to_value(
                BatchService::create_batch(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "create_official_account" => {
            let request: CreateAccountRequest = parse_arg(&args, "request")?;
            to_value(
                BatchService::create_official_account(
                    &state.pool,
                    request.account,
                    request.batch_id,
                )
                .await
                .map_err(to_error)?,
            )
        }
        "get_official_account" => {
            let id = required_string_arg(&args, "id")?;
            to_value(
                BatchService::get_official_account(&state.pool, id)
                    .await
                    .map_err(to_error)?,
            )
        }
        "update_official_account" => {
            let input: UpdateAccountRequest = parse_arg(&args, "input")?;
            to_value(
                BatchService::update_official_account(&state.pool, input.id, input.account)
                    .await
                    .map_err(to_error)?,
            )
        }
        "import_example_json" => {
            let request: ExampleJsonImportRequest = parse_arg(&args, "request")?;
            to_value(
                ImportService::import_example_json(&state.pool, request)
                    .await
                    .map_err(to_error)?,
            )
        }
        "list_platform_capabilities" => to_value(PlatformCapabilityService::list()),
        "get_disk_space_status" => to_value(DiskSpaceService::status(&state.paths.data_dir)),
        "list_target_apps" => to_value(
            TargetService::list_targets(&state.pool)
                .await
                .map_err(to_error)?,
        ),
        "list_target_config_statuses" => to_value(
            TargetService::list_config_statuses(&state.paths, &state.pool, &state.config_writes)
                .await
                .map_err(to_error)?,
        ),
        "list_config_write_clients" => {
            let platform = required_string_arg(&args, "platform")?;
            let platform = PlatformId::parse(&platform).map_err(to_error)?;
            to_value(
                TargetService::list_config_write_clients_for_paths(
                    &state.paths,
                    &state.pool,
                    platform,
                )
                .await
                .map_err(to_error)?,
            )
        }
        "list_config_snapshots" => {
            let target_app_id = optional_string_arg(&args, "targetAppId")?;
            let limit = optional_i64_arg(&args, "limit")?
                .unwrap_or(50)
                .clamp(1, 200);
            ConfigWriteCoordinator::reconcile_prepared(
                &state.paths,
                &state.pool,
                &state.config_writes,
            )
            .await
            .map_err(to_error)?;
            to_value(
                ConfigSnapshotRepository::list(&state.pool, target_app_id.as_deref(), limit)
                    .await
                    .map_err(to_error)?,
            )
        }
        "rollback_config_snapshot" => {
            let id = required_string_arg(&args, "id")?;
            to_value(
                ConfigWriteCoordinator::rollback(
                    &state.paths,
                    &state.pool,
                    &state.config_writes,
                    &id,
                )
                .await
                .map_err(to_error)?,
            )
        }
        "get_settings" => to_value(
            get_settings_core(&state.paths, &state.deeplink_protocols)
                .await
                .map_err(to_error)?,
        ),
        "save_settings" => {
            let settings: AppSettings = parse_arg(&args, "settings")?;
            to_value(
                save_settings_core(
                    &state.paths,
                    &state.deeplink_protocols,
                    &state.close_to_tray,
                    &state.route_proxy,
                    settings,
                )
                .await
                .map_err(to_error)?,
            )
        }
        "list_sessions" => {
            let platform = optional_string_arg(&args, "platform")?;
            to_value(
                list_sessions_core(platform)
                    .await
                    .map_err(|message| command_error("web.session_scan", message))?,
            )
        }
        "get_session_usage_stats" => {
            let since = optional_string_arg(&args, "since")?;
            to_value(
                get_session_usage_stats_core(since)
                    .await
                    .map_err(to_error)?,
            )
        }
        "reload_model_price_overrides" => to_value(
            reload_model_price_overrides_core()
                .await
                .map_err(to_error)?,
        ),
        "get_model_price_configs" => {
            to_value(get_model_price_configs_core().await.map_err(to_error)?)
        }
        "save_model_price_configs" => {
            let configs: HashMap<String, ModelPriceConfig> = parse_arg(&args, "configs")?;
            to_value(
                save_model_price_configs_core(configs)
                    .await
                    .map_err(to_error)?,
            )
        }
        "get_usage_overview" => {
            let since = optional_string_arg(&args, "since")?;
            let page = optional_i64_arg(&args, "page")?;
            let page_size = optional_i64_arg(&args, "page_size")?;
            // The browser's own offset, so a phone in another timezone gets its
            // buckets cut at its midnight rather than the server's.
            let utc_offset_minutes = optional_i64_arg(&args, "utc_offset_minutes")?
                .and_then(|value| i32::try_from(value).ok());
            to_value(
                get_usage_overview_core(&state.pool, since, page, page_size, utc_offset_minutes)
                    .await
                    .map_err(to_error)?,
            )
        }
        "get_usage_history_storage" => to_value(
            get_usage_history_storage_core(&state.pool)
                .await
                .map_err(to_error)?,
        ),
        "compact_usage_history" => to_value(
            compact_usage_history_core(&state.pool)
                .await
                .map_err(to_error)?,
        ),
        "clear_usage_response_bodies" => {
            let older_than_days = optional_i64_arg(&args, "older_than_days")?.unwrap_or(0);
            to_value(
                clear_response_bodies_core(&state.pool, older_than_days)
                    .await
                    .map_err(to_error)?,
            )
        }
        "get_session_messages" => {
            let provider_id = required_string_arg(&args, "providerId")?;
            let source_path = required_string_arg(&args, "sourcePath")?;
            to_value(
                get_session_messages_core(provider_id, source_path)
                    .await
                    .map_err(|message| command_error("web.session_read", message))?,
            )
        }
        "list_route_credentials" => {
            let platform = required_string_arg(&args, "platform")?;
            let activity = state.route_proxy.activity();
            to_value(
                RouteCredentialService::list_with_activity(&state.pool, &activity, platform)
                    .await
                    .map_err(to_error)?,
            )
        }
        "export_route_credentials" => {
            let input: ExportRouteCredentialsInput = parse_arg(&args, "input")?;
            to_value(
                route_credential_transfer_service::export_route_credentials(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "list_route_credentials_page" => {
            let input: RouteCredentialPageRequest = parse_arg(&args, "input")?;
            let activity = state.route_proxy.activity();
            to_value(
                RouteCredentialService::page_with_activity(&state.pool, &activity, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "reorder_route_credentials" => {
            let input: ReorderRouteCredentialInput = parse_arg(&args, "input")?;
            to_value(
                RouteCredentialService::reorder(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "get_route_credential" => {
            let id = required_string_arg(&args, "id")?;
            let activity = state.route_proxy.activity();
            to_value(
                RouteCredentialService::get_with_activity(&state.pool, &activity, id)
                    .await
                    .map_err(to_error)?,
            )
        }
        "create_api_route_credential" => {
            let input: CreateApiRouteCredentialInput = parse_arg(&args, "input")?;
            to_value(
                RouteCredentialService::create_api(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "import_official_route_credentials_from_text" => {
            let input: ImportOfficialTextInput = parse_arg(&args, "input")?;
            to_value(
                RouteCredentialService::import_official_text(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "import_official_route_credentials_from_files" => {
            let input: ImportOfficialFilesInput = parse_arg(&args, "input")?;
            to_value(
                RouteCredentialService::import_official_files(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "update_route_credential" => {
            let id = required_string_arg(&args, "id")?;
            let input: UpdateRouteCredentialInput = parse_arg(&args, "input")?;
            to_value(
                RouteCredentialService::update(&state.pool, id, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "copy_route_credential" => {
            let id = required_string_arg(&args, "id")?;
            let input = args
                .get("input")
                .filter(|value| !value.is_null())
                .cloned()
                .map(serde_json::from_value::<CopyRouteCredentialInput>)
                .transpose()
                .map_err(|error| invalid_argument("input", Some(error.to_string())))?
                .unwrap_or_default();
            to_value(
                RouteCredentialService::copy_with_options(&state.pool, id, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "set_route_credential_recovery" => {
            let id = required_string_arg(&args, "id")?;
            let rule: RecoveryRule = parse_arg(&args, "rule")?;
            to_value(
                RouteRecoveryService::set_rule(&state.pool, id, rule)
                    .await
                    .map_err(to_error)?,
            )
        }
        "set_route_credential_model_status" => {
            let id = required_string_arg(&args, "id")?;
            let model_key = required_string_arg(&args, "model_key")?;
            let status = required_string_arg(&args, "status")?;
            to_value(
                RouteCredentialService::set_model_status(&state.pool, id, model_key, status)
                    .await
                    .map_err(to_error)?,
            )
        }
        "clear_route_credential_model_state" => {
            let id = required_string_arg(&args, "id")?;
            let model_key = required_string_arg(&args, "model_key")?;
            to_value(
                RouteCredentialService::clear_model_state(&state.pool, id, model_key)
                    .await
                    .map_err(to_error)?,
            )
        }
        "set_route_credential_cooldown" => {
            let id = required_string_arg(&args, "id")?;
            let seconds =
                optional_i64_arg(&args, "seconds")?.ok_or_else(|| missing_argument("seconds"))?;
            to_value(
                RouteCredentialService::set_cooldown(&state.pool, id, seconds)
                    .await
                    .map_err(to_error)?,
            )
        }
        "clear_route_credential_failure_state" => {
            let id = required_string_arg(&args, "id")?;
            to_value(
                RouteCredentialService::clear_failure_state(&state.pool, id)
                    .await
                    .map_err(to_error)?,
            )
        }
        "delete_route_credential" => {
            let id = required_string_arg(&args, "id")?;
            RouteCredentialService::delete(&state.pool, id)
                .await
                .map_err(to_error)?;
            to_value(())
        }
        "archive_route_credentials" => {
            let ids: Vec<String> = parse_arg(&args, "ids")?;
            RouteCredentialService::archive(&state.pool, ids)
                .await
                .map_err(to_error)?;
            to_value(())
        }
        "restore_route_credentials" => {
            let ids: Vec<String> = parse_arg(&args, "ids")?;
            RouteCredentialService::restore(&state.pool, ids)
                .await
                .map_err(to_error)?;
            to_value(())
        }
        "set_route_credential_statuses" => {
            let ids: Vec<String> = parse_arg(&args, "ids")?;
            let status = required_string_arg(&args, "status")?;
            RouteCredentialService::set_statuses(&state.pool, ids, status)
                .await
                .map_err(to_error)?;
            to_value(())
        }
        "refresh_route_credential_quota" => {
            let id = required_string_arg(&args, "id")?;
            to_value(
                RouteQuotaService::refresh_one(&state.pool, id)
                    .await
                    .map_err(to_error)?,
            )
        }
        "refresh_route_credentials_quota" => {
            let platform = required_string_arg(&args, "platform")?;
            to_value(
                RouteQuotaService::refresh_platform(&state.pool, platform)
                    .await
                    .map_err(to_error)?,
            )
        }
        "refresh_route_credential_relay_balance" => {
            let id = required_string_arg(&args, "id")?;
            to_value(
                RouteRelayBalanceService::refresh_one(&state.pool, id)
                    .await
                    .map_err(to_error)?,
            )
        }
        "refresh_route_credentials_relay_balance" => {
            let platform = required_string_arg(&args, "platform")?;
            to_value(
                RouteRelayBalanceService::refresh_platform(&state.pool, platform)
                    .await
                    .map_err(to_error)?,
            )
        }
        "get_route_pool" => {
            let platform = required_string_arg(&args, "platform")?;
            let group_id = optional_string_arg(&args, "group_id")?;
            let since = optional_string_arg(&args, "since")?;
            let request_page = optional_i64_arg(&args, "request_page")?;
            let request_page_size = optional_i64_arg(&args, "request_page_size")?;
            to_value(
                RoutePoolService::get_for_group(
                    &state.pool,
                    platform,
                    group_id,
                    since,
                    request_page,
                    request_page_size,
                )
                .await
                .map_err(to_error)?,
            )
        }
        "preview_route_credential_import" => {
            let input: PreviewRouteCredentialImportInput = parse_arg(&args, "input")?;
            to_value(
                crate::services::route_credential_transfer_import_service::preview_route_credential_import(
                    &state.pool,
                    input,
                )
                .await
                .map_err(to_error)?,
            )
        }
        "import_route_credentials" => {
            let input: ImportRouteCredentialsInput = parse_arg(&args, "input")?;
            to_value(
                crate::services::route_credential_transfer_import_service::import_route_credentials(
                    &state.pool,
                    input,
                )
                .await
                .map_err(to_error)?,
            )
        }
        "preview_external_client_import" => {
            let input: PreviewExternalClientImportInput = parse_arg(&args, "input")?;
            to_value(
                external_client_import_service::preview_external_client_import(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "import_external_client_accounts" => {
            let input: ImportExternalClientAccountsInput = parse_arg(&args, "input")?;
            to_value(
                external_client_import_service::import_external_client_accounts(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "set_route_pool_members" => {
            let input: SetRoutePoolMembersInput = parse_arg(&args, "input")?;
            to_value(
                RoutePoolService::set_members(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "create_route_pool_group" => {
            let input: CreateRoutePoolGroupInput = parse_arg(&args, "input")?;
            to_value(
                RoutePoolService::create_group(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "update_route_pool_group" => {
            let input: UpdateRoutePoolGroupInput = parse_arg(&args, "input")?;
            to_value(
                RoutePoolService::update_group(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "delete_route_pool_group" => {
            let input: DeleteRoutePoolGroupInput = parse_arg(&args, "input")?;
            to_value(
                RoutePoolService::delete_group(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "set_route_pool_group_members" => {
            let input: SetRoutePoolGroupMembersInput = parse_arg(&args, "input")?;
            to_value(
                RoutePoolService::set_group_members(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "move_route_pool_group_members" => {
            let input: SetRoutePoolGroupMembersInput = parse_arg(&args, "input")?;
            to_value(
                RoutePoolService::move_group_members(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "set_route_pool_model_mode" => {
            let input: SetRoutePoolModelModeInput = parse_arg(&args, "input")?;
            to_value(
                RoutePoolService::set_model_mode(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "set_route_pool_baseline_models" => {
            let input: SetRoutePoolBaselineModelsInput = parse_arg(&args, "input")?;
            to_value(
                RoutePoolService::set_baseline_models(&state.pool, input)
                    .await
                    .map_err(to_error)?,
            )
        }
        "route_pool_route_once" => {
            let request: RoutePoolRouteRequest = parse_arg(&args, "request")?;
            let activity = state.route_proxy.activity();
            to_value(
                RoutePoolService::route_once_with_activity(&state.pool, &activity, request)
                    .await
                    .map_err(to_error)?,
            )
        }
        "subscribe_route_proxy_live_log" => {
            let platform = required_string_arg(&args, "platform")?;
            to_value(state.route_proxy.live_log().subscribe(&platform))
        }
        "unsubscribe_route_proxy_live_log" => {
            state.route_proxy.live_log().unsubscribe();
            to_value(Value::Null)
        }
        "route_pool_test_model" => {
            let request: RoutePoolModelTestRequest = parse_arg(&args, "request")?;
            if route_model_test_targets_single_account(&request) {
                let activity = state.route_proxy.activity();
                to_value(
                    RouteModelTestService::test_model_with_activity(
                        &state.pool,
                        &activity,
                        request,
                    )
                    .await
                    .map_err(to_error)?,
                )
            } else {
                let base_url = route_model_test_proxy_base_url(state.as_ref()).await?;
                let root_certificate_pem = if base_url.starts_with("https://") {
                    Some(
                        RouteProxyHttpsService::load_root_certificate_pem(&state.paths)
                            .await
                            .map_err(to_error)?,
                    )
                } else {
                    None
                };
                to_value(
                    RouteModelTestService::test_model_through_proxy_with_root_certificate(
                        &state.pool,
                        request,
                        &base_url,
                        root_certificate_pem.as_deref(),
                    )
                    .await
                    .map_err(to_error)?,
                )
            }
        }
        "fetch_route_models" => {
            let request: RouteModelsFetchRequest = parse_arg(&args, "request")?;
            to_value(
                RouteModelFetchService::fetch(request)
                    .await
                    .map_err(to_error)?,
            )
        }
        "start_route_proxy" => to_value(
            RouteProxyHttpsService::start_proxy(state.as_ref())
                .await
                .map_err(to_error)?,
        ),
        "stop_route_proxy" => {
            let status = RouteProxyService::stop(&state.route_proxy)
                .await
                .map_err(to_error)?;
            RouteProxyHttpsService::clear_auto_start(&state.paths)
                .await
                .map_err(to_error)?;
            to_value(status)
        }
        "get_route_proxy_status" => to_value(RouteProxyService::status(&state.route_proxy).await),
        "get_route_proxy_key" => {
            let platform = required_string_arg(&args, "platform")?;
            to_value(
                RouteProxyService::get_or_create_platform_key(&state.pool, &platform)
                    .await
                    .map_err(to_error)?,
            )
        }
        "get_route_proxy_https_status" => to_value(
            RouteProxyHttpsService::status_for_state(state.as_ref())
                .await
                .map_err(to_error)?,
        ),
        "enable_route_proxy_https" => to_value(
            RouteProxyHttpsService::enable(state.as_ref())
                .await
                .map_err(to_error)?,
        ),
        "disable_route_proxy_https" => to_value(
            RouteProxyHttpsService::disable(state.as_ref())
                .await
                .map_err(to_error)?,
        ),
        "reimport_route_proxy_root_ca" => to_value(
            RouteProxyHttpsService::reimport_root_ca(state.as_ref())
                .await
                .map_err(to_error)?,
        ),
        "regenerate_route_proxy_https_certificates" => to_value(
            RouteProxyHttpsService::regenerate_certificates(state.as_ref())
                .await
                .map_err(to_error)?,
        ),
        "uninstall_route_proxy_root_ca" => to_value(
            RouteProxyHttpsService::uninstall_root_ca(state.as_ref())
                .await
                .map_err(to_error)?,
        ),
        "delete_route_proxy_https_certificates" => to_value(
            RouteProxyHttpsService::delete_certificates(state.as_ref())
                .await
                .map_err(to_error)?,
        ),
        "get_client_direct_modes" => to_value(
            crate::services::direct_mode_service::DirectModeService::statuses(&state.pool)
                .await
                .map_err(to_error)?,
        ),
        "write_route_proxy_configs" => {
            let base_url = optional_string_arg(&args, "baseUrl")?;
            let platform = optional_string_arg(&args, "platform")?.ok_or_else(|| {
                to_error(AppError::Validation {
                    code: "validation.route_config_platform_required",
                    message: "Route config platform is required".to_string(),
                    details: None,
                    recoverable: true,
                })
            })?;
            let client_keys = optional_string_array_arg(&args, "clientKeys")?;
            let status = RouteProxyService::status(&state.route_proxy).await;
            let resolved = base_url
                .filter(|value| !value.is_empty())
                .or(status.base_url)
                .ok_or_else(|| {
                    to_error(AppError::Validation {
                        code: "validation.route_proxy_not_running",
                        message: "Start the route proxy before writing config files".to_string(),
                        details: None,
                        recoverable: true,
                    })
                })?;
            to_value(
                RouteConfigService::write_configs(
                    &state.paths,
                    &state.pool,
                    &state.config_writes,
                    &resolved,
                    &platform,
                    client_keys.as_deref(),
                )
                .await
                .map_err(to_error)?,
            )
        }
        "route_config_write_is_stale" => {
            let base_url = optional_string_arg(&args, "baseUrl")?;
            let platform = optional_string_arg(&args, "platform")?.ok_or_else(|| {
                to_error(AppError::Validation {
                    code: "validation.route_config_platform_required",
                    message: "Route config platform is required".to_string(),
                    details: None,
                    recoverable: true,
                })
            })?;
            let client_keys = optional_string_array_arg(&args, "clientKeys")?;
            let status = RouteProxyService::status(&state.route_proxy).await;
            match base_url
                .filter(|value| !value.is_empty())
                .or(status.base_url)
            {
                Some(resolved) => to_value(
                    RouteConfigService::config_write_is_stale(
                        &state.paths,
                        &state.pool,
                        &resolved,
                        &platform,
                        client_keys.as_deref(),
                    )
                    .await,
                ),
                None => to_value(false),
            }
        }
        "test_notification" => {
            let kind: NotificationChannelKind = parse_arg(&args, "kind")?;
            notification_service::test_channel(&kind)
                .await
                .map_err(|message| command_error("web.notification_test", message))?;
            to_value(())
        }
        other => Err(ApiError::from(AppError::Validation {
            code: "web.command_unknown",
            message: "Web command is not recognized".to_string(),
            details: Some(other.to_string()),
            recoverable: false,
        })),
    }
}

// Kept outside AppState so the plugin's state contract stays independent of the
// optional Codex CLI. Shutdown clears the holder, allowing the plugin to reload.
static OAUTH_RUNTIME: tokio::sync::Mutex<Option<crate::services::codex_oauth::CodexOAuthRuntime>> =
    tokio::sync::Mutex::const_new(None);

async fn oauth_runtime() -> crate::services::codex_oauth::CodexOAuthRuntime {
    OAUTH_RUNTIME
        .lock()
        .await
        .get_or_insert_with(Default::default)
        .clone()
}

pub(crate) async fn shutdown_oauth() {
    let mut runtime = OAUTH_RUNTIME.lock().await;
    if let Some(current) = runtime.take() {
        current.shutdown().await;
    }
}

fn codex_login_state() -> Result<Value, ApiError> {
    use crate::adapters::route_config::{inspect_codex_login_state, CodexLoginState};
    let home = directories::BaseDirs::new().ok_or_else(|| {
        to_error(AppError::Filesystem {
            code: "filesystem.home_not_found",
            message: "Could not resolve the current user home directory".to_string(),
            details: None,
            recoverable: false,
        })
    })?;
    let (usable, state, auth_mode) =
        match inspect_codex_login_state(&home.home_dir().join(".codex/config.toml")) {
            CodexLoginState::Chatgpt => (true, "chatgpt", Some("chatgpt".to_string())),
            CodexLoginState::Missing => (false, "missing", None),
            CodexLoginState::Unreadable(_) => (false, "unreadable", None),
            CodexLoginState::NotChatgpt { auth_mode } => (false, "not_chatgpt", Some(auth_mode)),
        };
    Ok(json!({"usable": usable, "state": state, "auth_mode": auth_mode}))
}

/// Native management reads the same catalog as GET /v1/models without starting
/// a listener, minting a key or sending a browser request to its own localhost.
async fn list_route_proxy_models(state: &AppState, platform: &str) -> Result<Value, ApiError> {
    use crate::services::route_proxy_service::{build_proxy_state, proxy_handler};
    use axum::{
        body::Body,
        extract::State,
        http::{HeaderMap, Method},
    };
    let platform = PlatformId::parse(platform).map_err(to_error)?;
    let proxy =
        build_proxy_state(state.pool.clone(), &state.route_proxy).with_platform_scope(platform);
    let response = proxy_handler(
        State(proxy),
        Method::GET,
        HeaderMap::new(),
        "/v1/models".parse().expect("static URI"),
        Body::empty(),
    )
    .await;
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .map_err(|error| command_error("route_proxy.models_read", error.to_string()))?;
    let payload: Value = serde_json::from_slice(&bytes)
        .map_err(|error| command_error("route_proxy.models_invalid", error.to_string()))?;
    if !status.is_success() {
        return Err(command_error(
            "route_proxy.models_failed",
            payload
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Could not list route proxy models")
                .to_string(),
        ));
    }
    payload
        .get("data")
        .filter(|data| data.is_array())
        .cloned()
        .ok_or_else(|| {
            command_error(
                "route_proxy.models_invalid",
                "Model catalog response must contain a data array".to_string(),
            )
        })
}

fn normalize_json_file_name(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let mut remaining = lower.as_str();
    let mut suffixes = 0;
    while let Some(stripped) = remaining.strip_suffix(".json") {
        remaining = stripped;
        suffixes += 1;
    }
    match suffixes {
        0 => format!("{value}.json"),
        1 => value.to_string(),
        _ => format!("{}.json", &value[..remaining.len()]),
    }
}

fn export_path(
    state: &AppState,
    args: &Value,
    suggested: &str,
) -> Result<std::path::PathBuf, ApiError> {
    if let Some(path) = optional_string_arg(args, "path")? {
        let path = std::path::PathBuf::from(path);
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(invalid_argument(
                "path",
                Some("expected an absolute host file path".to_string()),
            ));
        }
        return Ok(path);
    }
    let basename = suggested
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("ai-switch-export");
    // Keep the display name, but isolate each export to avoid replacing an
    // earlier credential export when no explicit save destination was selected.
    let basename: String = basename
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                ch
            }
        })
        .collect();
    Ok(state
        .paths
        .data_dir
        .join("exports")
        .join(uuid::Uuid::new_v4().to_string())
        .join(basename))
}

async fn save_credential_export(state: &AppState, args: &Value) -> Result<Value, ApiError> {
    use crate::models::route_credential_transfer::{TRANSFER_MAX_BYTES, TRANSFER_MAX_ITEMS};
    let text = required_raw_string_arg(args, "json_text")?;
    if text.len() > TRANSFER_MAX_BYTES {
        return Err(command_error(
            "transfer.export_too_large",
            "The route credential export exceeds the maximum size".to_string(),
        ));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| command_error("transfer.export_invalid_json", error.to_string()))?;
    let items = value.as_array().ok_or_else(|| {
        command_error(
            "transfer.export_root_not_array",
            "The route credential export must be a top-level array".to_string(),
        )
    })?;
    if items.len() > TRANSFER_MAX_ITEMS {
        return Err(command_error(
            "transfer.export_too_many_items",
            "The route credential export contains too many items".to_string(),
        ));
    }
    let suggested = optional_string_arg(args, "suggested_file_name")?
        .unwrap_or_else(|| "ai-switch-route-credentials.json".to_string());
    // A save picker already confirmed the exact destination and any overwrite.
    // Normalize only the fallback suggestion, never the user's selected path.
    let path = export_path(state, args, &normalize_json_file_name(&suggested))?;
    crate::config_writer::ConfigWriter::write_atomic(&path, &text)
        .await
        .map_err(to_error)?;
    Ok(
        json!({"cancelled": false, "file_name": path.file_name().and_then(|name| name.to_str()), "path": path}),
    )
}

async fn save_diagnostics_export(state: &AppState, args: &Value) -> Result<Value, ApiError> {
    use crate::config_writer::ConfigWriter;
    use crate::services::route_proxy_diagnostics_service;
    let mut path = export_path(
        state,
        args,
        &route_proxy_diagnostics_service::suggested_file_name(),
    )?;
    if optional_string_arg(args, "path")?.is_none() && path.extension().is_none() {
        let name = format!(
            "{}.jsonl.{}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("ai-switch-diagnostics"),
            route_proxy_diagnostics_service::DIAGNOSTICS_FILE_EXTENSION
        );
        path.set_file_name(name);
    }
    // Inspect before building the potentially large bundle; concurrent changes
    // during compression must not be overwritten by this export.
    let expected = ConfigWriter::inspect(&path).await.map_err(to_error)?;
    let bundle = route_proxy_diagnostics_service::build(
        &state.pool,
        &state.paths,
        &state.route_proxy.live_log(),
    )
    .await
    .map_err(to_error)?;
    ConfigWriter::write_atomic_if_unchanged(&path, &bundle.bytes, &expected)
        .await
        .map_err(to_error)?;
    Ok(
        json!({"cancelled": false, "file_name": path.file_name().and_then(|name| name.to_str()), "path": path, "entries": bundle.entries, "byte_size": bundle.bytes.len()}),
    )
}

async fn open_certificate_directory(path: &std::path::Path) -> Result<(), ApiError> {
    if !path.is_dir() {
        return Err(command_error(
            "validation.route_proxy_https_certificate_dir_missing",
            "Generate local HTTPS certificates before opening their directory".to_string(),
        ));
    }
    // OTools native dispatch runs on the host, including for remote browsers.
    #[cfg(target_os = "windows")]
    let mut command = tokio::process::Command::new("explorer.exe");
    #[cfg(target_os = "macos")]
    let mut command = tokio::process::Command::new("open");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = tokio::process::Command::new("xdg-open");
    let mut child = command
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| {
            to_error(AppError::Filesystem {
                code: "filesystem.route_proxy_https_open_certificate_dir",
                message: "Could not open the local HTTPS certificate directory".to_string(),
                details: Some(error.to_string()),
                recoverable: true,
            })
        })?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

fn command_error(code: &'static str, message: String) -> ApiError {
    ApiError::from(AppError::Validation {
        code,
        message,
        details: None,
        recoverable: true,
    })
}

fn to_error(error: AppError) -> ApiError {
    ApiError::from(error)
}

fn to_value<T: Serialize>(value: T) -> Result<Value, ApiError> {
    serde_json::to_value(value).map_err(|error| {
        ApiError::from(AppError::Validation {
            code: "web.response_serialize",
            message: "Could not serialize the Web response".to_string(),
            details: Some(error.to_string()),
            recoverable: false,
        })
    })
}

fn parse_arg<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, ApiError> {
    let value = args
        .get(key)
        .cloned()
        .ok_or_else(|| missing_argument(key))?;
    serde_json::from_value(value).map_err(|error| invalid_argument(key, Some(error.to_string())))
}

fn optional_string_arg(args: &Value, key: &str) -> Result<Option<String>, ApiError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => {
            Ok(Some(text.trim().to_string()).filter(|value| !value.is_empty()))
        }
        Some(_) => Err(invalid_argument(key, Some("expected string".to_string()))),
    }
}

fn workspace_path_ref(workspace_path: &Option<String>) -> Option<&std::path::Path> {
    workspace_path.as_deref().map(std::path::Path::new)
}

fn optional_string_array_arg(args: &Value, key: &str) -> Result<Option<Vec<String>>, ApiError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(|text| text.trim().to_string())
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| {
                        invalid_argument(key, Some("expected array of strings".to_string()))
                    })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(invalid_argument(
            key,
            Some("expected array of strings".to_string()),
        )),
    }
}

fn route_model_test_targets_single_account(request: &RoutePoolModelTestRequest) -> bool {
    request
        .account_id
        .as_deref()
        .map(str::trim)
        .filter(|account_id| !account_id.is_empty())
        .is_some()
}

async fn route_model_test_proxy_base_url(state: &AppState) -> Result<String, ApiError> {
    let status = RouteProxyService::status(&state.route_proxy).await;
    let status = if status
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|base_url| !base_url.is_empty())
        .is_some()
    {
        status
    } else {
        RouteProxyHttpsService::start_proxy(state)
            .await
            .map_err(ApiError::from)?
    };

    status
        .base_url
        .map(|base_url| base_url.trim().to_string())
        .filter(|base_url| !base_url.is_empty())
        .ok_or_else(|| {
            ApiError::from(AppError::Validation {
                code: "validation.route_proxy_not_running",
                message: "Start the route proxy before testing the route pool".to_string(),
                details: None,
                recoverable: true,
            })
        })
}

fn optional_i64_arg(args: &Value, key: &str) -> Result<Option<i64>, ApiError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_i64()
            .map(Some)
            .ok_or_else(|| invalid_argument(key, Some("expected integer".to_string()))),
        Some(Value::String(text)) => text
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(|_| invalid_argument(key, Some("expected integer".to_string()))),
        Some(_) => Err(invalid_argument(key, Some("expected integer".to_string()))),
    }
}

fn required_string_arg(args: &Value, key: &str) -> Result<String, ApiError> {
    optional_string_arg(args, key)?.ok_or_else(|| missing_argument(key))
}

fn required_raw_string_arg(args: &Value, key: &str) -> Result<String, ApiError> {
    match args.get(key) {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Null) | None => Err(missing_argument(key)),
        Some(_) => Err(invalid_argument(key, Some("expected string".to_string()))),
    }
}

fn missing_argument(key: &str) -> ApiError {
    ApiError::from(AppError::Validation {
        code: "web.argument_missing",
        message: "A required Web command argument is missing".to_string(),
        details: Some(key.to_string()),
        recoverable: true,
    })
}

fn invalid_argument(key: &str, reason: Option<String>) -> ApiError {
    ApiError::from(AppError::Validation {
        code: "web.argument_invalid",
        message: "A Web command argument is invalid".to_string(),
        details: Some(match reason {
            Some(reason) => format!("{key}: {reason}"),
            None => key.to_string(),
        }),
        recoverable: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{create_memory_pool, run_migrations};
    use crate::services::config_write_service::ConfigWriteRuntimeState;
    use crate::services::route_proxy_service::RouteProxyRuntimeState;
    use crate::web::event_bridge::WebEventBroadcaster;
    use std::sync::Arc;
    use tempfile::tempdir;

    struct TestState {
        _temp: tempfile::TempDir,
        state: Arc<AppState>,
    }

    async fn test_state() -> TestState {
        let temp = tempdir().expect("temp dir");
        let pool = create_memory_pool().await.expect("pool");
        run_migrations(&pool).await.expect("migrations");

        TestState {
            state: Arc::new(AppState {
                paths: crate::paths::AppPaths::from_data_dir(temp.path().join("app-data")),
                pool,
                config_writes: ConfigWriteRuntimeState::default(),
                deeplink_protocols:
                    crate::services::deeplink_protocol_service::DeepLinkProtocolRuntime::default(),
                close_to_tray: crate::app_state::CloseToTrayRuntime::default(),
                route_proxy: RouteProxyRuntimeState::default(),
                event_broadcaster: Arc::new(WebEventBroadcaster::default()),
            }),
            _temp: temp,
        }
    }

    #[tokio::test]
    async fn plugin_credential_export_preserves_the_exact_selected_path() {
        let fixture = test_state().await;
        for (selected_name, unselected_name) in [
            ("selected", "selected.json"),
            ("A.txt", "A.txt.json"),
            ("A.json.json", "A.json"),
        ] {
            let selected = fixture._temp.path().join(selected_name);
            let unselected = fixture._temp.path().join(unselected_name);
            tokio::fs::write(&selected, "confirmed overwrite")
                .await
                .unwrap();
            tokio::fs::write(&unselected, "unselected file")
                .await
                .unwrap();
            let saved = dispatch_command(
                fixture.state.clone(),
                "save_route_credential_export",
                json!({"path": selected, "suggested_file_name": "ignored.json", "json_text": "[]"}),
            )
            .await
            .unwrap();
            assert_eq!(saved["path"].as_str(), selected.to_str());
            assert_eq!(saved["file_name"], selected_name);
            assert_eq!(tokio::fs::read_to_string(&selected).await.unwrap(), "[]");
            assert_eq!(
                tokio::fs::read_to_string(&unselected).await.unwrap(),
                "unselected file"
            );
        }
    }

    #[tokio::test]
    async fn plugin_diagnostics_export_preserves_the_exact_selected_path() {
        let fixture = test_state().await;
        let selected = fixture._temp.path().join("diagnostics");
        let unselected = fixture._temp.path().join("diagnostics.jsonl.br");
        tokio::fs::write(&selected, "confirmed overwrite")
            .await
            .unwrap();
        tokio::fs::write(&unselected, "unselected file")
            .await
            .unwrap();
        let saved = dispatch_command(
            fixture.state.clone(),
            "save_route_proxy_diagnostics_export",
            json!({"path": selected}),
        )
        .await
        .unwrap();
        assert_eq!(saved["path"].as_str(), selected.to_str());
        assert_eq!(saved["file_name"], "diagnostics");
        assert_eq!(
            tokio::fs::metadata(&selected).await.unwrap().len(),
            saved["byte_size"].as_u64().unwrap()
        );
        assert_eq!(
            tokio::fs::read_to_string(&unselected).await.unwrap(),
            "unselected file"
        );
    }

    #[tokio::test]
    async fn plugin_exports_write_validated_json_without_overwriting_default_exports() {
        let fixture = test_state().await;
        let args = json!({"suggested_file_name": "../credentials.json.json", "json_text": "[{\"token\":\"test-secret\"}]"});
        let first = dispatch_command(
            fixture.state.clone(),
            "save_route_credential_export",
            args.clone(),
        )
        .await
        .unwrap();
        let second = dispatch_command(fixture.state.clone(), "save_route_credential_export", args)
            .await
            .unwrap();
        assert_eq!(first["cancelled"], false);
        assert_eq!(first["file_name"], "credentials.json");
        assert_ne!(first["path"], second["path"]);
        for result in [&first, &second] {
            let path = std::path::Path::new(result["path"].as_str().unwrap());
            assert!(path.starts_with(fixture.state.paths.data_dir.join("exports")));
            assert_eq!(
                tokio::fs::read_to_string(path).await.unwrap(),
                "[{\"token\":\"test-secret\"}]"
            );
        }
        let selected = fixture._temp.path().join("selected");
        let saved = dispatch_command(
            fixture.state.clone(),
            "save_route_credential_export",
            json!({"path": selected, "json_text": "[]"}),
        )
        .await
        .unwrap();
        assert_eq!(saved["path"].as_str(), selected.to_str());
        let error = dispatch_command(
            fixture.state,
            "save_route_credential_export",
            json!({"path": selected, "json_text": "{}"}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "transfer.export_root_not_array");
        assert_eq!(tokio::fs::read_to_string(&selected).await.unwrap(), "[]");
    }

    #[tokio::test]
    async fn plugin_diagnostics_export_writes_readable_brotli_jsonl() {
        use std::io::Read;
        let fixture = test_state().await;
        let saved = dispatch_command(
            fixture.state.clone(),
            "save_route_proxy_diagnostics_export",
            json!({"path": fixture._temp.path().join("diagnostics")}),
        )
        .await
        .unwrap();
        assert_eq!(saved["cancelled"], false);
        assert_eq!(saved["file_name"], "diagnostics");
        let bytes = tokio::fs::read(saved["path"].as_str().unwrap())
            .await
            .unwrap();
        assert_eq!(saved["byte_size"], bytes.len());
        let mut decompressed = String::new();
        brotli::Decompressor::new(bytes.as_slice(), 4096)
            .read_to_string(&mut decompressed)
            .unwrap();
        let lines: Vec<Value> = decompressed
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines[0]["kind"], "meta");
        assert_eq!(lines[1]["kind"], "pool");
        assert_eq!(saved["entries"], 0);
    }

    #[tokio::test]
    async fn plugin_oauth_commands_reach_the_runtime_and_validate_arguments() {
        let fixture = test_state().await;
        let error = dispatch_command(
            fixture.state.clone(),
            "start_codex_oauth",
            json!({"input":{"method":"browser","batch_name":" "}}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "validation.batch_name_required");
        for command in ["get_codex_oauth_status", "cancel_codex_oauth"] {
            let error = dispatch_command(
                fixture.state.clone(),
                command,
                json!({"session_id":"nonexistent"}),
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, "codex_oauth.session_not_found");
        }
    }

    #[tokio::test]
    async fn plugin_model_listing_is_read_only_and_preserves_codex_metadata() {
        let fixture = test_state().await;
        let credential = dispatch_command(fixture.state.clone(), "create_api_route_credential", json!({"input": {
            "platform": "codex", "display_name": "Catalog fixture", "api_key": "sk-test",
            "base_url": "https://example.invalid/v1", "interface_format": "openai",
            "model_mappings_json": "[{\"from\":\"catalog-fixture\",\"to\":\"gpt-5.5\",\"context_window\":400000,\"reasoning_levels\":[\"xhigh\",\"max\"]}]"
        }})).await.unwrap();
        dispatch_command(
            fixture.state.clone(),
            "set_route_pool_members",
            json!({"input": {"platform":"codex", "account_ids":[credential["id"]]}}),
        )
        .await
        .unwrap();
        // The catalog assertion below expects the bare alias. Pin aggregate
        // mode rather than inheriting the fresh-pool routing default.
        dispatch_command(
            fixture.state.clone(),
            "set_route_pool_model_mode",
            json!({"input": {"platform":"codex", "mode":"aggregate"}}),
        )
        .await
        .unwrap();
        let models = dispatch_command(
            fixture.state.clone(),
            "list_route_proxy_models",
            json!({"platform":"codex"}),
        )
        .await
        .unwrap();
        let model = models
            .as_array()
            .unwrap()
            .iter()
            .find(|model| model["id"] == "catalog-fixture")
            .expect("mapped model from active pool");
        assert_eq!(model["context_window"], 400000);
        assert_eq!(model["default_reasoning_level"], "max");
        assert_eq!(model["supported_reasoning_levels"][0]["effort"], "xhigh");
        assert!(
            !crate::services::route_proxy_service::RouteProxyService::status(
                &fixture.state.route_proxy
            )
            .await
            .running
        );
        assert!(crate::database::repositories::route_proxy_key_repository::RouteProxyKeyRepository::list_all(&fixture.state.pool).await.unwrap().is_empty());
        let invalid = dispatch_command(
            fixture.state,
            "list_route_proxy_models",
            json!({"platform":"invalid-pool"}),
        )
        .await
        .unwrap_err();
        assert_ne!(invalid.code, "web.command_unknown");
    }

    #[tokio::test]
    async fn plugin_rejects_removed_management_and_terminal_commands() {
        let fixture = test_state().await;
        for command in [
            "saas_admin",
            "create_terminal_session",
            "resume_session_terminal",
            "write_terminal_input",
            "resize_terminal",
            "kill_terminal_session",
            "list_terminal_sessions",
            "open_session_terminal",
            "list_agent_launch_options",
            "get_web_service_config",
            "save_web_service_config",
            "get_web_server_status",
            "start_web_server",
            "stop_web_server",
            "set_route_access",
            "get_tailscale_status",
            "create_mobile_pairing",
            "start_tailscale_login",
            "start_tailscale_with_auth_key",
            "disconnect_tailscale",
            "list_plugins",
        ] {
            let error = dispatch_command(fixture.state.clone(), command, json!({}))
                .await
                .unwrap_err();
            assert_eq!(error.code, "web.command_unknown", "{command}");
        }
    }

    #[tokio::test]
    async fn request_body_limit_settings_round_trip_and_update_live_proxy() {
        use crate::models::platform::PlatformId;
        use crate::services::route_proxy_service::{build_proxy_state, proxy_handler};
        use axum::{
            body::Body,
            extract::State,
            http::{HeaderMap, Method, StatusCode},
        };

        let test = test_state().await;
        let proxy = build_proxy_state(test.state.pool.clone(), &test.state.route_proxy)
            .with_access_scope(PlatformId::Claude, Default::default());
        let mut settings = dispatch_command(test.state.clone(), "get_settings", json!({}))
            .await
            .unwrap();
        assert_eq!(settings["route_proxy_request_body_limit_mib"], 128);
        let mut payload =
            br#"{"model":"claude-sonnet-4","messages":[{"role":"user","content":"hello"}]}"#
                .to_vec();
        payload.resize(1024 * 1024 + 1, b' ');
        for (limit, expected) in [
            (1, StatusCode::PAYLOAD_TOO_LARGE),
            (2, StatusCode::OK),
            (0, StatusCode::OK),
        ] {
            settings["route_proxy_request_body_limit_mib"] = json!(limit);
            let saved = dispatch_command(
                test.state.clone(),
                "save_settings",
                json!({"settings": settings}),
            )
            .await
            .unwrap();
            assert_eq!(saved["route_proxy_request_body_limit_mib"], limit);
            let loaded = dispatch_command(test.state.clone(), "get_settings", json!({}))
                .await
                .unwrap();
            assert_eq!(loaded["route_proxy_request_body_limit_mib"], limit);
            // A stream without a Content-Length must obey the same live limit.
            let body = Body::from_stream(futures_util::stream::iter(
                payload
                    .chunks(4096)
                    .map(|chunk| Ok::<_, std::io::Error>(axum::body::Bytes::copy_from_slice(chunk)))
                    .collect::<Vec<_>>(),
            ));
            let response = proxy_handler(
                State(proxy.clone()),
                Method::POST,
                HeaderMap::new(),
                "/v1/messages/count_tokens".parse().unwrap(),
                body,
            )
            .await;
            assert_eq!(response.status(), expected, "limit {limit} MiB");
            if expected == StatusCode::PAYLOAD_TOO_LARGE {
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let error: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(error["error"]["code"], "route_proxy.request_body_too_large");
                assert!(error["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("1 MiB"));
            }
        }
    }

    #[test]
    fn required_raw_string_arg_preserves_skill_content_whitespace() {
        let args = json!({ "data": "\r" });

        assert_eq!(required_raw_string_arg(&args, "data").unwrap(), "\r");
    }

    #[tokio::test]
    async fn route_pool_group_commands_dispatch_dynamic_groups() {
        let test = test_state().await;

        let state = dispatch_command(
            test.state.clone(),
            "create_route_pool_group",
            json!({
                "input": {
                    "platform": "codex",
                    "name": "Web分组",
                    "is_internal": false
                }
            }),
        )
        .await
        .expect("create group command");
        let group_id = state["group_id"]
            .as_str()
            .expect("created group id")
            .to_string();
        assert_eq!(state["groups"].as_array().expect("groups").len(), 4);

        let state = dispatch_command(
            test.state.clone(),
            "get_route_pool",
            json!({
                "platform": "codex",
                "group_id": group_id,
                "since": null,
                "request_page": null,
                "request_page_size": null
            }),
        )
        .await
        .expect("get group state");
        assert_eq!(state["group_id"].as_str(), Some(group_id.as_str()));

        let state = dispatch_command(
            test.state.clone(),
            "update_route_pool_group",
            json!({
                "input": {
                    "platform": "codex",
                    "id": group_id,
                    "name": "Web分组二",
                    "is_internal": null,
                    "activate": true,
                    "sort_order": null
                }
            }),
        )
        .await
        .expect("update group command");
        assert_eq!(state["active_group_id"].as_str(), Some(group_id.as_str()));

        dispatch_command(
            test.state,
            "delete_route_pool_group",
            json!({
                "input": { "platform": "codex", "id": group_id }
            }),
        )
        .await
        .expect_err("active group delete command must fail");
    }

    #[test]
    fn required_raw_string_arg_allows_empty_skill_content() {
        let args = json!({ "data": "" });

        assert_eq!(required_raw_string_arg(&args, "data").unwrap(), "");
    }

    #[test]
    fn required_string_arg_still_rejects_blank_regular_input() {
        let args = json!({ "data": "\r" });

        let error = required_string_arg(&args, "data").unwrap_err();
        assert_eq!(error.code, "web.argument_missing");
        assert_eq!(error.details.as_deref(), Some("data"));
    }

    #[test]
    fn argument_helpers_report_stable_missing_and_invalid_codes() {
        let missing = parse_arg::<String>(&json!({}), "value").unwrap_err();
        assert_eq!(missing.code, "web.argument_missing");
        assert_eq!(missing.details.as_deref(), Some("value"));

        let invalid = optional_i64_arg(&json!({ "limit": [] }), "limit").unwrap_err();
        assert_eq!(invalid.code, "web.argument_invalid");
        assert_eq!(invalid.details.as_deref(), Some("limit: expected integer"));

        let negative = u32::try_from(-1_i64).map_err(|_| {
            invalid_argument("limit", Some("expected non-negative integer".to_string()))
        });
        let negative = negative.unwrap_err();
        assert_eq!(negative.code, "web.argument_invalid");
        assert_eq!(
            negative.details.as_deref(),
            Some("limit: expected non-negative integer")
        );
    }

    #[tokio::test]
    async fn dispatches_marketplace_and_skill_catalog_commands() {
        let fixture = test_state().await;
        let marketplaces = dispatch_command(
            Arc::clone(&fixture.state),
            "mcp_list_marketplaces",
            json!({}),
        )
        .await
        .expect("marketplaces");
        assert_eq!(marketplaces.as_array().map(Vec::len), Some(2));

        let agents = dispatch_command(fixture.state, "skills_list_agents", json!({}))
            .await
            .expect("skill agents");
        assert_eq!(agents.as_array().map(Vec::len), Some(11));
    }

    #[tokio::test]
    async fn dispatch_unknown_command_returns_structured_error() {
        let fixture = test_state().await;
        let error = dispatch_command(fixture.state, "not_a_command", json!({}))
            .await
            .unwrap_err();

        assert_eq!(error.code, "web.command_unknown");
        assert_eq!(error.details.as_deref(), Some("not_a_command"));
        assert!(!error.recoverable);
    }

    #[tokio::test]
    async fn saving_a_masked_secret_payload_keeps_the_stored_key() {
        // The paired phone is shown MASKED_SECRET_PAYLOAD instead of the real
        // secret. It loads the same edit drawer, so saving from there must not
        // write the mask over the key — masking a read has to be safe on its own.
        let fixture = test_state().await;
        let credential = dispatch_command(
            Arc::clone(&fixture.state),
            "create_api_route_credential",
            json!({
                "input": {
                    "platform": "codex",
                    "display_name": "Phone edited",
                    "api_key": "sk-real-key",
                    "base_url": "https://api.example.com/v1",
                    "interface_format": "openai",
                    "model_mappings_json": "[]"
                }
            }),
        )
        .await
        .expect("create credential");
        let id = credential["id"]
            .as_str()
            .expect("credential id")
            .to_string();
        let config_json = credential["config_json"]
            .as_str()
            .expect("config json")
            .to_string();

        let updated = dispatch_command(
            fixture.state,
            "update_route_credential",
            json!({
                "id": id,
                "input": {
                    "display_name": "Renamed from the phone",
                    "email": null,
                    "status": "ok",
                    "route_priority": 1,
                    "max_concurrency": 1,
                    "secret_payload_json": MASKED_SECRET_PAYLOAD,
                    "config_json": config_json,
                    "preview_json": "{}"
                }
            }),
        )
        .await
        .expect("update credential");

        assert_eq!(updated["display_name"], "Renamed from the phone");
        let secret = updated["secret_payload_json"]
            .as_str()
            .expect("secret payload");
        assert!(
            secret.contains("sk-real-key"),
            "the mask must not overwrite the stored key, got {secret}"
        );
    }

    #[tokio::test]
    async fn saving_a_masked_payload_merged_with_an_empty_key_keeps_the_stored_key() {
        // What the edit drawer actually sends back: it re-serializes the payload it
        // loaded and merges its (empty) api_key field in, so the mask arrives as
        // `{"__masked":true,"api_key":""}`. A whole-string comparison would miss
        // this and blank out the key.
        let fixture = test_state().await;
        let credential = dispatch_command(
            Arc::clone(&fixture.state),
            "create_api_route_credential",
            json!({
                "input": {
                    "platform": "codex",
                    "display_name": "Phone edited",
                    "api_key": "sk-real-key",
                    "base_url": "https://api.example.com/v1",
                    "interface_format": "openai",
                    "model_mappings_json": "[]"
                }
            }),
        )
        .await
        .expect("create credential");
        let id = credential["id"]
            .as_str()
            .expect("credential id")
            .to_string();
        let config_json = credential["config_json"]
            .as_str()
            .expect("config json")
            .to_string();

        let updated = dispatch_command(
            fixture.state,
            "update_route_credential",
            json!({
                "id": id,
                "input": {
                    "display_name": "Phone edited",
                    "email": null,
                    "status": "ok",
                    "route_priority": 1,
                    "max_concurrency": 1,
                    "secret_payload_json": r#"{"__masked": true, "api_key": ""}"#,
                    "config_json": config_json,
                    "preview_json": "{}"
                }
            }),
        )
        .await
        .expect("update credential");

        let secret = updated["secret_payload_json"]
            .as_str()
            .expect("secret payload");
        assert!(
            secret.contains("sk-real-key"),
            "the drawer's re-serialized mask must not overwrite the stored key, got {secret}"
        );
    }

    #[tokio::test]
    async fn dispatches_route_credential_recovery_rule() {
        let fixture = test_state().await;
        let credential = dispatch_command(
            Arc::clone(&fixture.state),
            "create_api_route_credential",
            json!({
                "input": {
                    "platform": "codex",
                    "display_name": "Scheduled account",
                    "api_key": "sk-test",
                    "base_url": "https://api.example.com/v1",
                    "interface_format": "openai",
                    "model_mappings_json": "[]"
                }
            }),
        )
        .await
        .expect("create credential");
        let id = credential["id"].as_str().expect("credential id");

        let updated = dispatch_command(
            fixture.state,
            "set_route_credential_recovery",
            json!({
                "id": id,
                "rule": {
                    "mode": "scheduled",
                    "times": ["3:00", "15:00"]
                }
            }),
        )
        .await
        .expect("set recovery rule");
        let config: Value =
            serde_json::from_str(updated["config_json"].as_str().expect("config json"))
                .expect("config value");

        assert_eq!(config["recovery"]["mode"], "scheduled");
        assert_eq!(config["recovery"]["times"], json!(["03:00", "15:00"]));
    }

    #[tokio::test]
    async fn dispatches_route_credential_copy_options_and_keeps_legacy_calls_working() {
        let fixture = test_state().await;
        let credential = dispatch_command(
            Arc::clone(&fixture.state),
            "create_api_route_credential",
            json!({
                "input": {
                    "platform": "claude",
                    "display_name": "Claude API",
                    "api_key": "sk-source",
                    "base_url": "https://api.example.com",
                    "interface_format": "anthropic",
                    "model_mappings_json": "[]"
                }
            }),
        )
        .await
        .expect("create credential");
        let id = credential["id"].as_str().expect("credential id");

        let copied = dispatch_command(
            Arc::clone(&fixture.state),
            "copy_route_credential",
            json!({
                "id": id,
                "input": {
                    "target_platform": "codex",
                    "api_key": "sk-override"
                }
            }),
        )
        .await
        .expect("copy credential");
        let copied_config: Value =
            serde_json::from_str(copied["config_json"].as_str().expect("config json"))
                .expect("config value");
        let copied_secret: Value = serde_json::from_str(
            copied["secret_payload_json"]
                .as_str()
                .expect("secret payload json"),
        )
        .expect("secret value");

        assert_eq!(copied["platform"], "codex");
        assert_eq!(copied_config["base_url"], "https://api.example.com/v1");
        assert_eq!(copied_secret["api_key"], "sk-override");

        let legacy_copy =
            dispatch_command(fixture.state, "copy_route_credential", json!({ "id": id }))
                .await
                .expect("legacy copy");
        // 统一账号池（G-P8 P4a）：legacy copy 无 target_platform 时默认目标协议是
        // codex，而源账号是 anthropic —— 跨协议转换照常发生（base_url 加 /v1）。
        assert_eq!(legacy_copy["platform"], "codex");
        assert_eq!(
            legacy_copy["config_json"].as_str().map(|raw| {
                serde_json::from_str::<Value>(raw).expect("config value")["base_url"].clone()
            }),
            Some(json!("https://api.example.com/v1"))
        );
    }

    #[tokio::test]
    async fn dispatches_export_with_the_exact_input_argument() {
        let fixture = test_state().await;
        let result = dispatch_command(
            Arc::clone(&fixture.state),
            "export_route_credentials",
            json!({
                "input": {
                    "selection_context": {"platform": "claude", "pool_scope": "in_pool"},
                    "credential_ids": []
                }
            }),
        )
        .await
        .unwrap();

        assert!(result["json_text"].is_null());
        assert_eq!(result["errors"][0]["code"], "transfer.selection_empty");

        let error = dispatch_command(
            fixture.state,
            "export_route_credentials",
            json!({"Input": {}}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "web.argument_missing");
        assert_eq!(error.details.as_deref(), Some("input"));
    }

    #[tokio::test]
    async fn dispatches_import_preview_and_commit_with_the_exact_input_argument() {
        let fixture = test_state().await;
        let input = json!({
            "text": "[]",
            "ambiguous_platform_choices": [],
            "restore_pool_membership": false
        });

        let preview = dispatch_command(
            Arc::clone(&fixture.state),
            "preview_route_credential_import",
            json!({
                "input": {
                    "text": "[]",
                    "ambiguous_platform_choices": []
                }
            }),
        )
        .await
        .expect("import preview");
        assert_eq!(preview["counts"]["total"], 0);

        let outcome = dispatch_command(
            fixture.state,
            "import_route_credentials",
            json!({ "input": input }),
        )
        .await
        .expect("import commit");
        assert_eq!(outcome["imported"], 0);
        assert_eq!(outcome["failed"], 0);
    }

    #[tokio::test]
    async fn dispatch_get_route_proxy_https_status_returns_a_serializable_status() {
        let fixture = test_state().await;
        let result = dispatch_command(fixture.state, "get_route_proxy_https_status", json!({}))
            .await
            .expect("HTTPS status response");

        assert_eq!(result.get("enabled").and_then(Value::as_bool), Some(false));
        assert_eq!(
            result.get("certReady").and_then(Value::as_bool),
            Some(false)
        );
        assert!(result
            .get("certificateDir")
            .and_then(Value::as_str)
            .is_some());
        assert!(result
            .get("manualInstructions")
            .and_then(Value::as_array)
            .is_some());
    }

    #[tokio::test]
    async fn dispatch_get_route_proxy_status_reports_both_endpoints_in_snake_case() {
        let fixture = test_state().await;
        let material =
            crate::services::route_proxy_https_service::RouteProxyHttpsService::ensure_material(
                &fixture.state.paths,
            )
            .await
            .expect("certificate material");
        RouteProxyService::start(
            &fixture.state.route_proxy,
            fixture.state.pool.clone(),
            crate::services::route_proxy_service::RouteProxyTransport::HttpAndHttps {
                certificate_pem_path: material.server_certificate_pem,
                private_key_pem_path: material.server_private_key_pem,
            },
        )
        .await
        .expect("start proxy");

        let result = dispatch_command(fixture.state.clone(), "get_route_proxy_status", json!({}))
            .await
            .expect("route proxy status response");

        // `RouteProxyStatus` has no `rename_all`, unlike its HTTPS sibling above, so
        // the new fields stay snake_case. The frontend types are written against
        // exactly these names.
        assert!(result
            .get("base_url")
            .and_then(Value::as_str)
            .is_some_and(|url| url.starts_with("http://")));
        assert!(result
            .get("https_base_url")
            .and_then(Value::as_str)
            .is_some_and(|url| url.starts_with("https://")));
        assert!(result.get("https_port").and_then(Value::as_u64).is_some());
        assert!(result.get("https_error").is_some_and(Value::is_null));

        RouteProxyService::stop(&fixture.state.route_proxy)
            .await
            .expect("stop proxy");
    }

    #[tokio::test]
    async fn dispatch_get_disk_space_status_reports_the_shipped_threshold() {
        let fixture = test_state().await;
        let result = dispatch_command(fixture.state, "get_disk_space_status", json!({}))
            .await
            .expect("disk space response");

        assert_eq!(
            result.get("threshold_bytes").and_then(Value::as_u64),
            Some(crate::models::disk_space::LOW_DISK_SPACE_THRESHOLD_BYTES)
        );
        let volumes = result.get("volumes").and_then(Value::as_array).cloned();
        let volumes = volumes.expect("volume rows");
        assert!(!volumes.is_empty(), "{result}");
        for volume in &volumes {
            assert!(
                volume
                    .get("total_bytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > 0,
                "{volume}"
            );
            assert!(
                volume.get("label").and_then(Value::as_str).is_some(),
                "{volume}"
            );
        }
    }

    #[tokio::test]
    async fn dispatch_list_platform_capabilities_returns_phase_a_matrix() {
        let fixture = test_state().await;
        let result = dispatch_command(fixture.state, "list_platform_capabilities", json!({}))
            .await
            .expect("platform capability response");

        let rows = result.as_array().expect("capability rows");
        assert_eq!(rows.len(), 7);
        let hermes = rows
            .iter()
            .find(|row| row.get("platform").and_then(Value::as_str) == Some("hermes"))
            .expect("Hermes capability");
        assert_eq!(
            hermes.get("support_level").and_then(Value::as_str),
            Some("partial")
        );
        assert_eq!(
            hermes
                .pointer("/operations/config_write/availability")
                .and_then(Value::as_str),
            Some("supported")
        );
        // What "partial" now means: no official vendor account to import, route
        // through, or read quota from.
        assert_eq!(
            hermes
                .pointer("/operations/official_import/availability")
                .and_then(Value::as_str),
            Some("unavailable")
        );
    }

    #[tokio::test]
    async fn dispatches_target_config_statuses_and_snapshot_summaries() {
        let fixture = test_state().await;
        let statuses = dispatch_command(
            Arc::clone(&fixture.state),
            "list_target_config_statuses",
            json!({}),
        )
        .await
        .expect("target statuses");
        assert!(statuses.as_array().is_some_and(|rows| rows
            .iter()
            .any(|row| row.pointer("/target/key") == Some(&json!("codex")))));

        let snapshots = dispatch_command(
            fixture.state,
            "list_config_snapshots",
            json!({ "limit": 5 }),
        )
        .await
        .expect("snapshot summaries");
        assert_eq!(snapshots, json!([]));
    }
}
