use std::time::Instant;

use axum::body::Bytes;
use axum::response::Response;
use serde_json::Value;
use tokio::sync::oneshot;

mod forward;
mod types;

use types::{Forward, ForwardOutcome, Requested};

use crate::db::Store;
use crate::protocol::translate::stream::StreamEnd;
use crate::protocol::{self, Continuation, Metadata, Protocol};
use crate::provider::AttemptOutcome;
use crate::routing::{Route, RouteRequest, RoutingInput, ranked_routes};
use crate::server::Server;
use crate::server::error::ServerError;

// ── pipeline entry point ────────────────────────────────────────

pub(crate) async fn process(
    server: &Server,
    app: &str,
    body: Bytes,
    protocol: Protocol,
) -> Result<Response, ServerError> {
    let req_json: Value = serde_json::from_slice(&body)
        .map_err(|e| ServerError::BadRequest(format!("invalid JSON body: {e}")))?;
    let streaming = req_json["stream"].as_bool() == Some(true);

    let input = server.store.routing_snapshot(app).await?;
    let route = resolve_route(server, protocol, &req_json).await?;
    let candidates = ranked_routes(&route, &input)?;

    let requested = Requested {
        app: app.to_string(),
        protocol,
        model: route.model,
    };

    forward_with_fallback(server, requested, candidates, &input, req_json, streaming).await
}

// ── routing ─────────────────────────────────────────────────────

async fn resolve_route(
    server: &Server,
    protocol: Protocol,
    body: &Value,
) -> Result<RouteRequest, ServerError> {
    let model = body["model"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| ServerError::BadRequest("request has no model".into()))?;

    let continuation = protocol.spec().continuation(body);
    let forced_provider = match &continuation {
        Some(Continuation::Response(id)) => server.store.provider_by_response(id).await?,
        Some(Continuation::Conversation(id)) => server.store.provider_by_conversation(id).await?,
        None => None,
    };

    Ok(RouteRequest {
        protocol,
        model,
        now: chrono::Utc::now().time(),
        forced_provider,
        continuation: continuation.is_some(),
    })
}

// ── forward with fallback ───────────────────────────────────────

async fn forward_with_fallback(
    server: &Server,
    requested: Requested,
    candidates: Vec<Route>,
    input: &RoutingInput,
    req_json: Value,
    streaming: bool,
) -> Result<Response, ServerError> {
    let mut last_err: Option<ServerError> = None;
    for (idx, candidate) in candidates.iter().enumerate() {
        let provider = input
            .providers
            .iter()
            .find(|rp| rp.provider.id == candidate.provider)
            .map(|rp| &rp.provider)
            .ok_or_else(|| {
                ServerError::Internal(
                    "route() resolved a provider missing from the routing snapshot".into(),
                )
            })?;

        let Some(url) = provider.url_for(candidate.protocol) else {
            return Err(ServerError::Internal(format!(
                "{} is not served by {}",
                candidate.protocol, candidate.provider
            )));
        };

        let forward = Forward {
            requested: requested.clone(),
            route: candidate.clone(),
            key: provider.key.clone(),
            url,
        };

        let attempt_t0 = Instant::now();
        let outcome =
            forward::forward_once(&server.transport, &forward, &req_json, streaming).await;
        let attempt_latency = attempt_t0.elapsed().as_millis() as u64;

        match outcome {
            Ok(ForwardOutcome::Done { response, metadata }) => {
                let ledger = forward.ledger(metadata);
                tracing::info!(
                    app = %ledger.app,
                    provider_id = %ledger.provider_id,
                    model = %ledger.served_model,
                    input_tokens = ledger.usage.input_tokens,
                    output_tokens = ledger.usage.output_tokens,
                    latency_ms = attempt_latency,
                    "forwarded"
                );
                server
                    .store
                    .enqueue_attempt(forward.attempt(AttemptOutcome::Delivered, attempt_latency));
                server.store.enqueue_ledger(ledger);
                return Ok(response);
            }

            Ok(ForwardOutcome::Stream { response, ended }) => {
                let store = server.store.clone();
                let tasks = server.stream_tasks.clone();
                tasks.spawn(async move {
                    record_stream_end(store, forward, attempt_latency, ended).await;
                });

                return Ok(response);
            }

            Err(err) => {
                let retryable = failure_outcome(&err);
                if let Some(outcome) = retryable {
                    server
                        .store
                        .enqueue_attempt(forward.attempt(outcome, attempt_latency));
                }

                if idx + 1 < candidates.len() && retryable.is_some() {
                    tracing::warn!(
                        provider = %forward.route.provider,
                        error = %err,
                        "forward failed; trying next candidate"
                    );
                    last_err = Some(err);
                    continue;
                }
                tracing::error!(error = %err, "forward failed");
                return Err(err);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        ServerError::Internal("candidate list exhausted without a result".into())
    }))
}

// ── ledger helpers ──────────────────────────────────────────────

fn failure_outcome(err: &ServerError) -> Option<AttemptOutcome> {
    match err {
        ServerError::Transport { .. } | ServerError::Protocol(protocol::Error::Json(_)) => {
            Some(AttemptOutcome::Failed)
        }
        _ => None,
    }
}

async fn record_stream_end(
    store: Store,
    forward: Forward,
    latency: u64,
    ended: oneshot::Receiver<StreamEnd>,
) {
    let (outcome, metadata) = classify_stream_end(ended.await);
    if let Some(metadata) = metadata {
        store.enqueue_ledger(forward.ledger(metadata));
    }
    store.enqueue_attempt(forward.attempt(outcome, latency));
}

fn classify_stream_end(
    end: Result<StreamEnd, oneshot::error::RecvError>,
) -> (AttemptOutcome, Option<Metadata>) {
    match end {
        Ok(StreamEnd::Completed(metadata)) => (AttemptOutcome::Delivered, Some(metadata)),
        Ok(StreamEnd::Errored) => (AttemptOutcome::Failed, None),
        Ok(StreamEnd::Dropped) | Err(_) => (AttemptOutcome::Aborted, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_outcome_keeps_retryable_and_terminal_apart() {
        for s in [0u16, 401, 403, 404, 429, 500, 599] {
            assert!(
                failure_outcome(&ServerError::Transport {
                    status: s,
                    body: "".into()
                })
                .is_some(),
                "status {s}"
            );
        }
        assert!(
            failure_outcome(&ServerError::Protocol(protocol::Error::Json(
                serde_json::from_str::<serde_json::Value>("invalid").unwrap_err()
            )))
            .is_some()
        );
    }

    #[test]
    fn classify_stream_end_only_blames_an_errored_stream_on_the_provider() {
        let (outcome, metadata) =
            classify_stream_end(Ok(StreamEnd::Completed(Metadata::default())));
        assert_eq!(outcome, AttemptOutcome::Delivered);
        assert!(metadata.is_some());

        let (outcome, metadata) = classify_stream_end(Ok(StreamEnd::Errored));
        assert_eq!(outcome, AttemptOutcome::Failed);
        assert!(metadata.is_none());

        let (tx, rx) = oneshot::channel();
        drop(tx);

        for end in [Ok(StreamEnd::Dropped), rx.blocking_recv()] {
            let (outcome, metadata) = classify_stream_end(end);
            assert_eq!(outcome, AttemptOutcome::Aborted);
            assert!(metadata.is_none());
        }
    }
}
