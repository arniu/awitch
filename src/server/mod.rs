use std::sync::Arc;

use anyhow::Context;
use axum::body::Bytes;
use axum::response::Response;
use tokio::net::TcpListener;

use crate::db::Store;
use crate::protocol::Protocol;

use error::ServerError;

mod auth;
mod control;
mod control_token;
mod data;
mod error;
mod pid;
pub(crate) mod serve;
mod transport;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct Server {
    store: Store,
    control_token: String,
    transport: transport::ReqwestTransport,
    /// Sends events to the background task loop.
    pub(crate) task_tx: tokio::sync::mpsc::Sender<crate::tasks::TaskEvent>,
    /// Streamed-ledger recorders (spawned per streamed request). Held here so
    /// boot can drain them before the store stops — a row whose body ended
    /// during connection drain must still land (ADR-0006).
    stream_tasks: Arc<StreamTasks>,
}

#[derive(Default)]
pub(crate) struct StreamTasks {
    tasks: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl StreamTasks {
    pub(crate) fn spawn<F>(&self, task: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.retain(|task| !task.is_finished());
        tasks.push(tokio::spawn(task));
    }

    /// Wait for every recorder spawned so far.
    pub(crate) async fn drain(&self) {
        let tasks: Vec<_> = std::mem::take(&mut *self.tasks.lock().unwrap());
        for task in tasks {
            let _ = task.await;
        }
    }
}

impl Server {
    pub(crate) fn new(
        store: Store,
        control_token: String,
        task_tx: tokio::sync::mpsc::Sender<crate::tasks::TaskEvent>,
    ) -> Server {
        Server {
            store,
            control_token,
            transport: transport::ReqwestTransport::new(),
            task_tx,
            stream_tasks: Arc::new(StreamTasks::default()),
        }
    }
}

/// The composition root wires the server's own types together. The store the
/// pipeline consults and writes lives here — `db` never depends on
/// `protocol`.
impl Server {
    pub async fn serve(config_dir: &std::path::Path) -> anyhow::Result<()> {
        crate::utils::ensure_private_dir(config_dir)?;
        // Taken BEFORE opening the DB: a second instance must fail before it
        // ever touches the SQLite file (single-writer invariant).
        let _lock = crate::utils::lock_file(&config_dir.join("server.lock"))?;

        let db_path = config_dir.join("data.db");
        let store =
            Store::open(&db_path).with_context(|| format!("open store {}", db_path.display()))?;

        let config = crate::config::Config::load(config_dir)?;
        let token = control_token::read_or_create(&config_dir.join("control.token"))?;
        let (task_tx, task_rx) = tokio::sync::mpsc::channel(1);
        let server = Server::new(store.clone(), token, task_tx);

        let data_addr = format!("{}:{}", config.host(), config.port());
        let data_listener = TcpListener::bind(&data_addr)
            .await
            .with_context(|| format!("bind {}", data_addr))?;

        let control_addr = format!("{}:{}", config.control_host(), config.control_port());
        let control_listener = TcpListener::bind(&control_addr)
            .await
            .with_context(|| format!("bind control {}", control_addr))?;

        let _pid = pid::PidFile::write(&config_dir.join("server.pid"))?;

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        tokio::spawn(shutdown_on_signal(shutdown_tx));

        crate::tasks::spawn_event_loop(store.clone(), task_rx);
        let mut jobs = crate::tasks::spawn_timer(store.clone()).await?;
        jobs.start().await?;

        // stream recorders are drained below after the connections drain; the
        // tracker outlives `server` (each router's state holds a clone).
        let stream_tasks = server.stream_tasks.clone();

        let data_app = data::router(&server);
        let mut data_rx = shutdown_rx.clone();
        let data_handle = tokio::spawn(async move {
            axum::serve(data_listener, data_app)
                .with_graceful_shutdown(async move {
                    let _ = data_rx.changed().await;
                })
                .await
        });

        let control_app = control::router(&server);
        let mut control_rx = shutdown_rx.clone();
        let control_handle = tokio::spawn(async move {
            axum::serve(control_listener, control_app)
                .with_graceful_shutdown(async move {
                    let _ = control_rx.changed().await;
                })
                .await
        });

        // Both listeners are now actually accepting and the scheduler runs.
        tracing::info!("awitch server is running");
        tracing::info!("  data:    http://{}", data_addr);
        tracing::info!("  control: http://{}", control_addr);

        let _ = data_handle.await;
        let _ = control_handle.await;

        // In-flight connections already drained: axum's serve futures returned
        // only once graceful shutdown finished them. Wait for the stream
        // recorders those connections may have fired, then flush queued writes
        // and stop the store — a row is never dropped for arriving a moment
        // late (ADR-0006).
        stream_tasks.drain().await;
        let _ = jobs.shutdown().await;
        store.shutdown().await;

        Ok(())
    }
}

async fn shutdown_on_signal(tx: tokio::sync::watch::Sender<bool>) {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
        match &mut term {
            Ok(term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                tokio::signal::ctrl_c().await.ok();
            }
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.ok();
    }
    let _ = tx.send(true);
}

impl Server {
    /// Auth is handled by the data plane's `Identity` extractor before this runs.
    pub(crate) async fn execute(
        &self,
        app: &str,
        body: Bytes,
        protocol: Protocol,
    ) -> Result<Response, ServerError> {
        serve::process(self, app, body, protocol).await
    }

    pub(crate) fn http_client(&self) -> &reqwest::Client {
        self.transport.http_client()
    }
}
