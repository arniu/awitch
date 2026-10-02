use std::time::Duration;

use tokio_cron_scheduler::{Job, JobScheduler};

use crate::db::Store;

/// Event-driven task commands.
pub(crate) enum TaskEvent {
    SyncProvider(String),
    /// No-op placeholder for interface shaping.
    #[expect(dead_code)]
    Ping,
}

async fn sync_providers(store: &Store) -> anyhow::Result<()> {
    let providers = store.list_providers().await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    for p in &providers {
        if let Err(e) = sync_models(&client, store, p).await {
            tracing::error!(
                task = "provider-sync",
                provider = %p.id,
                "sync failed: {e}"
            );
        }
    }

    Ok(())
}

async fn sync_provider(store: &Store, id: &str) -> anyhow::Result<()> {
    let Some(p) = store.get_provider(id).await? else {
        return Ok(());
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    sync_models(&client, store, &p).await
}

async fn sync_models(
    client: &reqwest::Client,
    store: &Store,
    p: &crate::provider::Provider,
) -> anyhow::Result<()> {
    if p.models_url().is_none() {
        return Ok(());
    }

    let models = crate::provider_upstream::fetch_models(client, p).await?;
    store.set_provider_models(&p.id, models).await?;

    Ok(())
}

pub(crate) fn spawn_event_loop(store: Store, mut rx: tokio::sync::mpsc::Receiver<TaskEvent>) {
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            // Drain queued events of the same kind to coalesce.
            while rx.try_recv().is_ok() {}

            match event {
                TaskEvent::SyncProvider(id) => {
                    if let Err(e) = sync_provider(&store, &id).await {
                        tracing::error!(
                            task = "provider-sync",
                            provider = %id,
                            "sync failed: {e}"
                        );
                    }
                }

                TaskEvent::Ping => {
                    tracing::debug!("pong")
                }
            }
        }
        // Channel closed (sender dropped) — task exits cleanly.
    });
}

pub(crate) async fn spawn_timer(store: Store) -> anyhow::Result<JobScheduler> {
    let jobs = JobScheduler::new().await?;

    let _prune_ledger = store.clone();
    jobs.add(Job::new_repeated_async(
        Duration::from_secs(24 * 60 * 60),
        move |_uuid, _lock| {
            let store = _prune_ledger.clone();
            Box::pin(async move {
                if let Err(e) = store.prune_ledger().await {
                    tracing::error!(task = "ledger-prune", "scheduled task failed: {e}");
                }
            })
        },
    )?)
    .await?;

    let _prune_attempts = store.clone();
    jobs.add(Job::new_repeated_async(
        Duration::from_secs(24 * 60 * 60),
        move |_uuid, _lock| {
            let store = _prune_attempts.clone();
            Box::pin(async move {
                if let Err(e) = store.prune_attempts().await {
                    tracing::error!(task = "attempt-prune", "scheduled task failed: {e}");
                }
            })
        },
    )?)
    .await?;

    let _sync_providers = store.clone();
    jobs.add(Job::new_repeated_async(
        Duration::from_secs(24 * 60 * 60),
        move |_uuid, _lock| {
            let store = _sync_providers.clone();
            Box::pin(async move {
                if let Err(e) = sync_providers(&store).await {
                    tracing::error!(task = "providers-sync", "sync failed: {e}");
                }
            })
        },
    )?)
    .await?;

    Ok(jobs)
}
