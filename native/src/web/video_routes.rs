//! 视频渠道的 HTTP 入口（G-P4）
//! =================================================================
//! 把**两条**契约端点接到 `VideoGenerationService`，指向同一套任务：
//!
//! ```text
//! ① new-api 对齐
//! POST /v1/video/generations            提交 → 201 { id, object, model, created_at, task_id, status }
//! GET  /v1/video/generations/{task_id}  查询 → 200 { task_id, status, url, format, metadata, error }
//!
//! ② OpenAI 原生（`/v1/videos`）
//! POST /v1/videos                       提交 → 201 { id, object, model, created_at, status, progress, seconds, size }
//! GET  /v1/videos/{video_id}            查询 → 200 { id, object, model, created_at, status, progress, seconds, size, url, error }
//! GET  /v1/videos/{video_id}/content    内容 → 200 视频字节流（video/mp4）
//! ```
//!
//! 两套契约共享本地账号池、任务存储与后台轮询。所有公开入口都要求本地
//! API key（Bearer、API key 请求头或 key 查询参数）。平台只由 key 决定；
//! x-ai-switch-platform 与请求体 platform 字段不能授权或覆盖 key 的作用域。
//! 查询与下载还校验任务属于该 key 的账号池，不接受历史外部租户任务。

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::time::Duration;

use crate::app_state::AppState;
use crate::database::repositories::route_proxy_key_repository::RouteProxyKeyRepository;
use crate::error::AppError;
use crate::videogen::models::VideoTask;
use crate::videogen::protocol::{
    effective_duration_seconds, effective_size, openai_video_create_response,
    openai_video_retrieve_response, video_request_from_openai, VideoGenerationRequest,
    VideoTaskStatus, VIDEO_TASK_ID_PREFIX,
};
use crate::videogen::repository::VideoGenerationRepository;
use crate::videogen::service::{SubmitVideoInput, VideoGenerationService};

/// 平台可能放在请求体里的字段名（curl 友好；请求头与入站 key 都优先）。
const PLATFORM_BODY_FIELD: &str = "platform";
/// new-api 契约里 `metadata` 的字段名。
const METADATA_FIELD: &str = "metadata";
/// 请求体上限。视频请求体只是 JSON（首帧图是 URL，不是字节），4 MiB 绰绰有余。
const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
/// 拉取上游视频字节流的超时。出片动辄几十上百 MB，比轮询的 10s 宽松得多，
/// 但仍然要有上限——不然一个挂死的 CDN 会一直占着连接。
const CONTENT_FETCH_TIMEOUT: Duration = Duration::from_secs(120);

/// `/v1/video/generations` 是否为「提交」路径（与状态查询区分）。
pub(crate) fn is_video_submit_path(path: &str) -> bool {
    matches!(
        path.trim().trim_end_matches('/'),
        "/v1/video/generations" | "/video/generations"
    )
}

/// 从 `/v1/video/generations/{task_id}` 里取出 task_id。
pub(crate) fn video_task_id_from_path(path: &str) -> Option<String> {
    let normalized = path.trim().trim_end_matches('/');
    let rest = normalized
        .strip_prefix("/v1/video/generations/")
        .or_else(|| normalized.strip_prefix("/video/generations/"))?;
    let task_id = rest.trim();
    (!task_id.is_empty() && !task_id.contains('/')).then(|| task_id.to_string())
}

/// OpenAI 原生契约的提交路径（`POST /v1/videos`）。
///
/// 与 `/v1/video/generations` 是**两条独立的路径**，不会互相匹配：
/// 前者要求路径正好是 `/v1/videos`，后者要求 `/v1/video/generations`。
///
/// **只有 `/v1` 前缀这一种形态**，不做 `/videos` 别名：OpenAI 的路径永远是
/// `/v1/videos`，而加那个别名会让 `/videos/generations` 这类无关路径被
/// `route_proxy_service::is_shared_model_api_path` 顺手放进来。
pub(crate) fn is_openai_video_submit_path(path: &str) -> bool {
    path.trim().trim_end_matches('/') == "/v1/videos"
}

/// `/v1/videos/{video_id}` → `video_id`（**不含** `/content` 那条）。
pub(crate) fn openai_video_id_from_path(path: &str) -> Option<String> {
    openai_video_segment_from_path(path, false)
}

/// `/v1/videos/{video_id}/content` → `video_id`。
pub(crate) fn openai_video_content_id_from_path(path: &str) -> Option<String> {
    openai_video_segment_from_path(path, true)
}

/// 两个 OpenAI 视频路径共用一份解析：摘出 id，并校验剩下的分段正好符合预期。
///
/// `content = false` 时要求 `{id}` 之后没有分段；`true` 时要求正好一个 `content`。
/// 多一个分段就不认（否则 `/v1/videos/a/b/c` 会被当成 id 为 `a` 的合法请求）。
fn openai_video_segment_from_path(path: &str, content: bool) -> Option<String> {
    let normalized = path.trim().trim_end_matches('/');
    let rest = normalized.strip_prefix("/v1/videos/")?;
    let mut segments = rest.split('/');
    let video_id = segments.next()?.trim();
    if video_id.is_empty() {
        return None;
    }
    match (content, segments.next()) {
        (true, Some("content")) | (false, None) => {}
        _ => return None,
    }
    // 再多就不认了
    segments.next().is_none().then(|| video_id.to_string())
}

/// 加载任务，并容忍 new-api 契约 `id` 的 `video-` 前缀。
///
/// new-api 的 201 体里 `id` 是 `video-{task_id}`，客户端可能直接拿那个 `id`
/// 去查 OpenAI 的 `GET /v1/videos/{id}`（两条契约指向同一套任务）。摘掉前缀再查，
/// 比回一个「任务不存在」让用户困惑要好。task_id 是 UUID，不会以 `video-` 开头，
/// 所以摘前缀不会误伤裸 id。
pub(crate) async fn load_video_task(
    state: &AppState,
    video_id: &str,
) -> Result<VideoTask, AppError> {
    let trimmed = video_id.trim();
    let task_id = trimmed
        .strip_prefix(VIDEO_TASK_ID_PREFIX)
        .filter(|rest| !rest.is_empty())
        .unwrap_or(trimmed);
    VideoGenerationRepository::get_task(&state.pool, task_id).await
}

/// 本地池 key 只能读取同池账号提交的本地任务。
pub(crate) async fn authorize_video_task_read(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<(), Response> {
    let platform = resolve_video_platform(&state.pool, headers, uri.query(), b"{}").await?;
    let Some(task_id) = video_task_id_from_path(uri.path())
        .or_else(|| openai_video_content_id_from_path(uri.path()))
        .or_else(|| openai_video_id_from_path(uri.path()))
    else {
        return Ok(());
    };
    let task = load_video_task(state, &task_id)
        .await
        .map_err(app_error_response)?;
    let task_platform: Option<String> = match task.credential_id.as_deref() {
        Some(id) => sqlx::query_scalar("SELECT platform FROM route_credentials WHERE id=?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| {
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "task_scope",
                    "Could not verify video task scope",
                )
            })?,
        None => None,
    };
    if task.owner_user_id.is_some() || task_platform.as_deref() != Some(platform.as_str()) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "task_scope",
            "This pool key cannot access the video task",
        ));
    }
    Ok(())
}

/// OpenAI 原生契约的任务对象体（`GET /v1/videos/{video_id}` 的响应）。
///
/// 同步、无副作用，所以能直接拿任务行单测。
pub(crate) fn openai_video_object(task: &VideoTask) -> Vec<u8> {
    // 任务表里的 status 是字符串；解析不出来（脏数据）就按「进行中」处理——
    // 让客户端继续轮询，而不是误报完成或失败。
    let status = VideoTaskStatus::parse(&task.status).unwrap_or(VideoTaskStatus::Processing);
    openai_video_retrieve_response(
        &task.task_id,
        &task.model,
        &task.created_at,
        status,
        task.url.as_deref(),
        task.error_message.as_deref(),
        task.duration_seconds,
        task.size.as_deref(),
    )
}

/// 拉取上游视频字节流（`GET /v1/videos/{video_id}/content`）。
///
/// **自己去上游取字节再转发，而不是 302 重定向**：OpenAI 的契约是「返回视频
/// 文件流」，而不少 SDK 的 HTTP 客户端默认不跟随重定向（Python `httpx` 就是
/// `follow_redirects=False`），一个 302 会让它们拿到空体。
///
/// 目标地址来自**我们自己刚存下的上游响应**（签名 CDN 链接），不是客户端给的，
/// 所以没有 SSRF 面；仍然加超时兜住挂死的上游。
///
/// 流式转发而不是先缓冲：出片动辄几十上百 MB，缓冲会把并发上限变成内存上限。
pub(crate) async fn openai_video_content(task: &VideoTask) -> Result<(String, Body), AppError> {
    let Some(url) = task
        .url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    else {
        return Err(validation(
            "videogen.asset_not_found",
            "The video is not ready: no output URL has been recorded for this task yet",
        ));
    };
    let client = crate::services::http_client::build_outbound_http_client(Some(CONTENT_FETCH_TIMEOUT))
        .map_err(|error| validation("videogen.poll_failed", &error))?;
    let response = client.get(url).send().await.map_err(|error| {
        validation(
            "videogen.poll_failed",
            &format!("Could not fetch the video content: {error}"),
        )
    })?;
    if !response.status().is_success() {
        return Err(validation(
            "videogen.poll_failed",
            &format!(
                "The video host answered HTTP {} for the stored output URL",
                response.status().as_u16()
            ),
        ));
    }
    // 上游给的 content-type 才准；它没给、或给的不是视频类时按契约回 mp4。
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| value.starts_with("video/") || *value == "application/octet-stream")
        .unwrap_or("video/mp4")
        .to_string();
    Ok((content_type, Body::from_stream(response.bytes_stream())))
}

/// 提交：转成上游方言体 → 落任务 → 回 201 契约体。
pub(crate) async fn submit_video_generation(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
    body: axum::body::Bytes,
) -> Response {
    let body = match normalize_request_body(&body) {
        Ok(body) => body,
        Err(response) => return response,
    };
    let platform = match resolve_video_platform(&state.pool, headers, uri.query(), &body).await {
        Ok(platform) => platform,
        Err(response) => return response,
    };
    let request = match VideoGenerationRequest::parse(&body) {
        Ok(request) => request,
        Err(error) => {
            return error_response(StatusCode::BAD_REQUEST, "invalid_request", &error);
        }
    };
    match VideoGenerationService::submit(
        state,
        SubmitVideoInput {
            platform,
            request,
            session_id: None,
            message_id: None,
        },
    )
    .await
    {
        Ok(outcome) => (
            StatusCode::CREATED,
            [("content-type", "application/json")],
            outcome.body,
        )
            .into_response(),
        Err(error) => app_error_response(error),
    }
}

/// OpenAI 原生契约的提交（`POST /v1/videos`）。
///
/// 与 `submit_video_generation` 的差别只有两处：请求体按 OpenAI 的形状解析
/// （`seconds` / `size` / `input_reference`），响应体按 OpenAI 的形状拼
/// （`id` 是裸 task_id、状态用 `queued`、带 `seconds`/`size`/`progress`）。
/// 平台鉴权、账号池、方言与落库全部复用同一条路径。
async fn submit_openai_video(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
    body: axum::body::Bytes,
) -> Response {
    let body = match normalize_request_body(&body) {
        Ok(body) => body,
        Err(response) => return response,
    };
    let platform = match resolve_video_platform(&state.pool, headers, uri.query(), &body).await {
        Ok(platform) => platform,
        Err(response) => return response,
    };
    let request = match video_request_from_openai(&body) {
        Ok(request) => request,
        Err(error) => {
            return error_response(StatusCode::BAD_REQUEST, "invalid_request", &error);
        }
    };
    // 响应体要回显 `seconds`/`size`，与落库、与上游体**共用同一份计算**
    // （`effective_duration_seconds` / `effective_size`），否则三个地方会各算一套。
    let duration_seconds = effective_duration_seconds(&request);
    let size = effective_size(&request);
    let model = request.model.trim().to_string();

    match VideoGenerationService::submit(
        state,
        SubmitVideoInput {
            platform,
            request,
            session_id: None,
            message_id: None,
        },
    )
    .await
    {
        Ok(outcome) => (
            StatusCode::CREATED,
            [("content-type", "application/json")],
            openai_video_create_response(
                &outcome.task_id,
                &model,
                duration_seconds,
                size.as_deref(),
            ),
        )
            .into_response(),
        Err(error) => app_error_response(error),
    }
}

/// OpenAI 原生契约的查询（`GET /v1/videos/{video_id}`）。
async fn openai_video_retrieve(state: &AppState, video_id: &str) -> Response {
    match load_video_task(state, video_id).await {
        Ok(task) => (
            StatusCode::OK,
            [("content-type", "application/json")],
            openai_video_object(&task),
        )
            .into_response(),
        Err(error) => app_error_response(error),
    }
}

/// OpenAI 原生契约的内容（`GET /v1/videos/{video_id}/content`）。
async fn openai_video_content_response(state: &AppState, video_id: &str) -> Response {
    let task = match load_video_task(state, video_id).await {
        Ok(task) => task,
        Err(error) => return app_error_response(error),
    };
    match openai_video_content(&task).await {
        Ok((content_type, body)) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", content_type)
            .body(body)
            .unwrap_or_else(|_| {
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "video_error",
                    "Could not build the video content response",
                )
            }),
        Err(error) => app_error_response(error),
    }
}

/// 查询：`refresh=1` 时先向上游拉一次，否则读本地状态。
pub(crate) async fn video_generation_status(
    state: &AppState,
    uri: &Uri,
    task_id: &str,
) -> Response {
    let refresh = uri.query().is_some_and(|query| {
        query
            .split('&')
            .any(|pair| pair == "refresh=1" || pair == "refresh=true")
    });
    match VideoGenerationService::status(state, task_id, refresh).await {
        Ok(body) => (StatusCode::OK, [("content-type", "application/json")], body).into_response(),
        Err(error) => app_error_response(error),
    }
}

/// Only a managed local API key selects the pool. A platform hint is never an
/// authentication credential, even when a caller reaches this module directly.
async fn resolve_video_platform(
    pool: &SqlitePool,
    headers: &HeaderMap,
    query: Option<&str>,
    _body: &[u8],
) -> Result<String, Response> {
    let unauthorized = || {
        error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_key",
            "A valid local pool API key is required",
        )
    };
    let key = crate::services::route_proxy_service::extract_inbound_api_key(headers, query)
        .ok_or_else(unauthorized)?;
    let platform = RouteProxyKeyRepository::get_platform_by_key(pool, &key)
        .await
        .map_err(app_error_response)?
        .ok_or_else(unauthorized)?;
    crate::models::platform::PlatformId::parse(&platform).map_err(app_error_response)?;
    Ok(platform)
}

/// 把 `platform` 字段从请求体里摘掉再交给契约解析。
///
/// `VideoGenerationRequest` 是 `#[derive(Deserialize)]` 且没有
/// `deny_unknown_fields`，多一个字段本不会报错；这里仍然显式移除，是为了让
/// 契约体保持「只有 new-api 字段」，不再往下游传递本地扩展。
fn normalize_request_body(body: &[u8]) -> Result<axum::body::Bytes, Response> {
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Request body must be a JSON object",
        ));
    };
    let Some(object) = value.as_object_mut() else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Request body must be a JSON object",
        ));
    };
    object.remove(PLATFORM_BODY_FIELD);
    // `metadata` 已存在但不是对象时（例如被写成字符串）直接拒绝：
    // 各家私有参数都从它取，一个非对象值会让方言转换静默丢参数。
    if object
        .get(METADATA_FIELD)
        .is_some_and(|metadata| !metadata.is_object() && !metadata.is_null())
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "metadata must be a JSON object",
        ));
    }
    serde_json::to_vec(&value)
        .map(axum::body::Bytes::from)
        .map_err(|_| {
            error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Request body could not be normalized",
            )
        })
}

fn app_error_response(error: crate::error::AppError) -> Response {
    use crate::error::AppError;
    let status = match &error {
        AppError::Validation { code, .. } => match *code {
            "validation.video_platform" | "validation.video_request" => StatusCode::BAD_REQUEST,
            "videogen.task_not_found"
            | "videogen.session_not_found"
            | "videogen.message_not_found"
            | "videogen.asset_not_found" => StatusCode::NOT_FOUND,
            "videogen.submit_failed" | "videogen.poll_failed" => StatusCode::BAD_GATEWAY,
            _ => StatusCode::BAD_REQUEST,
        },
        AppError::Database { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    };
    let body = json!({
        "error": {
            "code": error.code(),
            "message": error.to_string(),
            "type": "video_error",
        }
    });
    (status, axum::Json(body)).into_response()
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        axum::Json(json!({
            "error": {
                "code": code,
                "message": message,
                "type": "video_error",
            }
        })),
    )
        .into_response()
}

/// AI-only router 入口：鉴权后按路径分派到提交 / 查询 / 内容。
///
/// 两条契约在这里汇合：`/v1/video/generations`（new-api）与 `/v1/videos`
/// （OpenAI 原生）。分派顺序是「先精确路径、再带 id 的路径」，且 `content`
/// 必须排在裸 `{id}` 之前——虽然两个解析函数互斥（裸 id 那条会拒绝多余分段），
/// 但顺序写清楚能避免以后有人放宽 `{id}` 的匹配时把 `/content` 吃掉。
pub(crate) async fn handle_video_request(
    state: &AppState,
    method: axum::http::Method,
    headers: &HeaderMap,
    uri: &Uri,
    body: Body,
) -> Response {
    use axum::http::Method;

    let path = uri.path();
    let recognized = is_video_submit_path(path)
        || is_openai_video_submit_path(path)
        || video_task_id_from_path(path).is_some()
        || openai_video_id_from_path(path).is_some()
        || openai_video_content_id_from_path(path).is_some();
    if !recognized {
        return error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Unknown video generation endpoint",
        );
    }
    if let Err(response) = resolve_video_platform(&state.pool, headers, uri.query(), b"{}").await {
        return response;
    }
    if method == Method::GET {
        if let Err(response) = authorize_video_task_read(state, headers, uri).await {
            return response;
        }
    }

    // ── 提交 ──
    if is_video_submit_path(path) {
        if method != Method::POST {
            return error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Use POST to submit a video generation",
            );
        }
        let body = match read_request_body(body).await {
            Ok(body) => body,
            Err(response) => return response,
        };
        return submit_video_generation(state, headers, uri, body).await;
    }
    if is_openai_video_submit_path(path) {
        if method != Method::POST {
            return error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Use POST to submit a video generation",
            );
        }
        let body = match read_request_body(body).await {
            Ok(body) => body,
            Err(response) => return response,
        };
        return submit_openai_video(state, headers, uri, body).await;
    }

    // ── 查询 / 内容 ──
    if let Some(task_id) = video_task_id_from_path(path) {
        if method != Method::GET {
            return error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Use GET to query a video generation task",
            );
        }
        return video_generation_status(state, uri, &task_id).await;
    }
    if let Some(video_id) = openai_video_content_id_from_path(path) {
        if method != Method::GET {
            return error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Use GET to download a video",
            );
        }
        return openai_video_content_response(state, &video_id).await;
    }
    if let Some(video_id) = openai_video_id_from_path(path) {
        if method != Method::GET {
            return error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Use GET to query a video generation task",
            );
        }
        return openai_video_retrieve(state, &video_id).await;
    }

    error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "Unknown video generation endpoint",
    )
}

/// 读请求体并施加大小上限（两条契约共用）。
async fn read_request_body(body: Body) -> Result<axum::body::Bytes, Response> {
    axum::body::to_bytes(body, MAX_REQUEST_BYTES)
        .await
        .map_err(|_| {
            error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_limits",
                "Video request body exceeds the limit",
            )
        })
}

/// 构造一个可恢复的校验错误（本模块只用来表达「拿不到上游产物」这类失败）。
fn validation(code: &'static str, message: &str) -> AppError {
    AppError::Validation {
        code,
        message: message.to_string(),
        details: None,
        recoverable: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn video_entrypoint_rejects_platform_header_and_body_without_a_valid_key() {
        use axum::http::Method;
        let state = test_state().await;
        for key in [None, Some("Bearer invalid-key")] {
            let mut headers = HeaderMap::new();
            headers.insert("x-ai-switch-platform", "codex".parse().unwrap());
            if let Some(key) = key {
                headers.insert("authorization", key.parse().unwrap());
            }
            for (method, path) in [
                (Method::POST, "/v1/video/generations"),
                (Method::POST, "/v1/videos"),
                (Method::GET, "/v1/video/generations/missing"),
                (Method::GET, "/v1/videos/missing"),
                (Method::GET, "/v1/videos/missing/content"),
            ] {
                let response = handle_video_request(
                    &state,
                    method,
                    &headers,
                    &path.parse().unwrap(),
                    Body::from(r#"{"platform":"codex","model":"sora-2","prompt":"test"}"#),
                )
                .await;
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
            }
        }
    }

    #[tokio::test]
    async fn valid_video_key_cannot_be_overridden_by_a_platform_header() {
        let state = test_state().await;
        RouteProxyKeyRepository::ensure_platform_key(&state.pool, "codex", "sk-codex")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer sk-codex".parse().unwrap());
        headers.insert("x-ai-switch-platform", "claude".parse().unwrap());
        assert_eq!(
            resolve_video_platform(&state.pool, &headers, None, br#"{"platform":"claude"}"#)
                .await
                .unwrap(),
            "codex"
        );
        let from_query =
            resolve_video_platform(&state.pool, &HeaderMap::new(), Some("key=sk-codex"), b"{}")
                .await
                .unwrap();
        assert_eq!(from_query, "codex");
    }

    #[tokio::test]
    async fn video_task_reads_reject_other_pools_and_historical_external_owners() {
        use crate::videogen::models::NewVideoTask;
        let state = test_state().await;
        sqlx::query("INSERT INTO route_credentials(id,platform,kind,display_name,status,secret_payload_json,config_json,created_at,updated_at) VALUES('task-account','codex','api','video','ok','{}','{}','2026-01-01','2026-01-01')").execute(&state.pool).await.unwrap();
        VideoGenerationRepository::create_task(
            &state.pool,
            NewVideoTask {
                task_id: "scope-task".into(),
                session_id: None,
                message_id: None,
                model: "m".into(),
                upstream_model: None,
                dialect: "kling".into(),
                credential_id: Some("task-account".into()),
                upstream_task_id: Some("up".into()),
                duration_seconds: Some(5),
                size: None,
                callback_url: None,
                owner_user_id: None,
            },
        )
        .await
        .unwrap();
        RouteProxyKeyRepository::ensure_platform_key(&state.pool, "codex", "sk-codex")
            .await
            .unwrap();
        RouteProxyKeyRepository::ensure_platform_key(&state.pool, "claude", "sk-claude")
            .await
            .unwrap();
        let uri: Uri = "/v1/videos/scope-task".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer sk-claude".parse().unwrap());
        assert_eq!(
            authorize_video_task_read(&state, &headers, &uri)
                .await
                .unwrap_err()
                .status(),
            StatusCode::FORBIDDEN
        );
        headers.insert("authorization", "Bearer sk-codex".parse().unwrap());
        assert!(authorize_video_task_read(&state, &headers, &uri)
            .await
            .is_ok());
        sqlx::query("UPDATE videogen_tasks SET owner_user_id='tenant' WHERE task_id='scope-task'")
            .execute(&state.pool)
            .await
            .unwrap();
        assert_eq!(
            authorize_video_task_read(&state, &headers, &uri)
                .await
                .unwrap_err()
                .status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn authenticated_video_queries_and_download_stream_keep_both_contracts() {
        use crate::videogen::models::NewVideoTask;
        use axum::{http::Method, routing::get, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/clip",
                    get(|| async { ([("content-type", "video/webm")], "test-video-bytes") }),
                ),
            )
            .await
            .unwrap();
        });
        let state = test_state().await;
        sqlx::query("INSERT INTO route_credentials(id,platform,kind,display_name,status,secret_payload_json,config_json,created_at,updated_at) VALUES('read-account','codex','api','video','ok','{}','{}','2026-01-01','2026-01-01')").execute(&state.pool).await.unwrap();
        VideoGenerationRepository::create_task(
            &state.pool,
            NewVideoTask {
                task_id: "download-task".into(),
                session_id: None,
                message_id: None,
                model: "sora-2".into(),
                upstream_model: None,
                dialect: "sora".into(),
                credential_id: Some("read-account".into()),
                upstream_task_id: Some("upstream-id".into()),
                duration_seconds: Some(8),
                size: Some("1280x720".into()),
                callback_url: None,
                owner_user_id: None,
            },
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE videogen_tasks SET status='succeeded', url=? WHERE task_id='download-task'",
        )
        .bind(format!("http://{addr}/clip"))
        .execute(&state.pool)
        .await
        .unwrap();
        RouteProxyKeyRepository::ensure_platform_key(&state.pool, "codex", "sk-download")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "sk-download".parse().unwrap());
        for (path, status) in [
            ("/v1/video/generations/download-task", "succeeded"),
            (
                "/v1/video/generations/download-task?refresh=true",
                "succeeded",
            ),
            ("/v1/videos/download-task", "completed"),
            ("/v1/videos/video-download-task", "completed"),
        ] {
            let response = handle_video_request(
                &state,
                Method::GET,
                &headers,
                &path.parse().unwrap(),
                Body::empty(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["status"], status);
            assert_eq!(value["url"], format!("http://{addr}/clip"));
        }
        let response = handle_video_request(
            &state,
            Method::GET,
            &HeaderMap::new(),
            &"/v1/videos/video-download-task/content?key=sk-download"
                .parse()
                .unwrap(),
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "video/webm");
        assert_eq!(
            axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .as_ref(),
            b"test-video-bytes"
        );
        server.abort();
    }

    #[test]
    fn recognizes_submit_and_status_paths() {
        assert!(is_video_submit_path("/v1/video/generations"));
        assert!(is_video_submit_path("/video/generations/"));
        assert!(!is_video_submit_path("/v1/video/generations/t-1"));

        assert_eq!(
            video_task_id_from_path("/v1/video/generations/t-1").as_deref(),
            Some("t-1")
        );
        assert_eq!(
            video_task_id_from_path("/v1/video/generations").as_deref(),
            None
        );
        // 多余的分段不是合法的 task id
        assert_eq!(
            video_task_id_from_path("/v1/video/generations/t-1/extra").as_deref(),
            None
        );
    }

    /// 入站 API key 是外部 OpenAI 兼容客户端唯一会带的东西，必须能定出渠道。
    ///
    /// 这条曾经是坏的：key 通过了 `shared_fallback` 的鉴权，但平台解析只认
    /// 请求头/请求体字段，于是合法 key 换回一个 400 —— 端点对 OpenAI 兼容客户端
    /// 完全不可用。
    ///
    /// **任何池的 key 都能调视频端点**：视频不是平台，它是账号的模态分类。
    /// 所以这里断言的是「key 解析出了它所属的池」，而不是「只有视频 key 能用」。
    #[tokio::test]
    async fn resolves_the_platform_from_a_route_proxy_key() {
        use crate::database::repositories::route_proxy_key_repository::RouteProxyKeyRepository;
        use crate::database::{create_memory_pool, run_migrations};
        use axum::http::header::AUTHORIZATION;

        let pool = create_memory_pool().await.expect("pool");
        run_migrations(&pool).await.expect("migrations");
        let key =
            RouteProxyKeyRepository::ensure_platform_key(&pool, "codex", "sk-ai-switch-codex")
                .await
                .expect("mint key");

        // 只带 Authorization，不带任何自定义头 —— 这就是 OpenAI 客户端的样子
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, format!("Bearer {key}").parse().unwrap());
        assert_eq!(
            resolve_video_platform(&pool, &headers, None, br#"{"model":"kling-v1"}"#)
                .await
                .ok()
                .as_deref(),
            Some("codex")
        );

        // 无效 key 且没有头/体字段 → 拒绝
        let mut bogus = HeaderMap::new();
        bogus.insert(AUTHORIZATION, "Bearer sk-nope".parse().unwrap());
        assert!(resolve_video_platform(&pool, &bogus, None, b"{}")
            .await
            .is_err());
    }

    #[test]
    fn strips_platform_field_and_rejects_bad_metadata() {
        let normalized =
            normalize_request_body(br#"{"platform":"codex","model":"kling-v2-6","prompt":"fox"}"#)
                .expect("normalized");
        let value: Value = serde_json::from_slice(&normalized).expect("json");
        assert!(value.get("platform").is_none());
        assert_eq!(value["model"], "kling-v2-6");

        assert!(normalize_request_body(br#"{"metadata":"nope"}"#).is_err());
        assert!(normalize_request_body(br#"{"metadata":{"aspect_ratio":"9:16"}}"#).is_ok());
        assert!(normalize_request_body(b"[]").is_err());
    }

    /// 带迁移的测试状态。
    ///
    /// 需要迁移：OpenAI 原生契约的查询路径会去读 `videogen_tasks`，表不存在会
    /// 变成 500 而不是「任务不存在」的 404 —— 那样测的就不是分派逻辑了。
    async fn test_state() -> AppState {
        let pool = crate::database::create_memory_pool().await.unwrap();
        crate::database::run_migrations(&pool).await.unwrap();
        AppState {
            paths: crate::paths::AppPaths::from_data_dir(
                std::env::temp_dir().join("ai-switch-video-routes-test"),
            ),
            pool,
            config_writes: Default::default(),
            deeplink_protocols: Default::default(),
            close_to_tray: Default::default(),
            route_proxy: Default::default(),
            event_broadcaster: Default::default(),
        }
    }

    /// 两条契约的路径解析。
    #[test]
    fn recognizes_openai_video_paths() {
        assert!(is_openai_video_submit_path("/v1/videos"));
        assert!(is_openai_video_submit_path("/v1/videos/"));
        assert!(!is_openai_video_submit_path("/v1/videos/t-1"));
        // 不做无 `/v1` 前缀的别名（否则 `/videos/generations` 会被顺手放进来）
        assert!(!is_openai_video_submit_path("/videos"));

        assert_eq!(
            openai_video_id_from_path("/v1/videos/t-1").as_deref(),
            Some("t-1")
        );
        assert_eq!(
            openai_video_id_from_path("/v1/videos/t-1/").as_deref(),
            Some("t-1")
        );
        // `/content` 那条不是裸 id；缺 id、多分段也都不认
        assert_eq!(openai_video_id_from_path("/v1/videos/t-1/content"), None);
        assert_eq!(openai_video_id_from_path("/v1/videos"), None);
        assert_eq!(openai_video_id_from_path("/v1/videos/t-1/extra"), None);
        assert_eq!(openai_video_id_from_path("/v1/videos//"), None);
        assert_eq!(openai_video_id_from_path("/videos/t-1"), None);

        assert_eq!(
            openai_video_content_id_from_path("/v1/videos/t-1/content").as_deref(),
            Some("t-1")
        );
        assert_eq!(openai_video_content_id_from_path("/v1/videos/t-1"), None);
        assert_eq!(
            openai_video_content_id_from_path("/v1/videos/t-1/content/x"),
            None
        );
    }

    /// 两条契约的路径族**互不匹配** —— 这是 `handle_video_request` 分派正确的前提。
    #[test]
    fn the_two_contract_path_families_do_not_overlap() {
        for path in ["/v1/videos", "/v1/videos/t-1", "/v1/videos/t-1/content"] {
            assert!(
                !is_video_submit_path(path),
                "new-api 提交路径不该匹配 {path}"
            );
            assert_eq!(video_task_id_from_path(path), None, "{path}");
        }
        for path in ["/v1/video/generations", "/v1/video/generations/t-1"] {
            assert!(
                !is_openai_video_submit_path(path),
                "OpenAI 提交路径不该匹配 {path}"
            );
            assert_eq!(openai_video_id_from_path(path), None, "{path}");
            assert_eq!(openai_video_content_id_from_path(path), None, "{path}");
        }
    }

    /// HTTP 入口的分派与状态码映射（两条契约）。
    ///
    /// 这些分支都在碰到账号池之前就返回了（平台解析失败 / 方法不对 / 路径不匹配 /
    /// 任务不存在），所以不需要 mock 上游。
    #[tokio::test]
    async fn dispatches_methods_and_maps_errors_to_status_codes() {
        use axum::body::Body;
        use axum::http::Method;

        let state = test_state().await;
        RouteProxyKeyRepository::ensure_platform_key(&state.pool, "codex", "sk-video-tests")
            .await
            .unwrap();
        let call = |method: Method, path: &str, body: &str| {
            let state = state.clone();
            let uri: Uri = path.parse().unwrap();
            let body = Body::from(body.to_string());
            async move {
                let mut headers = HeaderMap::new();
                headers.insert("authorization", "Bearer sk-video-tests".parse().unwrap());
                handle_video_request(&state, method, &headers, &uri, body)
                    .await
                    .status()
            }
        };

        // 提交路径只接受 POST（两条契约各一条）
        assert_eq!(
            call(Method::GET, "/v1/video/generations", "").await,
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            call(Method::GET, "/v1/videos", "").await,
            StatusCode::METHOD_NOT_ALLOWED
        );
        // 查询 / 内容路径只接受 GET
        assert_eq!(
            call(Method::POST, "/v1/video/generations/t-1", "{}").await,
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            call(Method::POST, "/v1/videos/t-1", "{}").await,
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            call(Method::POST, "/v1/videos/t-1/content", "{}").await,
            StatusCode::METHOD_NOT_ALLOWED
        );
        // 路径不匹配
        assert_eq!(
            call(Method::POST, "/v1/video/generations/t-1/extra", "{}").await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(Method::GET, "/v1/video/other", "").await,
            StatusCode::NOT_FOUND
        );
        // 不存在的任务：两条契约的查询路径共用同一张表，都该是 404 而不是 500
        assert_eq!(
            call(Method::GET, "/v1/video/generations/t-missing", "").await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(Method::GET, "/v1/videos/t-missing", "").await,
            StatusCode::NOT_FOUND
        );
        // 非 JSON 体（两条契约）
        assert_eq!(
            call(Method::POST, "/v1/video/generations", "not-json").await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call(Method::POST, "/v1/videos", "not-json").await,
            StatusCode::BAD_REQUEST
        );
        // 缺平台（请求头与请求体都没有）
        assert_eq!(
            call(
                Method::POST,
                "/v1/video/generations",
                r#"{"model":"kling-v1","prompt":"fox"}"#
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call(
                Method::POST,
                "/v1/videos",
                r#"{"model":"sora-2","prompt":"fox"}"#
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        // 平台不是视频渠道
        assert_eq!(
            call(
                Method::POST,
                "/v1/video/generations",
                r#"{"platform":"codex","model":"kling-v1","prompt":"fox"}"#
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call(
                Method::POST,
                "/v1/videos",
                r#"{"platform":"codex","model":"sora-2","prompt":"fox"}"#
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        // OpenAI 契约缺 `model` → 400（在碰账号池之前）
        assert_eq!(
            call(
                Method::POST,
                "/v1/videos",
                r#"{"platform":"video","prompt":"fox"}"#
            )
            .await,
            StatusCode::BAD_REQUEST
        );
    }

    /// new-api 契约的 201 体里 `id` 是 `video-{task_id}`，客户端可能拿它来查
    /// OpenAI 的 `GET /v1/videos/{id}`。两条契约指向同一套任务，必须能查到。
    ///
    /// 同时钉住落库的 `seconds`/`size` 会被回显 —— 那是迁移 `202610100003`
    /// 加这两列的唯一理由。
    #[tokio::test]
    async fn openai_lookup_tolerates_the_new_api_id_prefix() {
        use crate::videogen::models::NewVideoTask;

        let state = test_state().await;
        VideoGenerationRepository::create_task(
            &state.pool,
            NewVideoTask {
                task_id: "t-1".into(),
                session_id: None,
                message_id: None,
                model: "sora-2".into(),
                upstream_model: None,
                dialect: "sora".into(),
                credential_id: None,
                upstream_task_id: Some("up-1".into()),
                duration_seconds: Some(8),
                size: Some("1280x720".into()),
                callback_url: None,
                owner_user_id: None,
            },
        )
        .await
        .expect("create task");

        for id in ["t-1", "video-t-1"] {
            let task = load_video_task(&state, id).await.expect(id);
            assert_eq!(task.task_id, "t-1");
            let body: Value = serde_json::from_slice(&openai_video_object(&task)).expect("json");
            assert_eq!(body["id"], "t-1");
            assert_eq!(body["object"], "video");
            assert_eq!(body["model"], "sora-2");
            // 还在 processing → OpenAI 词表里是 `in_progress`
            assert_eq!(body["status"], "in_progress");
            assert_eq!(body["progress"], 0);
            assert_eq!(body["seconds"], "8");
            assert_eq!(body["size"], "1280x720");
            assert_eq!(body["url"], Value::Null);
        }

        // 真的不存在 → 任务不存在（`app_error_response` 会映射成 404）
        let missing = load_video_task(&state, "nope").await.unwrap_err();
        assert_eq!(missing.code(), "videogen.task_not_found");

        // 还没出片时 `/content` 应当是「拿不到产物」而不是去连一个空 URL
        let task = load_video_task(&state, "t-1").await.unwrap();
        let error = openai_video_content(&task).await.unwrap_err();
        assert_eq!(error.code(), "videogen.asset_not_found");
    }
}
