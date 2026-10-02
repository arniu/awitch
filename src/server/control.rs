//! Control API
//!
//! Management endpoints under `/api/`.
//!
//! Auth: per-install token as `Authorization: Bearer <token>`.
//! The control listener is a separate TCP port.
//!
//! Response envelope:
//! - Single resource → `{ ...fields }` (the resource directly)
//! - Collection     → `{ "data": [...] }`
//! - Mutation       → 204 No Content, or the resulting resource
//! - Membership     → 204 No Content or 404, for a relation read
//! - Error          → `{ "error": { "message": "...", "code": <status> } }`
//!
//! Concurrency: all handlers are async over the store; there is no
//! process-wide lock.

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use subtle::ConstantTimeEq;

use crate::api_types::Health;

use super::Server;
use super::auth::bearer_token;
use super::error::{error_response, unknown_path};

pub(crate) fn router(server: &Server) -> Router {
    let control_token = server.control_token.clone();
    let routes = Router::new()
        .nest("/api", api(server.clone()))
        .fallback(unknown_path)
        .with_state(server.clone());
    routes
        .layer(middleware::from_fn_with_state(
            control_token,
            require_control_token,
        ))
        .layer(middleware::map_response(envelope_errors))
}

async fn require_control_token(
    State(control_token): State<String>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let provided = bearer_token(req.headers());
    if provided.is_some_and(|p| token_matches(p, &control_token)) {
        return next.run(req).await;
    }

    let mut resp = StatusCode::UNAUTHORIZED.into_response();
    resp.headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    resp
}

fn token_matches(provided: &str, expected: &str) -> bool {
    let (provided, expected) = (provided.as_bytes(), expected.as_bytes());
    provided.len() == expected.len() && bool::from(provided.ct_eq(expected))
}

/// Enough for any rejection message: an error path must not become a way to
/// make the server buffer an arbitrary body.
const MAX_ERROR_BODY: usize = 8 * 1024;

/// Every error the control plane returns wears the same envelope — the ones
/// our handlers build, and axum's own rejections too. This layer is what makes
/// that total: a 405, a 413 or a rejected `Path` never reaches a handler, so
/// no handler can shape them.
async fn envelope_errors(resp: Response) -> Response {
    let status = resp.status();
    if !status.is_client_error() && !status.is_server_error() {
        return resp;
    }
    // Ours are already enveloped.
    if resp
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|v| v.as_bytes() == b"application/json")
    {
        return resp;
    }

    let (parts, body) = resp.into_parts();
    let text = axum::body::to_bytes(body, MAX_ERROR_BODY)
        .await
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .unwrap_or_default();
    let message = match text.is_empty() {
        false => text,
        true => status
            .canonical_reason()
            .unwrap_or("request failed")
            .to_lowercase(),
    };

    let mut out = error_response(status, &message);
    // Keep what the framework put on it (`Allow` on a 405, say), minus the
    // headers that described the body we just replaced.
    let headers = out.headers_mut();
    for (name, value) in parts.headers.iter() {
        headers.insert(name.clone(), value.clone());
    }
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );

    headers.remove(header::CONTENT_LENGTH);

    out
}

fn api(server: Server) -> Router<Server> {
    Router::new()
        .route("/health", get(health))
        .route("/provider-templates", get(provider_templates::list))
        .route(
            "/provider-templates/{id}/providers",
            post(provider_templates::instantiate),
        )
        .route("/providers", get(providers::list).post(providers::add))
        .route(
            "/providers/{id}",
            get(providers::get)
                .patch(providers::update)
                .delete(providers::delete),
        )
        .route("/providers/{id}/balance", post(providers::poll_balance))
        .route("/providers/{id}/pins", get(providers::pinned_apps))
        .route("/pins", get(pins::list).delete(pins::clear_all))
        .route(
            "/pins/{app}",
            get(pins::pinned_providers).delete(pins::clear_app),
        )
        .route(
            "/pins/{app}/{provider}",
            get(pins::exists).put(pins::add).delete(pins::remove),
        )
        .route("/usage", get(usage::show))
        .route("/usage/app/{app}", get(usage::app))
        .route("/usage/provider/{id}", get(usage::provider))
        .route("/settings", get(settings::get).patch(settings::patch))
        .route("/app-keys", get(app_keys::list).post(app_keys::issue))
        .route(
            "/app-keys/{id}",
            get(app_keys::get).delete(app_keys::revoke),
        )
        .with_state(server)
}

/// Control-plane error: handlers return `Result`, and the envelope shape comes
/// from [`error_response`], so it cannot drift across endpoints.
#[derive(Debug, thiserror::Error)]
enum ControlError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    BadRequest(String),
    /// A create named a taken provider id — a state conflict, not a malformed
    /// request (409): the caller must free or change the id.
    #[error("{0}")]
    Conflict(String),
    /// An upstream control call (balance poll) failed — a 502 to the caller.
    /// The upstream module is display-only, so the payload is the message;
    /// no error chain is consumed anywhere.
    #[error("upstream request failed: {0}")]
    Upstream(String),
    #[error(transparent)]
    Internal(crate::db::Error),
}

/// A record the store refused is the caller's input; any other store failure
/// is ours.
impl From<crate::db::Error> for ControlError {
    fn from(e: crate::db::Error) -> ControlError {
        match e {
            crate::db::Error::InvalidRecord(err) => ControlError::BadRequest(err.to_string()),
            e => ControlError::Internal(e),
        }
    }
}

impl IntoResponse for ControlError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ControlError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ControlError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ControlError::Conflict(m) => (StatusCode::CONFLICT, m),
            ControlError::Upstream(m) => (StatusCode::BAD_GATEWAY, m),
            ControlError::Internal(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };

        error_response(status, &message)
    }
}

fn not_found(what: &str) -> ControlError {
    ControlError::NotFound(format!("{what} not found"))
}

fn provider_not_found(id: &str) -> ControlError {
    not_found(&format!("provider '{id}'"))
}

fn app_key_not_found(id: &str) -> ControlError {
    not_found(&format!("app-key '{id}'"))
}

fn bad_request(msg: &str) -> ControlError {
    ControlError::BadRequest(msg.to_string())
}

fn ok_with<T: serde::Serialize>(v: T) -> Response {
    (StatusCode::OK, axum::Json(v)).into_response()
}

/// A 201 that locates the created resource.
fn created<T: serde::Serialize>(location: &str, payload: T) -> Response {
    let mut resp = (StatusCode::CREATED, axum::Json(payload)).into_response();
    if let Ok(value) = HeaderValue::from_str(location) {
        resp.headers_mut().insert(header::LOCATION, value);
    }

    resp
}

async fn health() -> Response {
    ok_with(Health {
        version: env!("CARGO_PKG_VERSION"),
    })
}

mod provider_templates {
    use axum::extract::{Json, Path, State};
    use axum::response::Response;

    use crate::api_types::{List, TemplateInstantiate};
    use crate::provider::{ProviderNew, ProviderTemplate};

    use super::providers;
    use super::{ControlError, Server, ok_with};

    pub(super) async fn list() -> Result<Response, ControlError> {
        let templates: Vec<ProviderTemplate> = crate::provider::builtin_templates().collect();
        Ok(ok_with(List { data: templates }))
    }

    pub(super) async fn instantiate(
        State(state): State<Server>,
        Path(template_id): Path<String>,
        Json(req): Json<TemplateInstantiate>,
    ) -> Result<Response, ControlError> {
        let new = ProviderNew::from_template(&template_id, &req.key)
            .map_err(|e| ControlError::NotFound(e.to_string()))?
            .with_id(req.id);
        providers::insert(&state, new).await
    }
}

mod providers {
    use axum::extract::{Json, Path, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};

    use crate::api_types::{self, List, ProviderAdd};
    use crate::provider::{Provider, ProviderNew, ProviderPatch};

    use super::{ControlError, Server, created, ok_with, provider_not_found};

    pub(super) async fn list(State(state): State<Server>) -> Result<Response, ControlError> {
        let input = state.store.routing_snapshot("").await?;
        let data: Vec<api_types::ProviderView> = input.providers.iter().map(routing_view).collect();
        Ok(ok_with(List { data }))
    }

    pub(super) async fn get(
        State(state): State<Server>,
        Path(id): Path<String>,
    ) -> Result<Response, ControlError> {
        let provider = fetch(&state, &id).await?;
        Ok(ok_with(view(&state, provider).await?))
    }

    pub(super) async fn add(
        State(state): State<Server>,
        Json(req): Json<ProviderAdd>,
    ) -> Result<Response, ControlError> {
        insert(&state, ProviderNew::from(req)).await
    }

    pub(super) async fn delete(
        State(state): State<Server>,
        Path(id): Path<String>,
    ) -> Result<Response, ControlError> {
        match state.store.delete_provider(&id).await? {
            true => {
                // deleting a provider also clears its pins (cascade)
                Ok(StatusCode::NO_CONTENT.into_response())
            }
            false => Err(provider_not_found(&id)),
        }
    }

    pub(super) async fn update(
        State(state): State<Server>,
        Path(id): Path<String>,
        Json(patch): Json<ProviderPatch>,
    ) -> Result<Response, ControlError> {
        match state.store.update_provider(&id, &patch).await? {
            Some(provider) => Ok(ok_with(view(&state, provider).await?)),
            None => Err(provider_not_found(&id)),
        }
    }

    /// The apps pinning one provider.
    pub(super) async fn pinned_apps(
        State(state): State<Server>,
        Path(id): Path<String>,
    ) -> Result<Response, ControlError> {
        fetch(&state, &id).await?;
        let data: Vec<String> = state
            .store
            .pins_by_provider(&id)
            .await?
            .into_iter()
            .map(|pin| pin.app)
            .collect();
        Ok(ok_with(List { data }))
    }

    pub(super) async fn poll_balance(
        State(state): State<Server>,
        Path(id): Path<String>,
    ) -> Result<Response, ControlError> {
        let provider = fetch(&state, &id).await?;
        let balance = crate::provider_upstream::poll_balance(state.http_client(), &provider)
            .await
            .map_err(|e| ControlError::Upstream(e.to_string()))?;
        state.store.insert_balance(&provider.id, &balance).await?;
        Ok(ok_with(balance))
    }

    pub(super) async fn insert(state: &Server, new: ProviderNew) -> Result<Response, ControlError> {
        let requested_id = new.id.clone();
        let maybe_provider = state.store.insert_provider(new).await?;
        let Some(provider) = maybe_provider else {
            return Err(ControlError::Conflict(format!(
                "provider id '{}' already exists",
                requested_id.unwrap_or_default()
            )));
        };

        state
            .task_tx
            .send(crate::tasks::TaskEvent::SyncProvider(provider.id.clone()))
            .await
            .ok();

        Ok(created(
            &format!("/api/providers/{}", provider.id),
            view(state, provider).await?,
        ))
    }

    async fn fetch(state: &Server, provider_id: &str) -> Result<Provider, ControlError> {
        state
            .store
            .get_provider(provider_id)
            .await?
            .ok_or_else(|| provider_not_found(provider_id))
    }

    /// A provider and the gateway's current observation of it.
    pub(super) fn routing_view(rp: &crate::routing::RoutingProvider) -> api_types::ProviderView {
        api_types::ProviderView {
            provider: (*rp.provider).clone(),
            metrics: rp.metrics.clone(),
        }
    }

    async fn view(
        state: &Server,
        provider: Provider,
    ) -> Result<api_types::ProviderView, ControlError> {
        let input = state.store.routing_snapshot("").await?;
        let metrics = input
            .providers
            .iter()
            .find(|rp| rp.provider.id == provider.id)
            .map(|rp| rp.metrics.clone())
            .unwrap_or_default();
        Ok(api_types::ProviderView { provider, metrics })
    }
}

/// The pin relation: an app pinned to a provider.
mod pins {
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};

    use crate::api_types::{List, ProviderView};

    use super::providers::routing_view;
    use super::{ControlError, Server, not_found, ok_with, provider_not_found};

    pub(super) async fn list(State(state): State<Server>) -> Result<Response, ControlError> {
        let data = state.store.list_pins().await?;
        Ok(ok_with(List { data }))
    }

    /// The providers an app pins, in the order it pinned them.
    pub(super) async fn pinned_providers(
        State(state): State<Server>,
        Path(app): Path<String>,
    ) -> Result<Response, ControlError> {
        let pins = state.store.pins_by_app(&app).await?;
        let input = state.store.routing_snapshot(&app).await?;
        let data: Vec<ProviderView> = pins
            .iter()
            .filter_map(|pin| {
                input
                    .providers
                    .iter()
                    .find(|rp| rp.provider.id == pin.provider_id)
            })
            .map(routing_view)
            .collect();
        Ok(ok_with(List { data }))
    }

    pub(super) async fn exists(
        State(state): State<Server>,
        Path((app, provider)): Path<(String, String)>,
    ) -> Result<Response, ControlError> {
        if state.store.has_pin(&app, &provider).await? {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }

        Err(not_found(&format!("pin '{app}/{provider}'")))
    }

    pub(super) async fn add(
        State(state): State<Server>,
        Path((app, provider)): Path<(String, String)>,
    ) -> Result<Response, ControlError> {
        // The one foreign key this write can violate is the provider it names:
        // that is the caller's 404, not ours.
        if let Err(crate::db::Error::ReferencedRowMissing) =
            state.store.insert_pin(&app, &provider).await
        {
            return Err(provider_not_found(&provider));
        }

        Ok(StatusCode::NO_CONTENT.into_response())
    }

    pub(super) async fn remove(
        State(state): State<Server>,
        Path((app, provider)): Path<(String, String)>,
    ) -> Result<Response, ControlError> {
        state.store.delete_pin(&app, &provider).await?;
        Ok(StatusCode::NO_CONTENT.into_response())
    }

    /// DELETE /pins — every app's set, cleared (whole-table reset).
    pub(super) async fn clear_all(State(state): State<Server>) -> Result<Response, ControlError> {
        state.store.clear_all_pins().await?;
        Ok(StatusCode::NO_CONTENT.into_response())
    }

    pub(super) async fn clear_app(
        State(state): State<Server>,
        Path(app): Path<String>,
    ) -> Result<Response, ControlError> {
        state.store.clear_pins(&app).await?;
        Ok(StatusCode::NO_CONTENT.into_response())
    }
}

mod usage {
    use std::collections::HashMap;

    use axum::extract::{Path, Query, State};
    use axum::response::Response;

    use crate::api_types;
    use crate::ledger;

    use super::{ControlError, Server, bad_request, ok_with};

    pub(super) async fn show(
        State(state): State<Server>,
        Query(q): Query<api_types::UsageQuery>,
    ) -> Result<Response, ControlError> {
        query(&state, &q, None, None).await
    }

    pub(super) async fn app(
        Path(app): Path<String>,
        State(state): State<Server>,
        Query(q): Query<api_types::UsageQuery>,
    ) -> Result<Response, ControlError> {
        query(&state, &q, Some(&app), None).await
    }

    pub(super) async fn provider(
        Path(provider_id): Path<String>,
        State(state): State<Server>,
        Query(q): Query<api_types::UsageQuery>,
    ) -> Result<Response, ControlError> {
        query(&state, &q, None, Some(&provider_id)).await
    }

    async fn query(
        state: &Server,
        q: &api_types::UsageQuery,
        app: Option<&str>,
        provider_id: Option<&str>,
    ) -> Result<Response, ControlError> {
        let now = crate::utils::now_unix_secs();
        if q.from > now {
            return Err(bad_request("from is in the future"));
        }

        let width = q.bucket_width;
        let (start, end) = width.window(q.from, now);
        let rows = state
            .store
            .usage_window(start, end, width, app, provider_id)
            .await?;

        let mut by_start: HashMap<i64, Vec<api_types::UsageRow>> = HashMap::new();
        for row in rows {
            by_start
                .entry(row.bucket_start)
                .or_default()
                .push(row_view(row));
        }

        let buckets = width
            .buckets(start, end)
            .map(|(bucket_start, bucket_end)| api_types::UsageBucket {
                start: bucket_start,
                end: bucket_end,
                rows: by_start.remove(&bucket_start).unwrap_or_default(),
            })
            .collect();

        Ok(ok_with(api_types::UsageReport {
            start,
            end,
            bucket_width: width,
            buckets,
        }))
    }

    fn row_view(row: ledger::UsageSummary) -> api_types::UsageRow {
        api_types::UsageRow {
            app: row.app,
            provider_id: row.provider_id,
            provider_name: row.provider_name,
            served_model: row.served_model,
            requests: row.requests,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            cost: money::Money {
                amount: row.cost,
                currency: crate::pricing::ACCOUNTING_CURRENCY,
            },
        }
    }
}

mod settings {
    use axum::extract::{Json, State};
    use axum::response::Response;

    use super::{ControlError, Server, ok_with};

    pub(super) async fn get(State(state): State<Server>) -> Result<Response, ControlError> {
        let s = state.store.get_settings().await?;
        Ok(ok_with(serde_json::to_value(&s).unwrap_or_default()))
    }

    pub(super) async fn patch(
        State(state): State<Server>,
        Json(body): Json<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<Response, ControlError> {
        let mut writes = Vec::new();
        for (key, raw) in &body {
            let field = crate::settings::SETTINGS
                .iter()
                .find(|s| s.key() == key.as_str())
                .ok_or_else(|| ControlError::BadRequest(format!("unknown setting '{key}'")))?;
            let stored = match raw {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                other => return Err(ControlError::BadRequest(format!("invalid {key} '{other}'"))),
            };
            field.validate(&stored).map_err(ControlError::BadRequest)?;
            writes.push((key.clone(), stored));
        }

        if !writes.is_empty() {
            state.store.set_settings(&writes).await?;
        }

        let s = state.store.get_settings().await?;
        Ok(ok_with(serde_json::to_value(&s).unwrap_or_default()))
    }
}

mod app_keys {
    use axum::extract::{Json, Path, Query, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};

    use crate::api_types::{AppKeyIssue, AppKeyIssued, AppKeyView, AppKeysQuery, List};

    use super::{ControlError, Server, app_key_not_found, created, ok_with};

    pub(super) async fn issue(
        State(state): State<Server>,
        Json(req): Json<AppKeyIssue>,
    ) -> Result<Response, ControlError> {
        let key = crate::utils::random_hex(16);
        let row = state.store.insert_app_key(&req.app, &key).await?;
        let issued = AppKeyIssued {
            id: row.id,
            app: row.app,
            created_at: crate::utils::iso8601(row.created_at),
            last4: crate::utils::last4(&key),
            key,
        };
        Ok(created(&format!("/api/app-keys/{}", issued.id), issued))
    }

    pub(super) async fn list(
        Query(q): Query<AppKeysQuery>,
        State(state): State<Server>,
    ) -> Result<Response, ControlError> {
        let rows = match q.app.as_deref() {
            Some(a) => state.store.keys_by_app(a).await?,
            None => state.store.list_app_keys().await?,
        };
        let data: Vec<AppKeyView> = rows.iter().map(view).collect();
        Ok(ok_with(List { data }))
    }

    pub(super) async fn get(
        State(state): State<Server>,
        Path(id): Path<String>,
    ) -> Result<Response, ControlError> {
        match state.store.get_app_key(&id).await? {
            Some(row) => Ok(ok_with(view(&row))),
            None => Err(app_key_not_found(&id)),
        }
    }

    pub(super) async fn revoke(
        State(state): State<Server>,
        Path(id): Path<String>,
    ) -> Result<Response, ControlError> {
        match state.store.delete_app_key(&id).await? {
            true => Ok(StatusCode::NO_CONTENT.into_response()),
            false => Err(app_key_not_found(&id)),
        }
    }

    fn view(row: &crate::db::AppKey) -> AppKeyView {
        AppKeyView {
            id: row.id.clone(),
            app: row.app.clone(),
            created_at: crate::utils::iso8601(row.created_at),
            last4: crate::utils::last4(&row.key),
        }
    }
}
