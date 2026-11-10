use crate::app_state::AppState;
use crate::services::route_proxy_service::{
    build_proxy_state, extract_inbound_api_key, is_shared_model_api_path, is_video_generation_path,
    proxy_handler, ProxyAppState,
};
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
struct AiContext {
    state: Arc<AppState>,
    proxy: ProxyAppState,
}

/// This listener is only a model protocol endpoint. Host management is FFI.
pub(crate) fn build_ai_router(state: Arc<AppState>) -> Router {
    let proxy = build_proxy_state(state.pool.clone(), &state.route_proxy);
    Router::new()
        .fallback(ai_request)
        .with_state(AiContext { state, proxy })
        .layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| async move {
                let mut response = next.run(request).await;
                response
                    .headers_mut()
                    .insert("access-control-allow-origin", "*".parse().unwrap());
                response
            },
        ))
}

fn model_route_allowed(method: &Method, raw: &str) -> bool {
    let path = raw.trim_end_matches('/');
    let post = matches!(
        path,
        "/v1/chat/completions"
            | "/chat/completions"
            | "/v1/responses"
            | "/responses"
            | "/v1/messages"
            | "/messages"
            | "/v1/messages/count_tokens"
            | "/messages/count_tokens"
            | "/v1/images/generations"
            | "/images/generations"
            | "/v1/images/edits"
            | "/images/edits"
            | "/v1/audio/speech"
            | "/v1/audio/transcriptions"
            | "/v1/audio/translations"
            | "/v1/videos"
            | "/v1/video/generations"
            | "/video/generations"
    ) || ["/v1/models/", "/v1beta/models/", "/v1alpha/models/"]
        .iter()
        .any(|prefix| {
            path.strip_prefix(prefix).is_some_and(|tail| {
                !tail.contains('/')
                    && [":generateContent", ":streamGenerateContent", ":countTokens"]
                        .iter()
                        .any(|action| tail.ends_with(action))
            })
        });
    let get = matches!(
        path,
        "/models" | "/v1/models" | "/v1beta/models" | "/v1alpha/models"
    ) || [
        "/v1/video/generations/",
        "/video/generations/",
        "/v1/videos/",
    ]
    .iter()
    .any(|prefix| {
        path.strip_prefix(prefix).is_some_and(|tail| {
            let parts: Vec<_> = tail.split('/').collect();
            !parts[0].is_empty()
                && (parts.len() == 1
                    || (*prefix == "/v1/videos/" && parts.len() == 2 && parts[1] == "content"))
        })
    });
    (method == Method::POST && post)
        || (method == Method::GET && get)
        || (method == Method::OPTIONS && (post || get))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn browser_preflight_is_allowed_but_management_and_non_model_apis_are_not() {
        let pool = crate::database::create_memory_pool().await.unwrap();
        crate::database::run_migrations(&pool).await.unwrap();
        let state = Arc::new(AppState {
            pool,
            paths: crate::paths::AppPaths::from_data_dir(
                std::env::temp_dir().join("otools-router-test"),
            ),
            config_writes: Default::default(),
            deeplink_protocols: Default::default(),
            close_to_tray: Default::default(),
            route_proxy: Default::default(),
            event_broadcaster: Default::default(),
        });
        let app = build_ai_router(state);
        for (method, path, expected) in [
            (Method::OPTIONS, "/v1/chat/completions", 204),
            (Method::OPTIONS, "/api/get_settings", 404),
            (Method::GET, "/v1/files", 404),
            (Method::DELETE, "/v1/files/secret", 404),
            (Method::POST, "/v1/chat/completions", 401),
        ] {
            let response = app
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .method(method)
                        .uri(path)
                        .header("origin", "https://client.example")
                        .header("access-control-request-method", "POST")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), expected, "{path}");
            if expected == 204 {
                assert_eq!(response.headers()["access-control-allow-origin"], "*");
            }
        }
    }
}

async fn ai_request(
    State(context): State<AiContext>,
    method: Method,
    mut headers: HeaderMap,
    uri: Uri,
    body: Body,
) -> Response {
    if !model_route_allowed(&method, uri.path()) {
        return (StatusCode::NOT_FOUND, Json(json!({"error":{"code":"route.not_found","message":"This listener only serves AI model protocols"}}))).into_response();
    }
    if method == Method::OPTIONS {
        return (StatusCode::NO_CONTENT, [("access-control-allow-origin", "*"), ("access-control-allow-methods", "GET, POST, OPTIONS"), ("access-control-allow-headers", "authorization, content-type, x-api-key, x-goog-api-key, anthropic-version, anthropic-beta, openai-beta"), ("access-control-max-age", "600")]).into_response();
    }
    let key = extract_inbound_api_key(&headers, uri.query());
    let keys = match crate::database::repositories::route_proxy_key_repository::RouteProxyKeyRepository::list_all(&context.state.pool).await {
        Ok(keys) => keys,
        Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":{"code":"auth.unavailable","message":"Local key store unavailable"}}))).into_response(),
    };
    if !key
        .as_ref()
        .is_some_and(|key| keys.iter().any(|(stored, _)| stored == key))
    {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error":{"code":"invalid_api_key","message":"A valid local API key is required"}}))).into_response();
    }
    // Platform is derived exclusively from the verified local key, never a header.
    headers.remove("x-ai-switch-platform");
    if is_video_generation_path(uri.path()) {
        return crate::web::video_routes::handle_video_request(
            &context.state,
            method,
            &headers,
            &uri,
            body,
        )
        .await;
    }
    proxy_handler(State(context.proxy), method, headers, uri, body).await
}
