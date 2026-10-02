pub(crate) mod completions;
pub(crate) mod migrate;
pub(crate) mod pin;
pub(crate) mod point;
pub(crate) mod provider;
pub(crate) mod service;
pub(crate) mod settings;
pub(crate) mod status;
pub(crate) mod usage;

use std::path::Path;

use crate::pointing::app_by_name;

/// A provider's observed failures, as facts. Nothing here ejects a provider:
/// the cooldown and the failure threshold are the user's settings, and the
/// verdict is routing's (ADR-0009) — the display only reports what was
/// observed.
pub(crate) fn failures_str(failures: u32, last_attempt_at: i64, now: i64) -> String {
    if failures == 0 {
        return "0 failures".to_string();
    }
    match last_attempt_at {
        0 => format!("{failures} failures"),
        at => format!(
            "{failures} failures, last {} ago",
            compact_secs(now.saturating_sub(at))
        ),
    }
}

/// The ejection rule's parameters, which a reader needs alongside the failures
/// to apply the rule (ADR-0009).
pub(crate) fn failure_rule(settings: &crate::settings::Settings) -> String {
    format!(
        "eject at {} consecutive failures, cooldown {}",
        settings.routing_failure_threshold,
        compact_secs(settings.routing_cooldown_secs)
    )
}

/// Seconds as a compact span: `45s`, `12m`, `3h`, `2d`.
fn compact_secs(secs: i64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// Render an amount: `$` for the accounting currency, the code otherwise.
/// Two decimals — the ledger's precision is finer than a status line needs.
pub(crate) fn money_str(money: &money::Money) -> String {
    let amount = money.amount.round_dp(2);
    match money.currency {
        money::Currency::Usd => format!("${amount}"),
        currency => format!("{amount} {currency}"),
    }
}

fn resolve_apps(apps: &[String]) -> Vec<String> {
    if apps.is_empty() {
        crate::pointing::apps()
            .map(|def| def.name.to_string())
            .collect()
    } else {
        apps.to_vec()
    }
}

fn resolve_installed_apps(config_dir: &Path, apps: &[String]) -> Vec<String> {
    let targets = resolve_apps(apps);
    if apps.is_empty() {
        targets
            .into_iter()
            .filter(|app| app_by_name(app).is_some_and(|def| def.bind(config_dir).is_installed()))
            .collect()
    } else {
        targets
    }
}

fn print_probe(p: &crate::provider::Provider) {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();
    for (proto, url, status) in crate::provider_upstream::probe_endpoints(&client, p) {
        println!("verify: {} {proto} {url} -> {status}", p.id);
    }
}

/// Trailing `", failed N"` for a summary line — empty when nothing failed.
fn suffix_failed(n: usize) -> String {
    if n == 0 {
        String::new()
    } else {
        format!(", failed {n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_are_reported_as_facts() {
        assert_eq!(failures_str(0, 0, 1_000), "0 failures");
        assert_eq!(
            failures_str(3, 1_000, 1_000 + 12 * 60),
            "3 failures, last 12m ago"
        );
        // an unobserved provider has no age to report
        assert_eq!(failures_str(3, 0, 1_000), "3 failures");
    }

    #[test]
    fn the_ejection_rule_comes_from_the_settings() {
        let settings = crate::settings::Settings::parse_from(&std::collections::HashMap::from([
            ("routing.failure_threshold".to_string(), "2".to_string()),
            ("routing.cooldown_secs".to_string(), "3600".to_string()),
        ]));
        assert_eq!(
            failure_rule(&settings),
            "eject at 2 consecutive failures, cooldown 1h"
        );
    }
}
