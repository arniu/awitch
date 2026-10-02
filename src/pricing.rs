use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

use money::{Amount, Currency};

use crate::protocol::Usage;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{field}: price must be non-negative, got {value}")]
    InvalidPrice { field: &'static str, value: Amount },
    #[error("metered_timed: at least one tier required")]
    EmptyTiers,
    #[error("metered_timed: invalid window {window:?} (expected HH:MM-HH:MM)")]
    InvalidWindow { window: String },
}

/// The accounting currency.
pub(crate) const ACCOUNTING_CURRENCY: Currency = Currency::Usd;

/// An input/output unit-price pair, in USD per million tokens — the
/// accounting currency (ADR-0007). Fixed-point amounts (money.rs).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Price {
    pub input: Amount,
    pub output: Amount,
}

impl Price {
    pub const ZERO: Price = Price {
        input: Amount::ZERO,
        output: Amount::ZERO,
    };

    /// What `usage` costs at these prices.
    pub fn cost(&self, usage: Usage) -> Amount {
        let cost_for = |price: Amount, token: u32| {
            const PER_MILLION: i64 = 1_000_000;
            price
                .checked_mul_div(i64::from(token), PER_MILLION)
                .unwrap_or_default()
        };

        let input_cost = cost_for(self.input, usage.input_tokens);
        let output_cost = cost_for(self.output, usage.output_tokens);
        input_cost.checked_add(output_cost).unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pricing {
    /// One input/output price at any time.
    MeteredFlat {
        #[serde(flatten)]
        price: Price,
    },
    /// Tiers by time window (peak/off-peak). A tier's `window` is `HH:MM-HH:MM`,
    /// UTC, cross-day supported.
    MeteredTimed { tiers: Vec<PriceTier> },
    /// Free within remaining quota, metered beyond (fallback). No fallback =
    /// unavailable once quota is exhausted (ADR-0009 hard constraint).
    QuotaPlan {
        plan: String,
        period: QuotaPeriod,
        /// Current remaining quota (ADR-0007).
        remaining: Option<Amount>,
        /// Optional metered prices used once quota is exhausted.
        #[serde(default)]
        fallback: Option<Price>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceTier {
    /// `HH:MM-HH:MM`, UTC; cross-day when `end <= start`.
    pub window: String,
    #[serde(flatten)]
    pub price: Price,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaPeriod {
    Day,
    Month,
}

impl Pricing {
    pub fn has_quota_remaining(&self) -> bool {
        matches!(
            self,
            Pricing::QuotaPlan { remaining: Some(r), .. } if *r > Amount::ZERO
        )
    }

    pub fn effective_price(&self, now: NaiveTime) -> Option<Price> {
        match self {
            Pricing::MeteredFlat { price } => Some(*price),
            Pricing::MeteredTimed { tiers } => tier_price(tiers, now),
            Pricing::QuotaPlan { .. } if self.has_quota_remaining() => Some(Price::ZERO),
            Pricing::QuotaPlan { fallback, .. } => fallback.as_ref().copied(),
        }
    }

    /// A pricing card's structural well-formedness — exercised by unit tests
    /// today; its write path lands with the pricing control surface.
    #[cfg_attr(not(test), expect(dead_code))]
    pub fn validate(&self) -> Result<(), Error> {
        let bad_price = |value: Amount, field: &'static str| -> Result<(), Error> {
            if value.is_negative() {
                Err(Error::InvalidPrice { field, value })
            } else {
                Ok(())
            }
        };
        match self {
            Pricing::MeteredFlat { price } => {
                bad_price(price.input, "metered_flat input")?;
                bad_price(price.output, "metered_flat output")?;
            }
            Pricing::MeteredTimed { tiers } => {
                if tiers.is_empty() {
                    return Err(Error::EmptyTiers);
                }
                for tier in tiers {
                    if TimeWindow::parse(&tier.window).is_none() {
                        return Err(Error::InvalidWindow {
                            window: tier.window.clone(),
                        });
                    }
                    bad_price(tier.price.input, "metered_timed tier input")?;
                    bad_price(tier.price.output, "metered_timed tier output")?;
                }
            }
            Pricing::QuotaPlan { fallback, .. } => {
                if let Some(f) = fallback {
                    bad_price(f.input, "quota_plan fallback input")?;
                    bad_price(f.output, "quota_plan fallback output")?;
                }
            }
        }

        Ok(())
    }
}

fn tier_price(tiers: &[PriceTier], now: NaiveTime) -> Option<Price> {
    for tier in tiers {
        if let Some(win) = TimeWindow::parse(&tier.window)
            && win.contains(now)
        {
            return Some(tier.price);
        }
    }

    None
}

/// `HH:MM-HH:MM` window; cross-day when `end < start`; `start == end` means
/// all day. Invalid windows are rejected by `Pricing::validate` and ignored
/// by tier lookup (a malformed tier simply never matches).
struct TimeWindow {
    start: NaiveTime,
    end: NaiveTime,
}

impl TimeWindow {
    fn parse(s: &str) -> Option<TimeWindow> {
        let (a, b) = s.split_once('-')?;
        let start = parse_hm(a.trim())?;
        let end = parse_hm(b.trim())?;
        Some(TimeWindow { start, end })
    }

    /// All day for `start == end`; `[start, end)` for same-day; `[start,
    /// 24:00) ∪ [00:00, end)` for cross-day (`end < start`).
    fn contains(&self, t: NaiveTime) -> bool {
        if self.start == self.end {
            return true;
        }
        if self.end > self.start {
            t >= self.start && t < self.end
        } else {
            t >= self.start || t < self.end
        }
    }
}

fn parse_hm(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    // "24:00" is accepted as end-of-day and mapped to 00:00, so
    // "09:00-24:00" and "00:00-24:00" (all day) read naturally.
    if h == 24 && m == 0 {
        return NaiveTime::from_hms_opt(0, 0, 0);
    }
    if h > 23 || m > 59 {
        return None;
    }
    NaiveTime::from_hms_opt(h, m, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    fn amt(s: &str) -> money::Amount {
        s.parse().unwrap()
    }

    fn price(input: &str, output: &str) -> Price {
        Price {
            input: amt(input),
            output: amt(output),
        }
    }

    fn hm(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn flat(input: &str, output: &str) -> Pricing {
        Pricing::MeteredFlat {
            price: price(input, output),
        }
    }

    fn timed(tiers: &[(&str, &str, &str)]) -> Pricing {
        Pricing::MeteredTimed {
            tiers: tiers
                .iter()
                .map(|(window, input, output)| PriceTier {
                    window: (*window).to_string(),
                    price: price(input, output),
                })
                .collect(),
        }
    }

    #[test]
    fn flat_returns_price_any_time() {
        let card = flat("2", "8");
        let expected = price("2", "8");
        assert_eq!(card.effective_price(hm(0, 0)), Some(expected));
        assert_eq!(card.effective_price(hm(12, 0)), Some(expected));
    }

    #[test]
    fn timed_peak_offpeak() {
        // README example: peak 09:00-23:00, off-peak 23:00-09:00 (cross-day)
        let card = timed(&[("09:00-23:00", "2", "8"), ("23:00-09:00", "1", "4")]);
        let peak = price("2", "8");
        let offpeak = price("1", "4");
        assert_eq!(card.effective_price(hm(10, 0)), Some(peak));
        assert_eq!(card.effective_price(hm(23, 30)), Some(offpeak));
        assert_eq!(card.effective_price(hm(0, 30)), Some(offpeak));
        // window boundary: 23:00 belongs to off-peak, 09:00 to peak
        assert_eq!(card.effective_price(hm(23, 0)), Some(offpeak));
        assert_eq!(card.effective_price(hm(9, 0)), Some(peak));
    }

    #[test]
    fn timed_no_matching_window_is_unavailable() {
        let card = timed(&[("09:00-18:00", "2", "8")]);
        assert_eq!(card.effective_price(hm(20, 0)), None);
    }

    #[test]
    fn malformed_window_is_ignored() {
        // a malformed tier never matches; a later valid tier still does
        let card = timed(&[("not-a-window", "1", "1"), ("09:00-18:00", "2", "8")]);
        assert_eq!(card.effective_price(hm(10, 0)), Some(price("2", "8")));
    }

    #[test]
    fn quota_free_while_remaining() {
        let card = Pricing::QuotaPlan {
            plan: "coding_plan".into(),
            period: QuotaPeriod::Day,
            remaining: Some(amt("1000000")),
            fallback: Some(price("2", "8")),
        };

        assert_eq!(card.effective_price(hm(12, 0)), Some(Price::ZERO));
    }

    #[test]
    fn quota_exhausted_falls_back_to_metered() {
        let card = Pricing::QuotaPlan {
            plan: "coding_plan".into(),
            period: QuotaPeriod::Month,
            remaining: Some(amt("0")),
            fallback: Some(price("2", "8")),
        };
        assert_eq!(card.effective_price(hm(12, 0)), Some(price("2", "8")));
    }

    #[test]
    fn quota_exhausted_without_fallback_is_unavailable() {
        let card = Pricing::QuotaPlan {
            plan: "coding_plan".into(),
            period: QuotaPeriod::Day,
            remaining: Some(amt("0")),
            fallback: None,
        };
        assert_eq!(card.effective_price(hm(12, 0)), None);
    }

    #[test]
    fn midnight_end_window_reads_naturally() {
        // "09:00-24:00" == "09:00-00:00" (09:00 → midnight)
        let day_until_midnight = timed(&[("09:00-24:00", "2", "8")]);
        assert!(day_until_midnight.validate().is_ok());
        assert_eq!(
            day_until_midnight.effective_price(hm(23, 0)),
            Some(price("2", "8"))
        );
        assert_eq!(day_until_midnight.effective_price(hm(8, 59)), None);
        // "00:00-24:00" = all day
        let all_day = timed(&[("00:00-24:00", "3", "9")]);
        assert!(all_day.validate().is_ok());
        assert_eq!(all_day.effective_price(hm(12, 0)), Some(price("3", "9")));
    }

    #[test]
    fn validate_rejects_malformed_window() {
        let card = timed(&[("nope", "1", "1")]);
        assert!(card.validate().is_err());
    }

    #[test]
    fn validate_rejects_empty_tiers() {
        let card = timed(&[]);
        assert!(card.validate().is_err());
    }

    #[test]
    fn validate_accepts_flat_and_quota() {
        assert!(flat("1", "1").validate().is_ok());
        let quota = Pricing::QuotaPlan {
            plan: "p".into(),
            period: QuotaPeriod::Day,
            remaining: Some(amt("1")),
            fallback: None,
        };
        assert!(quota.validate().is_ok());
    }

    #[test]
    fn validate_rejects_negative_prices() {
        let cards = [
            flat("-1", "8"),
            Pricing::MeteredTimed {
                tiers: vec![PriceTier {
                    window: "09:00-18:00".into(),
                    price: price("2", "-1"),
                }],
            },
            Pricing::QuotaPlan {
                plan: "p".into(),
                period: QuotaPeriod::Day,
                remaining: Some(amt("0")),
                fallback: Some(price("-1", "1")),
            },
        ];
        for card in cards {
            assert!(card.validate().is_err());
        }
    }

    #[test]
    fn has_quota_remaining_true_only_with_positive_remaining() {
        let quota = |remaining: Option<&str>| Pricing::QuotaPlan {
            plan: "p".into(),
            period: QuotaPeriod::Day,
            remaining: remaining.map(amt),
            fallback: None,
        };
        assert!(quota(Some("5")).has_quota_remaining());
        assert!(!quota(Some("0")).has_quota_remaining());
        assert!(!quota(None).has_quota_remaining());
        assert!(!flat("1", "1").has_quota_remaining());
    }
}
