use axum::response::Response;
use tokio::sync::oneshot;

use crate::ledger::LedgerNew;
use crate::protocol::translate::stream::StreamEnd;
use crate::protocol::{Metadata, Protocol};
use crate::provider::{Attempt, AttemptOutcome};
use crate::routing::Route;
use crate::utils::now_unix_secs;

#[derive(Clone)]
pub(super) struct Requested {
    pub protocol: Protocol,
    pub model: String,
    pub app: String,
}

pub(super) struct Forward {
    pub requested: Requested,
    pub route: Route,
    pub url: String,
    pub key: String,
}

impl Forward {
    pub(super) fn ledger(&self, metadata: Metadata) -> LedgerNew {
        LedgerNew {
            app: self.requested.app.clone(),
            provider_id: self.route.provider.clone(),
            response_id: metadata.response_id,
            conversation_id: metadata.conversation_id,
            requested_protocol: self.requested.protocol,
            requested_model: self.requested.model.clone(),
            served_protocol: self.route.protocol,
            served_model: self.route.model.clone(),
            usage: metadata.usage,
            price: self.route.price,
        }
    }

    pub(super) fn attempt(&self, outcome: AttemptOutcome, latency: u64) -> Attempt {
        Attempt {
            provider_id: self.route.provider.clone(),
            outcome,
            latency,
            at: now_unix_secs(),
        }
    }
}

pub(super) enum ForwardOutcome {
    Done {
        response: Response,
        metadata: Metadata,
    },
    Stream {
        response: Response,
        ended: oneshot::Receiver<StreamEnd>,
    },
}
