use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use rusqlite::Connection;

use crate::pricing::Pricing;
use crate::provider::{Attempt, AttemptOutcome, Provider, ProviderMetrics};

use super::Error;
use super::sql::{
    attempts_for_load, batch_latest_balances, get_provider, get_provider_prices, list_pins,
    list_providers,
};

pub(in crate::db) struct ProviderEntry {
    pub(in crate::db) provider: Arc<Provider>,
    pub(in crate::db) prices: Arc<BTreeMap<String, Pricing>>,
    pub(in crate::db) metrics: ProviderMetrics,
    /// The raw deliveries the p50 is computed from: evidence, not a metric.
    latency_samples: VecDeque<u64>,
}

impl ProviderEntry {
    fn new(provider: Provider) -> Self {
        ProviderEntry {
            provider: Arc::new(provider),
            prices: Arc::new(BTreeMap::new()),
            metrics: ProviderMetrics::default(),
            latency_samples: VecDeque::new(),
        }
    }
}

#[derive(Default)]
pub(in crate::db) struct RoutingCache {
    pub(in crate::db) providers: HashMap<String, ProviderEntry>,
    pub(in crate::db) pins: HashMap<String, Vec<String>>,
    dirty: Dirty,
}

/// A slice of the derived state a write can make stale.
#[derive(Clone, Copy)]
pub(in crate::db) enum Stale {
    Providers,
    Pins,
    Prices,
}

impl Stale {
    fn bit(self) -> u8 {
        match self {
            Stale::Providers => 1,
            Stale::Pins => 2,
            Stale::Prices => 4,
        }
    }
}

/// One bit per slice: three flags do not need a map.
#[derive(Default, Clone, Copy)]
struct Dirty(u8);

impl Dirty {
    fn mark(&mut self, slice: Stale) {
        self.0 |= slice.bit();
    }

    fn clear(&mut self, slice: Stale) {
        self.0 &= !slice.bit();
    }

    fn is_set(self, slice: Stale) -> bool {
        self.0 & slice.bit() != 0
    }
}

/// The p50 is judged on recent deliveries, not on all history.
const P50_WINDOW: usize = 50;

impl RoutingCache {
    pub(in crate::db) fn load(conn: &Connection) -> std::result::Result<RoutingCache, Error> {
        let mut cache = RoutingCache::default();
        cache.mark(Stale::Providers);
        cache.mark(Stale::Pins);
        cache.mark(Stale::Prices);
        cache.refresh(conn)?;

        // The hot path and the load path share `observe`: loading replays every
        // retained observation in arrival order.
        let ids: Vec<&str> = cache.providers.keys().map(String::as_str).collect();
        for (id, bal) in batch_latest_balances(conn, &ids)? {
            if let Some(entry) = cache.providers.get_mut(&id) {
                entry.metrics.balance = Some(bal);
            }
        }
        for attempt in attempts_for_load(conn)? {
            if let Some(entry) = cache.providers.get_mut(&attempt.provider_id) {
                observe(entry, &attempt);
            }
        }

        Ok(cache)
    }

    /// Rebuild the slices a write marked — and only those (ADR-0006). A failed
    /// rebuild leaves its slice marked, so the next read retries.
    pub(in crate::db) fn refresh(&mut self, conn: &Connection) -> std::result::Result<(), Error> {
        self.when_stale(Stale::Providers, |st| {
            let db_providers = list_providers(conn)?;
            let live: HashSet<&str> = db_providers.iter().map(|p| p.id.as_str()).collect();
            st.providers.retain(|id, _| live.contains(id.as_str()));
            for p in db_providers {
                match st.providers.get_mut(&p.id) {
                    Some(entry) => entry.provider = Arc::new(p),
                    None => {
                        st.providers.insert(p.id.clone(), ProviderEntry::new(p));
                    }
                }
            }

            Ok(())
        })?;

        self.when_stale(Stale::Pins, |st| {
            st.pins = load_pins(conn)?;

            Ok(())
        })?;

        self.when_stale(Stale::Prices, |st| {
            for entry in st.providers.values_mut() {
                entry.prices = Arc::new(get_provider_prices(conn, &entry.provider.id)?);
            }
            Ok(())
        })
    }

    /// The provider's entry, loaded on demand: a write may arrive before the
    /// providers slice has ever been read.
    pub(in crate::db) fn ensure_entry(
        &mut self,
        conn: &Connection,
        id: &str,
    ) -> Option<&mut ProviderEntry> {
        if !self.providers.contains_key(id)
            && let Ok(Some(provider)) = get_provider(conn, id)
        {
            self.providers
                .insert(id.to_string(), ProviderEntry::new(provider));
        }

        self.providers.get_mut(id)
    }

    fn when_stale(
        &mut self,
        slice: Stale,
        f: impl FnOnce(&mut RoutingCache) -> std::result::Result<(), Error>,
    ) -> std::result::Result<(), Error> {
        if self.dirty.is_set(slice) {
            f(self)?;
            self.dirty.clear(slice);
        }

        Ok(())
    }

    pub(in crate::db) fn mark(&mut self, slice: Stale) {
        self.dirty.mark(slice);
    }
}

fn load_pins(conn: &Connection) -> std::result::Result<HashMap<String, Vec<String>>, Error> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for pin in list_pins(conn)? {
        map.entry(pin.app).or_default().push(pin.provider_id);
    }
    Ok(map)
}

/// Shared by the load path and the hot path so both derive it the same way.
pub(in crate::db) fn observe(entry: &mut ProviderEntry, attempt: &Attempt) {
    entry.metrics.last_attempt_at = attempt.at;

    match attempt.outcome {
        AttemptOutcome::Failed => entry.metrics.consecutive_failures += 1,
        _ => entry.metrics.consecutive_failures = 0,
    }

    if attempt.outcome == AttemptOutcome::Delivered {
        entry.latency_samples.push_back(attempt.latency);
        if entry.latency_samples.len() > P50_WINDOW {
            entry.latency_samples.pop_front();
        }
        entry.metrics.p50_latency = Some(compute_median(&entry.latency_samples));
    }
}

fn compute_median(latencies: &VecDeque<u64>) -> u32 {
    if latencies.is_empty() {
        return 0;
    }
    let mut sorted: Vec<u64> = latencies.iter().copied().collect();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2
    } else {
        sorted[mid]
    };
    median as u32
}
