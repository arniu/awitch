use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

use crate::pricing::{Price, Pricing};
use crate::protocol::Protocol;
use crate::provider::{Provider, ProviderMetrics};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    #[default]
    Eco,
    Speed,
    Balanced,
}

impl RoutingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RoutingMode::Eco => "eco",
            RoutingMode::Speed => "speed",
            RoutingMode::Balanced => "balanced",
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown routing mode {0:?}")]
pub struct UnknownModeError(pub String);

impl std::str::FromStr for RoutingMode {
    type Err = UnknownModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "eco" => Ok(RoutingMode::Eco),
            "speed" => Ok(RoutingMode::Speed),
            "balanced" => Ok(RoutingMode::Balanced),
            other => Err(UnknownModeError(other.to_string())),
        }
    }
}

impl std::fmt::Display for RoutingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("no route for protocol {protocol} model '{model}': {reasons}")]
pub struct NoRouteError {
    pub protocol: Protocol,
    pub model: String,
    pub reasons: String,
}

/// Provider + its pricing cards + inlined health/balance metrics for routing.
/// Both heavy parts are shared with the store's cache, so a request's snapshot
/// is pointer copies, not a deep clone of the pool.
#[derive(Debug, Clone)]
pub struct RoutingProvider {
    pub provider: Arc<Provider>,
    /// model id → pricing card, from the `prices` table — routing facts kept
    /// apart from the catalog so a catalog sync can't overwrite them (ADR-0008).
    pub prices: Arc<BTreeMap<String, Pricing>>,
    pub metrics: ProviderMetrics,
}

/// The request, after protocol-level preprocessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteRequest {
    pub protocol: Protocol,
    /// Requested model — a literal model id (identity match).
    pub model: String,
    /// UTC time-of-day for pricing lookup.
    pub now: NaiveTime,
    /// Provider this request is forced to route to, if any (ADR-0009).
    pub forced_provider: Option<String>,
    /// The request continues provider-side state (openai responses
    /// `previous_response_id` / `conversation`). Its context lives at the
    /// provider that created it, so only a native match can serve it
    /// (ADR-0009) — translation has no form to carry it.
    pub continuation: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub provider: String,
    pub protocol: Protocol,
    pub model: String,
    pub latency: Option<u32>,
    pub price: Option<Price>,
}

/// Routing inputs, batched and cached by the store (ADR-0006): the pool, its
/// observed metrics, the pricing cards, and the stored routing settings.
#[derive(Debug, Clone, Default)]
pub struct RoutingInput {
    pub providers: Vec<RoutingProvider>,
    /// The routing mode — typed by the settings field, defaulted there.
    pub mode: RoutingMode,
    /// Pinned candidate providers (ADR-0009): the app may route only within
    /// this set. Empty = auto routing. 0/1/many rows in pins.
    pub pinned_providers: Vec<String>,
    /// The balance threshold — typed by the settings field, defaulted there.
    pub balance_threshold: money::Amount,
    /// Consecutive failures that eject the provider from the pool.
    pub failure_threshold: u32,
    /// The cooldown (a gateway setting): a provider quiet past it has spent its
    /// streak and returns to the pool on its own.
    pub cooldown_secs: i64,
    /// When this snapshot was created. Ages are judged against it, so routing
    /// never reads a clock.
    pub created_at: i64,
}

/// Whether the provider is ejected from the pool (ADR-0009): its consecutive
/// failures reached the threshold, and it has not been quiet long enough for
/// the streak to be spent. This is the only place the cooldown is applied.
pub fn ejected(
    metrics: &ProviderMetrics,
    now: i64,
    failure_threshold: u32,
    cooldown_secs: i64,
) -> bool {
    let spent =
        metrics.last_attempt_at > 0 && now.saturating_sub(metrics.last_attempt_at) > cooldown_secs;
    !spent && metrics.consecutive_failures >= failure_threshold
}

pub fn ranked_routes(req: &RouteRequest, input: &RoutingInput) -> Result<Vec<Route>, NoRouteError> {
    // A continuation names its provider: the state lives there alone, so there
    // is nothing to select — and nothing to translate (ADR-0009).
    if let Some(id) = req.forced_provider.as_deref() {
        return forced_route(req, input, id);
    }

    let mode = input.mode;
    let balance_threshold = input.balance_threshold;

    let mut candidates: Vec<Route> = Vec::new();
    let mut reasons: Vec<String> = Vec::new();

    for rp in &input.providers {
        let provider = &rp.provider;
        let metrics = &rp.metrics;

        if !input.pinned_providers.is_empty() && !input.pinned_providers.contains(&provider.id) {
            continue;
        }

        let Some(protocol) = served_protocol(provider, req.protocol, req.continuation) else {
            reasons.push(if req.continuation {
                format!(
                    "{}: continuation requires native {}",
                    provider.id, req.protocol
                )
            } else {
                format!("{}: protocol {} not servable", provider.id, req.protocol)
            });

            continue;
        };

        if ejected(
            metrics,
            input.created_at,
            input.failure_threshold,
            input.cooldown_secs,
        ) {
            reasons.push(format!(
                "{}: ejected ({} consecutive failures)",
                provider.id, input.failure_threshold
            ));
            continue;
        }

        let below_balance = metrics
            .balance
            .as_ref()
            .is_some_and(|b| b.currency == money::Currency::Usd && b.amount < balance_threshold);
        if below_balance && !has_quota_free_model(rp) {
            reasons.push(format!(
                "{}: balance below threshold {} and no quota-free model",
                provider.id, balance_threshold
            ));

            continue;
        }

        let mut emitted = false;
        for model in &provider.models {
            if model.id != req.model {
                continue;
            }

            let price = rp
                .prices
                .get(&model.id)
                .and_then(|card| card.effective_price(req.now));
            candidates.push(Route {
                provider: provider.id.clone(),
                model: model.id.clone(),
                price,
                latency: metrics.p50_latency,
                protocol,
            });

            emitted = true;
        }

        if !emitted {
            reasons.push(format!(
                "{}: no model matching '{}'",
                provider.id, req.model
            ));
        }
    }

    if candidates.is_empty() {
        let reasons = if !reasons.is_empty() {
            reasons.join("; ")
        } else if input.pinned_providers.is_empty() {
            "no provider in pool".to_string()
        } else {
            format!(
                "pinned providers {:?} ineligible or missing",
                input.pinned_providers
            )
        };
        return Err(NoRouteError {
            protocol: req.protocol,
            model: req.model.clone(),
            reasons,
        });
    }

    rank(&mut candidates, mode);

    Ok(candidates)
}

/// A continuation names its provider: the request goes straight there, served
/// natively. Selection (ranking, pins, health, balance, model match) exists to
/// *choose* among candidates, and there is nothing to choose here; translation
/// has no form that carries the continuation's state. The chosen provider is
/// the only one that can fail the request (ADR-0009).
fn forced_route(
    req: &RouteRequest,
    input: &RoutingInput,
    id: &str,
) -> Result<Vec<Route>, NoRouteError> {
    let missing = |reason: String| NoRouteError {
        protocol: req.protocol,
        model: req.model.clone(),
        reasons: reason,
    };

    let Some(rp) = input.providers.iter().find(|rp| rp.provider.id == id) else {
        return Err(missing(format!(
            "continuation provider '{id}' is not in the pool"
        )));
    };
    if !rp.provider.protocols.contains_key(&req.protocol) {
        return Err(missing(format!(
            "continuation provider '{id}' does not serve {} natively",
            req.protocol
        )));
    }

    Ok(vec![Route {
        provider: id.to_string(),
        model: req.model.clone(),
        price: rp
            .prices
            .get(&req.model)
            .and_then(|card| card.effective_price(req.now)),
        latency: rp.metrics.p50_latency,
        protocol: req.protocol,
    }])
}

/// The protocol a provider serves `requested` through: itself natively, or
/// openai chat by translation. A continuation is native-only — its state lives
/// at the provider that created it, and no translated request carries it.
fn served_protocol(p: &Provider, requested: Protocol, continuation: bool) -> Option<Protocol> {
    if continuation {
        return p.protocols.contains_key(&requested).then_some(requested);
    }

    [requested, Protocol::OpenaiChat]
        .iter()
        .find(|it| p.protocols.contains_key(it))
        .copied()
}

fn has_quota_free_model(rp: &RoutingProvider) -> bool {
    // Provider-level gate: ANY quota-free model keeps the provider eligible
    // below the balance threshold. A quota-free model prices at 0, so it
    // outranks the provider's metered models on cost — the metered ones are
    // rarely selected while the quota lasts (acceptable edge).
    rp.provider.models.iter().any(|m| {
        rp.prices
            .get(&m.id)
            .is_some_and(Pricing::has_quota_remaining)
    })
}

// FIXME: expected token mix is a fixed stand-in (19:1 input:output) — wire
// the ledger's per-app token ratio into routing when available
// (ADR-0009: Price × Usage). Coding-agent production telemetry: input is
// 93–99% of tokens (OpenRouter 23:1, Cursor 13:1, Claude Code 166:1).
const EXPECTED_INPUT_SHARE: f64 = 0.95;
const EXPECTED_OUTPUT_SHARE: f64 = 0.05;

fn estimated_cost(price: Price) -> f64 {
    price.input.to_f64() * EXPECTED_INPUT_SHARE + price.output.to_f64() * EXPECTED_OUTPUT_SHARE
}

fn rank(cands: &mut [Route], mode: RoutingMode) {
    let min_p = cands
        .iter()
        .filter_map(|c| c.price.map(estimated_cost))
        .fold(f64::INFINITY, f64::min);
    let max_p = cands
        .iter()
        .filter_map(|c| c.price.map(estimated_cost))
        .fold(f64::NEG_INFINITY, f64::max);
    let min_l = cands
        .iter()
        .filter_map(|c| c.latency)
        .fold(u32::MAX, u32::min);
    let max_l = cands.iter().filter_map(|c| c.latency).fold(0, u32::max);
    let lam = lambda(mode);
    cands.sort_by(|a, b| {
        score(a, min_p, max_p, min_l, max_l, lam)
            .total_cmp(&score(b, min_p, max_p, min_l, max_l, lam))
            // Equal scores must resolve the same way in every process: the pool
            // is iterated out of a map, so its order is not stable.
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.model.cmp(&b.model))
    });
}

fn lambda(mode: RoutingMode) -> f64 {
    match mode {
        RoutingMode::Eco => 0.8,
        RoutingMode::Balanced => 0.5,
        RoutingMode::Speed => 0.2,
    }
}

fn score(c: &Route, min_p: f64, max_p: f64, min_l: u32, max_l: u32, lambda: f64) -> f64 {
    let norm = |x: f64, min: f64, max: f64| {
        if max <= min {
            0.5
        } else {
            (x - min) / (max - min)
        }
    };

    let price_part = c
        .price
        .map(estimated_cost)
        .map_or(0.5, |p| norm(p, min_p, max_p));
    let lat_part = c
        .latency
        .map_or(0.5, |l| norm(l as f64, min_l as f64, max_l as f64));
    lambda * price_part + (1.0 - lambda) * lat_part
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::provider::Endpoint;
    use money::Currency;
    use std::collections::BTreeMap;

    #[test]
    fn mode_round_trip() {
        for (mode, expected) in [
            (RoutingMode::Eco, "eco"),
            (RoutingMode::Speed, "speed"),
            (RoutingMode::Balanced, "balanced"),
        ] {
            assert_eq!(mode.as_str(), expected);
            assert_eq!(format!("{mode}"), expected);
            assert_eq!(expected.parse::<RoutingMode>().unwrap(), mode);
        }
    }

    fn amt(s: &str) -> money::Amount {
        s.parse().unwrap()
    }

    fn hm(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn rp(id: &str, protocols: &[&str], model_ids: &[&str]) -> RoutingProvider {
        rp_metrics(id, protocols, model_ids, metrics(None, 0, None))
    }

    fn rp_metrics(
        id: &str,
        protocols: &[&str],
        model_ids: &[&str],
        metrics: ProviderMetrics,
    ) -> RoutingProvider {
        let caps: BTreeMap<Protocol, Endpoint> = protocols
            .iter()
            .map(|p| (p.parse::<Protocol>().unwrap(), Endpoint::Canonical))
            .collect();
        RoutingProvider {
            prices: Arc::new(BTreeMap::new()),
            provider: Arc::new(serde_json::from_value(serde_json::json!({
                "id": id,
                "protocols": caps,
                "base_url": format!("https://{id}.example.com"),
                "key": "k",
                "models": model_ids.iter().map(|m| serde_json::json!({"id": m})).collect::<Vec<_>>(),
            }))
            .unwrap()),
            metrics,
        }
    }

    fn top(req: &RouteRequest, input: &RoutingInput) -> Route {
        ranked_routes(req, input).unwrap().remove(0)
    }

    fn req(model: &str) -> RouteRequest {
        RouteRequest {
            protocol: Protocol::Anthropic,
            model: model.into(),
            now: hm(12, 0),
            forced_provider: None,
            continuation: false,
        }
    }

    fn base_input(providers: Vec<RoutingProvider>) -> RoutingInput {
        RoutingInput {
            providers,
            mode: RoutingMode::Eco,
            balance_threshold: amt("1"),
            failure_threshold: 5,
            ..Default::default()
        }
    }

    fn prices_of<'a>(input: &'a mut RoutingInput, id: &str) -> &'a mut BTreeMap<String, Pricing> {
        let rp = input
            .providers
            .iter_mut()
            .find(|rp| rp.provider.id == id)
            .unwrap();
        Arc::make_mut(&mut rp.prices)
    }

    fn attach(mut input: RoutingInput, cards: &[(&str, &str, Pricing)]) -> RoutingInput {
        for (provider, model, card) in cards {
            prices_of(&mut input, provider).insert((*model).to_string(), card.clone());
        }
        input
    }

    fn set_metrics(input: &mut RoutingInput, id: &str, metrics: ProviderMetrics) {
        input
            .providers
            .iter_mut()
            .find(|rp| rp.provider.id == id)
            .unwrap()
            .metrics = metrics;
    }

    fn flat(input: &str, output: &str) -> Pricing {
        Pricing::MeteredFlat {
            price: Price {
                input: amt(input),
                output: amt(output),
            },
        }
    }

    fn timed() -> Pricing {
        Pricing::MeteredTimed {
            tiers: vec![
                crate::pricing::PriceTier {
                    window: "09:00-23:00".into(),
                    price: Price {
                        input: amt("2"),
                        output: amt("8"),
                    },
                },
                crate::pricing::PriceTier {
                    window: "23:00-09:00".into(),
                    price: Price {
                        input: amt("1"),
                        output: amt("4"),
                    },
                },
            ],
        }
    }

    fn quota(remaining: &str, fallback: Option<(&str, &str)>) -> Pricing {
        Pricing::QuotaPlan {
            plan: "coding_plan".into(),
            period: crate::pricing::QuotaPeriod::Day,
            remaining: Some(amt(remaining)),
            fallback: fallback.map(|(input, output)| Price {
                input: amt(input),
                output: amt(output),
            }),
        }
    }

    fn metrics(balance: Option<money::Amount>, failures: u32, p50: Option<u32>) -> ProviderMetrics {
        ProviderMetrics {
            balance: balance.map(|amount| money::Money {
                amount,
                currency: Currency::Usd,
            }),
            consecutive_failures: failures,
            p50_latency: p50,
            last_attempt_at: 0,
        }
    }

    /// Equally ranked candidates must not depend on the order the pool was
    /// iterated in, or the winner changes between runs. Every field here is
    /// neutral (no price, no latency), so all candidates tie.
    #[test]
    fn ties_resolve_by_id_not_by_input_order() {
        let ranked = |providers: Vec<RoutingProvider>| {
            ranked_routes(&req("m"), &base_input(providers))
                .unwrap()
                .into_iter()
                .map(|r| r.provider)
                .collect::<Vec<_>>()
        };
        let one = ranked(vec![
            rp("a", &["anthropic"], &["m"]),
            rp("b", &["anthropic"], &["m"]),
        ]);
        let other = ranked(vec![
            rp("b", &["anthropic"], &["m"]),
            rp("a", &["anthropic"], &["m"]),
        ]);

        assert_eq!(one, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(one, other);
    }

    #[test]
    fn ranked_routes_returns_full_sorted_list() {
        let cheap = rp("cheap", &["anthropic"], &["m"]);
        let mid = rp("mid", &["anthropic"], &["m"]);
        let pricey = rp("pricey", &["anthropic"], &["m"]);
        let input = attach(
            base_input(vec![pricey, cheap, mid]),
            &[
                ("cheap", "m", flat("1.0", "1.0")),
                ("mid", "m", flat("3.0", "3.0")),
                ("pricey", "m", flat("5.0", "5.0")),
            ],
        );
        let ranked = ranked_routes(&req("m"), &input).unwrap();
        assert_eq!(
            ranked
                .iter()
                .map(|r| r.provider.as_str())
                .collect::<Vec<_>>(),
            vec!["cheap", "mid", "pricey"]
        );
    }

    #[test]
    fn time_aware_off_peak_beats_flat() {
        let t = rp("t", &["anthropic"], &["m"]);
        let flat_expensive = rp("f", &["anthropic"], &["m"]);
        let input = attach(
            base_input(vec![t, flat_expensive]),
            &[("t", "m", timed()), ("f", "m", flat("9.0", "9.0"))],
        );
        // off-peak: timed ≈1.15 < flat 9 → t
        let mut r = req("m");
        r.now = hm(0, 30);
        assert_eq!(top(&r, &input).provider, "t");
        // peak: timed ≈2.3 < flat 9 → still t
        r.now = hm(12, 0);
        assert_eq!(top(&r, &input).provider, "t");
    }

    #[test]
    fn unhealthy_provider_excluded() {
        let a = rp("a", &["anthropic"], &["m"]);
        let b = rp("b", &["anthropic"], &["m"]);
        let mut input = attach(
            base_input(vec![a, b]),
            &[
                ("a", "m", flat("1.0", "1.0")),
                ("b", "m", flat("2.0", "2.0")),
            ],
        );
        set_metrics(&mut input, "a", metrics(None, 5, None));
        let out = top(&req("m"), &input);
        assert_eq!(out.provider, "b");
        // below the limit → still eligible
        set_metrics(&mut input, "a", metrics(None, 4, None));
        assert_eq!(top(&req("m"), &input).provider, "a");
    }

    #[test]
    fn balance_below_threshold_excluded_unless_quota_free() {
        let a = rp("a", &["anthropic"], &["m"]);
        let b = rp("b", &["anthropic"], &["m"]);
        let quota_prov = rp("q", &["anthropic"], &["m"]);
        let mut input = attach(
            base_input(vec![a, b, quota_prov]),
            &[
                ("a", "m", flat("1.0", "1.0")),
                ("b", "m", flat("3.0", "3.0")),
                ("q", "m", quota("1000.0", Some(("5", "5")))),
            ],
        );
        for (id, bal) in [("a", "10"), ("b", "10"), ("q", "0.5")] {
            let bal: money::Amount = bal.parse().unwrap();
            set_metrics(&mut input, id, metrics(Some(bal), 0, None));
        }
        // q is below the balance threshold but has quota-free models → stays
        // eligible, and its effective price is 0 → wins on cost.
        assert_eq!(top(&req("m"), &input).provider, "q");
        // exhaust q's quota (fallback 5+5=10) → q is now below threshold AND
        // has no quota-free model → excluded; cheapest eligible is a (2).
        prices_of(&mut input, "q").insert("m".into(), quota("0.0", Some(("5", "5"))));
        assert_eq!(top(&req("m"), &input).provider, "a");
    }

    #[test]
    fn non_usd_balance_not_compared_against_usd_threshold() {
        let usd_low = rp("usd-low", &["anthropic"], &["m"]);
        let cny_low = rp_metrics(
            "cny-low",
            &["anthropic"],
            &["m"],
            ProviderMetrics {
                balance: Some(money::Money {
                    amount: amt("0.5"),
                    currency: Currency::Cny,
                }),
                consecutive_failures: 0,
                p50_latency: None,
                last_attempt_at: 0,
            },
        );
        let mut input = attach(
            base_input(vec![usd_low, cny_low]),
            &[
                ("usd-low", "m", flat("1.0", "1.0")),
                ("cny-low", "m", flat("1.0", "1.0")),
            ],
        );
        set_metrics(&mut input, "usd-low", metrics(Some(amt("0.5")), 0, None));
        assert_eq!(top(&req("m"), &input).provider, "cny-low");
    }

    /// The cooldown is the only place a quiet provider returns to the pool, and
    /// the edge is strict: exactly the cooldown is still ejected.
    #[test]
    fn a_quiet_streak_stops_ejecting() {
        let never = metrics(None, 5, None);
        assert!(ejected(&never, 1_000, 5, 600));

        let mut quiet = never;
        quiet.last_attempt_at = 1_000;
        assert!(ejected(&quiet, 1_600, 5, 600));
        assert!(!ejected(&quiet, 1_601, 5, 600));
    }

    #[test]
    fn quiet_provider_returns_to_the_pool() {
        let stale = rp_metrics(
            "stale",
            &["anthropic"],
            &["m"],
            ProviderMetrics {
                consecutive_failures: 5,
                last_attempt_at: 1_000,
                ..Default::default()
            },
        );
        let fresh = rp("fresh", &["anthropic"], &["m"]);
        let mut input = base_input(vec![stale, fresh]);
        input.cooldown_secs = 600;

        input.created_at = 1_600;
        let ids: Vec<String> = ranked_routes(&req("m"), &input)
            .unwrap()
            .into_iter()
            .map(|route| route.provider)
            .collect();
        assert_eq!(ids, vec!["fresh".to_string()]);

        input.created_at = 1_601;
        let ids: Vec<String> = ranked_routes(&req("m"), &input)
            .unwrap()
            .into_iter()
            .map(|route| route.provider)
            .collect();
        assert!(ids.contains(&"stale".to_string()), "{ids:?}");
    }

    #[test]
    fn speed_mode_ranks_by_latency() {
        let a = rp("a", &["anthropic"], &["m"]);
        let b = rp("b", &["anthropic"], &["m"]);
        let mut input = attach(
            base_input(vec![a, b]),
            &[
                ("a", "m", flat("1.0", "1.0")),
                ("b", "m", flat("1.0", "1.0")),
            ],
        );
        input.mode = RoutingMode::Speed;
        set_metrics(&mut input, "a", metrics(None, 0, Some(800)));
        set_metrics(&mut input, "b", metrics(None, 0, Some(200)));
        assert_eq!(top(&req("m"), &input).provider, "b");
    }

    #[test]
    fn cold_candidate_competes_by_latency() {
        let priced_slow = rp("slow", &["anthropic"], &["m"]);
        let cold_fast = rp("fast", &["anthropic"], &["m"]);
        let mut input = attach(
            base_input(vec![priced_slow, cold_fast]),
            &[("slow", "m", flat("1.0", "1.0"))],
        );
        set_metrics(&mut input, "slow", metrics(None, 0, Some(5000)));
        set_metrics(&mut input, "fast", metrics(None, 0, Some(100)));
        let out = top(&req("m"), &input);
        assert_eq!(out.provider, "fast");
        assert_eq!(out.price, None);
    }

    #[test]
    fn literal_model_matches_exact_id() {
        let a = rp("a", &["anthropic"], &["deepseek-chat"]);
        let b = rp("b", &["anthropic"], &["other"]);
        let input = attach(
            base_input(vec![a, b]),
            &[
                ("a", "deepseek-chat", flat("1.0", "1.0")),
                ("b", "other", flat("0.5", "0.5")),
            ],
        );
        let out = top(&req("deepseek-chat"), &input);
        assert_eq!(out.provider, "a");
    }

    #[test]
    fn forced_provider_forces_provider() {
        let a = rp("a", &["anthropic"], &["m"]);
        let b = rp("b", &["anthropic"], &["m"]);
        let input = attach(
            base_input(vec![a, b]),
            &[
                ("a", "m", flat("1.0", "1.0")),
                ("b", "m", flat("0.5", "0.5")),
            ],
        );
        let mut r = req("m");
        r.forced_provider = Some("a".into());
        let out = top(&r, &input);
        assert_eq!(out.provider, "a");
        // no forced provider → normal routing (b is cheapest)
        r.forced_provider = None;
        assert_eq!(top(&r, &input).provider, "b");
    }

    /// A continuation goes straight to its provider: pins, health, balance and
    /// the model catalog are selection concerns and cannot redirect it
    /// (ADR-0009).
    #[test]
    fn forced_provider_skips_selection() {
        let a = rp("a", &["anthropic"], &["other"]);
        let b = rp("b", &["anthropic"], &["m"]);
        let mut input = base_input(vec![a, b]);
        // a is unhealthy, below the balance threshold, and lacks the model
        set_metrics(&mut input, "a", metrics(Some(amt("0")), 9, None));
        input.pinned_providers = vec!["b".to_string()];

        let mut r = req("m");
        r.forced_provider = Some("a".into());
        let out = top(&r, &input);
        assert_eq!(out.provider, "a");
        assert_eq!(out.model, "m");
    }

    /// The provider a continuation names must speak the protocol natively:
    /// translation has no form that carries the state.
    #[test]
    fn forced_provider_must_serve_natively() {
        let chat = rp("chat", &["openai_chat"], &["m"]);
        let input = base_input(vec![chat]);

        let mut r = req("m");
        r.protocol = Protocol::OpenaiResponses;
        r.forced_provider = Some("chat".into());
        let err = ranked_routes(&r, &input).unwrap_err();
        assert!(err.to_string().contains("natively"), "err = {err}");
    }

    /// A continuation whose provider left the pool is unservable — never
    /// silently re-routed.
    #[test]
    fn forced_provider_missing_from_pool_is_unservable() {
        let a = rp("a", &["anthropic"], &["m"]);
        let input = base_input(vec![a]);

        let mut r = req("m");
        r.forced_provider = Some("ghost".into());
        let err = ranked_routes(&r, &input).unwrap_err();
        assert!(err.to_string().contains("not in the pool"), "err = {err}");
    }

    #[test]
    fn no_route_reports_reasons() {
        let b = rp("b", &["openai_chat"], &["m"]);
        let mut input = base_input(vec![b]);
        // claude requests anthropic; openai chat serves via translation → ok.
        // force exclusion: unhealthy
        set_metrics(&mut input, "b", metrics(None, 9, None));
        let err = ranked_routes(&req("m"), &input).unwrap_err();
        assert!(err.to_string().contains("ejected"), "err = {err}");
    }

    #[test]
    fn no_route_reports_protocol_and_model() {
        let a = rp("a", &["openai_chat"], &["m"]);
        let input = base_input(vec![a]);
        // claude requests anthropic (servable via translation), but no model
        // matches the literal id
        let err = ranked_routes(&req("nonexistent-model"), &input).unwrap_err();
        assert!(err.to_string().contains("no model matching"), "err = {err}");
        assert!(err.to_string().contains("anthropic"), "err = {err}");
    }

    #[test]
    fn pin_set_of_many_filters_to_the_set() {
        // 1+ cardinality: app pinned to {a, b} — c is excluded even if cheapest.
        let a = rp("a", &["anthropic"], &["m"]);
        let b = rp("b", &["anthropic"], &["m"]);
        let c = rp("c", &["anthropic"], &["m"]);
        let mut input = attach(
            base_input(vec![a, b, c]),
            &[
                ("a", "m", flat("1.0", "1.0")),
                ("b", "m", flat("2.0", "2.0")),
                ("c", "m", flat("0.1", "0.1")),
            ],
        );
        input.pinned_providers = vec!["a".to_string(), "b".to_string()];
        let out = top(&req("m"), &input);
        assert_eq!(out.provider, "a"); // eco: cheapest inside the set
        // 0 cardinality: auto routing, cheapest overall (c) wins
        input.pinned_providers = Vec::new();
        assert_eq!(top(&req("m"), &input).provider, "c");
    }

    #[test]
    fn no_route_reports_pin_issue() {
        let a = rp("a", &["anthropic"], &["m"]);
        let mut input = base_input(vec![a]);
        // pin to a provider that is unhealthy
        input.pinned_providers = vec!["a".to_string()];
        set_metrics(&mut input, "a", metrics(None, 9, None));
        let err = ranked_routes(&req("m"), &input).unwrap_err();
        assert!(err.to_string().contains("a"), "err = {err}");
        assert!(err.to_string().contains("ejected"), "err = {err}");
        // pin to a provider not in the pool at all
        input.providers.clear();
        input.pinned_providers = vec!["ghost".to_string()];
        let err = ranked_routes(&req("m"), &input).unwrap_err();
        assert!(err.to_string().contains("pinned provider"), "err = {err}");
    }

    #[test]
    fn balanced_mode_uses_normalized_score() {
        let a = rp("a", &["anthropic"], &["m"]);
        let c = rp("c", &["anthropic"], &["m"]);
        let b = rp("b", &["anthropic"], &["m"]);
        let mut input = attach(
            base_input(vec![a, c, b]),
            &[
                ("a", "m", flat("9.0", "9.0")),
                ("c", "m", flat("4.0", "4.0")),
                ("b", "m", flat("1.0", "1.0")),
            ],
        );
        input.mode = RoutingMode::Balanced;
        set_metrics(&mut input, "a", metrics(None, 0, Some(100)));
        set_metrics(&mut input, "c", metrics(None, 0, Some(500)));
        set_metrics(&mut input, "b", metrics(None, 0, Some(900)));
        let out = top(&req("m"), &input);
        assert_eq!(out.provider, "c");
    }

    #[test]
    fn served_protocol_native_or_translated_to_chat() {
        let chat = rp("chat", &["openai_chat"], &[]);
        let anthropic = rp("anth", &["anthropic"], &[]);
        let responses = rp("resp", &["openai_responses"], &[]);

        // anthropic/responses: themselves, or translated to chat; no other target
        for (requested, native) in [
            (Protocol::Anthropic, &anthropic),
            (Protocol::OpenaiResponses, &responses),
        ] {
            assert_eq!(
                served_protocol(&native.provider, requested, false),
                Some(requested)
            );
            assert_eq!(
                served_protocol(&chat.provider, requested, false),
                Some(Protocol::OpenaiChat)
            );
        }
        assert_eq!(
            served_protocol(&responses.provider, Protocol::Anthropic, false),
            None
        );
        assert_eq!(
            served_protocol(&anthropic.provider, Protocol::OpenaiResponses, false),
            None
        );

        // chat has no translation — native only
        assert_eq!(
            served_protocol(&chat.provider, Protocol::OpenaiChat, false),
            Some(Protocol::OpenaiChat)
        );
        assert_eq!(
            served_protocol(&anthropic.provider, Protocol::OpenaiChat, false),
            None
        );
        assert_eq!(
            served_protocol(&responses.provider, Protocol::OpenaiChat, false),
            None
        );

        // a continuation cannot be translated: native only, whatever the pool
        assert_eq!(
            served_protocol(&responses.provider, Protocol::OpenaiResponses, true),
            Some(Protocol::OpenaiResponses)
        );
        assert_eq!(
            served_protocol(&chat.provider, Protocol::OpenaiResponses, true),
            None
        );
    }

    /// A continuation is servable only by a provider that speaks the protocol
    /// natively: translation would send the upstream a turn stripped of its
    /// context (ADR-0009).
    #[test]
    fn continuation_is_served_natively_only() {
        let chat = rp("chat", &["openai_chat"], &["m"]);
        let native = rp("native", &["openai_responses"], &["m"]);
        let mut input = base_input(vec![chat, native]);

        let mut r = req("m");
        r.protocol = Protocol::OpenaiResponses;
        r.continuation = true;
        let ranked = ranked_routes(&r, &input).unwrap();
        assert_eq!(
            ranked
                .iter()
                .map(|c| c.provider.as_str())
                .collect::<Vec<_>>(),
            vec!["native"]
        );

        // no native provider left → unservable, and the reason says why
        input.providers.retain(|rp| rp.provider.id != "native");
        let err = ranked_routes(&r, &input).unwrap_err();
        assert!(err.to_string().contains("continuation"), "err = {err}");
    }

    #[test]
    fn route_filter_unhealthy_and_uses_price_cards() {
        // The same decision the DB snapshot feeds, built as pure routing input:
        // a healthy anthropic provider beats an unhealthy (5 consecutive
        // failures) chat-only one.
        let cheap = rp("a", &["anthropic"], &["m"]);
        let chat_only = rp("b", &["openai_chat"], &["m"]);
        let mut input = attach(
            base_input(vec![cheap, chat_only]),
            &[
                ("a", "m", flat("1.0", "1.0")),
                ("b", "m", flat("9.0", "9.0")),
            ],
        );
        set_metrics(&mut input, "a", metrics(Some(amt("10")), 0, None));
        set_metrics(&mut input, "b", metrics(Some(amt("10")), 5, None));

        let price = input
            .providers
            .iter()
            .find(|rp| rp.provider.id == "a")
            .unwrap()
            .prices
            .get("m")
            .unwrap();
        assert_eq!(price, &flat("1.0", "1.0"));

        // b is unhealthy (5 failures) → a wins
        let out = top(&req("m"), &input);
        assert_eq!(out.provider, "a");
        assert_eq!(out.model, "m");
    }
}
