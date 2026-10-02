//! The data plane: the `/v1/*` surface, one auth seam, one forward handler,
//! and the endpoints the gateway answers itself.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::json;

use crate::protocol::Protocol;
use crate::server::Server;
use crate::server::auth::bearer_token;
use crate::server::error::{ServerError, unknown_path};

const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

pub(crate) fn router(server: &Server) -> Router {
    Router::new()
        .route("/v1/messages", post(handle_anthropic))
        .route("/v1/chat/completions", post(handle_openai_chat))
        .route("/v1/responses", post(handle_openai_responses))
        .route("/v1/models", get(models))
        .fallback(unknown_path)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(server.clone())
}

async fn resolve_app(headers: &HeaderMap, server: &Server) -> Result<String, ServerError> {
    let credential = bearer_token(headers)
        .or_else(|| headers.get("x-api-key").and_then(|v| v.to_str().ok()))
        .ok_or(ServerError::MissingAppKey)?;
    server
        .store
        .app_by_key(credential)
        .await?
        .ok_or(ServerError::UnknownAppKey)
}

async fn handle(
    State(server): State<Server>,
    headers: HeaderMap,
    body: Bytes,
    protocol: Protocol,
) -> Response {
    let app = match resolve_app(&headers, &server).await {
        Ok(app) => app,
        Err(err) => return err.into_protocol_response(protocol),
    };
    match server.execute(&app, body, protocol).await {
        Ok(response) => response,
        Err(err) => err.into_protocol_response(protocol),
    }
}

async fn handle_anthropic(state: State<Server>, headers: HeaderMap, body: Bytes) -> Response {
    handle(state, headers, body, Protocol::Anthropic).await
}

async fn handle_openai_chat(state: State<Server>, headers: HeaderMap, body: Bytes) -> Response {
    handle(state, headers, body, Protocol::OpenaiChat).await
}

async fn handle_openai_responses(
    state: State<Server>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle(state, headers, body, Protocol::OpenaiResponses).await
}

async fn models(State(gw): State<Server>, headers: HeaderMap) -> Response {
    let _app = match resolve_app(&headers, &gw).await {
        Ok(app) => app,
        Err(e) => return e.into_protocol_response(Protocol::OpenaiChat),
    };
    let providers = match gw.store.list_providers().await {
        Ok(providers) => providers,
        Err(e) => return ServerError::Db(e).into_protocol_response(Protocol::OpenaiChat),
    };
    let mut data = Vec::new();
    for p in &providers {
        for m in &p.models {
            data.push(json!({
                "id": m.id,
                "object": "model",
                "created": crate::utils::now_unix_secs(),
                "owned_by": p.id,
            }));
        }
    }

    axum::Json(json!({"object": "list", "data": data})).into_response()
}
