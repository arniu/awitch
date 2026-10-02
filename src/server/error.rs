//! The server's error contracts: the vendor envelope a data endpoint speaks
//! for its own failures, and the gateway's own envelope for failures that
//! belong to no protocol.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::protocol::Protocol;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ServerError {
    #[error("missing app-key")]
    MissingAppKey,
    #[error("unknown app-key")]
    UnknownAppKey,
    #[error(transparent)]
    NoRoute(#[from] crate::routing::NoRouteError),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error(transparent)]
    Protocol(#[from] crate::protocol::Error),
    #[error("upstream {status}: {body}")]
    Transport { status: u16, body: String },
    #[error("internal: {0}")]
    Internal(String),
    #[error(transparent)]
    Db(#[from] crate::db::Error),
}

impl ServerError {
    fn http_status(&self) -> StatusCode {
        match self {
            ServerError::MissingAppKey => StatusCode::UNAUTHORIZED,
            ServerError::UnknownAppKey => StatusCode::UNAUTHORIZED,
            ServerError::NoRoute(_) => StatusCode::BAD_GATEWAY,
            ServerError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ServerError::Protocol(_) => StatusCode::BAD_REQUEST,
            ServerError::Transport { .. } => StatusCode::BAD_GATEWAY,
            ServerError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServerError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Error body in the vendor protocol's envelope.
    pub(crate) fn into_protocol_response(self, proto: Protocol) -> Response {
        let status = self.http_status();
        let message = self.to_string();
        let body = proto.spec().error(status, &message);
        (status, axum::Json(body)).into_response()
    }
}

pub(crate) fn error_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({"error": {"message": message, "code": status.as_u16()}});
    (status, axum::Json(body)).into_response()
}

pub(crate) async fn unknown_path(uri: axum::http::Uri) -> Response {
    let message = format!("unknown path: {}", uri.path());
    error_response(StatusCode::NOT_FOUND, &message)
}
