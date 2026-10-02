use std::collections::HashMap;
use std::ops::ControlFlow;
use std::time::Duration;

use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot};

use crate::settings::Settings;

use super::Error;
use super::cache::RoutingCache;
use super::commands::{self, Ctx};

/// Actor mailbox depth: backpressure bound, not capacity.
pub(super) const CHANNEL_BOUND: usize = 64;
/// Worst-case wait for an actor reply before failing the request.
pub(super) const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) type Reply<T> = oneshot::Sender<std::result::Result<T, Error>>;

/// The reply *address* is not part of a command — that belongs to the mailbox
/// item.
pub(super) trait Command: Send + 'static {
    type Reply: Send + 'static;

    fn run(self, cx: &mut Ctx<'_>) -> std::result::Result<Self::Reply, Error>;
}

pub(super) trait Envelope: Send {
    fn handle(self: Box<Self>, cx: &mut Ctx<'_>) -> ControlFlow<()>;
}

pub(super) struct Ask<C: Command> {
    pub(super) command: C,
    pub(super) reply: Reply<C::Reply>,
}

pub(super) struct Tell<C: Command<Reply = ()>> {
    pub(super) label: &'static str,
    pub(super) command: C,
}

/// The pill: stop once everything already queued has been handled.
pub(super) struct Shutdown;

impl<C: Command> Envelope for Ask<C> {
    fn handle(self: Box<Self>, cx: &mut Ctx<'_>) -> ControlFlow<()> {
        let _ = self.reply.send(self.command.run(cx));
        ControlFlow::Continue(())
    }
}

impl<C: Command<Reply = ()>> Envelope for Tell<C> {
    fn handle(self: Box<Self>, cx: &mut Ctx<'_>) -> ControlFlow<()> {
        // Fire-and-forget is not the same as unobserved: a row that cannot be
        // handled is logged here, not dropped in silence.
        if let Err(e) = self.command.run(cx) {
            tracing::error!("{}: {e}", self.label);
        }
        ControlFlow::Continue(())
    }
}

impl Envelope for Shutdown {
    fn handle(self: Box<Self>, _cx: &mut Ctx<'_>) -> ControlFlow<()> {
        ControlFlow::Break(())
    }
}

pub(super) struct ActorState {
    conn: Connection,
    settings: Settings,
    routing: RoutingCache,
    keys: HashMap<String, String>,
}

impl ActorState {
    pub(super) fn load(conn: Connection) -> std::result::Result<ActorState, Error> {
        let settings = commands::settings::read_settings(&conn)?;
        let routing = RoutingCache::load(&conn)?;

        Ok(ActorState {
            conn,
            routing,
            settings,
            keys: HashMap::new(),
        })
    }

    fn ctx(&mut self) -> Ctx<'_> {
        Ctx {
            conn: &self.conn,
            settings: &mut self.settings,
            routing: &mut self.routing,
            keys: &mut self.keys,
        }
    }
}

pub(super) async fn actor(mut rx: mpsc::Receiver<Box<dyn Envelope>>, mut st: ActorState) {
    while let Some(envelope) = rx.recv().await {
        let mut cx = st.ctx();
        if envelope.handle(&mut cx).is_break() {
            break;
        }
    }
}
