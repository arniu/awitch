use axum::http::header;
use axum::response::Response;
use serde_json::Value;

use super::types::{Forward, ForwardOutcome};
use crate::protocol::translate::Pair;
use crate::server::error::ServerError;
use crate::server::transport::ReqwestTransport;

pub(super) async fn forward_once(
    transport: &ReqwestTransport,
    forward: &Forward,
    body: &Value,
    streaming: bool,
) -> Result<ForwardOutcome, ServerError> {
    let pair = Pair::resolve(forward.requested.protocol, forward.route.protocol);
    let mut body = pair.transform_request(body.clone())?;
    body["model"] = serde_json::json!(&forward.route.model);
    let headers = forward.route.protocol.spec().auth_headers(&forward.key);
    let resp = transport.send(&forward.url, &body, headers).await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let err_body = resp.text().await.unwrap_or_default();
        let body = serde_json::from_str::<Value>(&err_body)
            .ok()
            .and_then(|v| forward.route.protocol.spec().extract_error_message(&v))
            .unwrap_or(err_body);
        return Err(ServerError::Transport { status, body });
    }

    if streaming {
        let outcome =
            pair.serve_stream(resp.bytes_stream())
                .await
                .map_err(|e| ServerError::Transport {
                    status: 0,
                    body: e.to_string(),
                })?;
        let response = set_content_type(
            Response::new(axum::body::Body::from_stream(outcome.bytes)),
            "text/event-stream",
        );
        Ok(ForwardOutcome::Stream {
            response,
            ended: outcome.ended,
        })
    } else {
        let upstream = resp.bytes().await.map_err(|e| ServerError::Transport {
            status: 0,
            body: e.to_string(),
        })?;
        let (metadata, bytes) = pair.serve(upstream)?;
        Ok(ForwardOutcome::Done {
            response: set_content_type(
                Response::new(axum::body::Body::from(bytes)),
                "application/json",
            ),
            metadata,
        })
    }
}

fn set_content_type(mut resp: Response, ct: &'static str) -> Response {
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, header::HeaderValue::from_static(ct));
    resp
}
