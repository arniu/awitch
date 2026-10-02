use chrono::{Datelike, Days, Months, NaiveDate};
use money::Amount;
use serde::{Deserialize, Serialize};

use crate::pricing::Price;
use crate::protocol::{Protocol, Usage};
use crate::utils::{utc_date, utc_midnight};

/// The width of one bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum BucketWidth {
    Hour,
    Day,
}

impl BucketWidth {
    pub const fn as_str(self) -> &'static str {
        match self {
            BucketWidth::Hour => "hour",
            BucketWidth::Day => "day",
        }
    }

    /// Seconds in one bucket.
    pub const fn secs(self) -> i64 {
        const HOUR: i64 = 3_600;
        const DAY: i64 = 86_400;

        match self {
            BucketWidth::Hour => HOUR,
            BucketWidth::Day => DAY,
        }
    }

    /// Start of the bucket `at` falls in, UTC.
    pub fn start(self, at: i64) -> i64 {
        at - at.rem_euclid(self.secs())
    }

    /// End of that bucket — the start of the next one.
    pub fn end(self, at: i64) -> i64 {
        self.start(at) + self.secs()
    }

    pub fn buckets(self, start: i64, end: i64) -> impl Iterator<Item = (i64, i64)> {
        let mut at = start;

        std::iter::from_fn(move || {
            if at >= end {
                return None;
            }

            let bucket_end = self.end(at).min(end);
            let bucket = (at, bucket_end);
            at = bucket_end;

            Some(bucket)
        })
    }

    pub fn window(self, start: i64, end: i64) -> (i64, i64) {
        let day = utc_date(start);
        let (start_day, end_day) = match self {
            BucketWidth::Hour => (day, week_on(day)),
            BucketWidth::Day => (month_start(day), next_month_start(day)),
        };

        (
            utc_midnight(start_day),
            utc_midnight(end_day).min(self.end(end)),
        )
    }
}

/// The first day of `day`'s month.
fn month_start(day: NaiveDate) -> NaiveDate {
    day.with_day(1).expect("the 1st of every month exists")
}

/// The first day of the month following `day`'s.
fn next_month_start(day: NaiveDate) -> NaiveDate {
    month_start(day)
        .checked_add_months(Months::new(1))
        .expect("one month on stays within the representable range")
}

/// The day a week on from `day`.
fn week_on(day: NaiveDate) -> NaiveDate {
    day.checked_add_days(Days::new(7))
        .expect("seven days on stays within the representable range")
}

/// Ledger rows aggregated per `(bucket_start, app, provider_id, served_model)`.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageSummary {
    /// The bucket's start.
    pub bucket_start: i64,
    pub app: String,
    pub provider_id: String,
    pub provider_name: Option<String>,
    pub served_model: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: Amount,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LedgerNew {
    pub app: String,
    pub provider_id: String,
    pub response_id: Option<String>,
    pub conversation_id: Option<String>,
    pub requested_protocol: Protocol,
    pub requested_model: String,
    pub served_protocol: Protocol,
    pub served_model: String,
    pub usage: Usage,
    pub price: Option<Price>,
}

impl LedgerNew {
    pub fn cost(&self) -> Amount {
        self.price
            .map(|p| p.cost(self.usage))
            .unwrap_or(Amount::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    }

    #[test]
    fn a_daily_window_is_the_month_containing_the_start() {
        let (start, end) = BucketWidth::Day.window(at(2026, 9, 10, 12, 0), at(2026, 9, 25, 8, 0));

        assert_eq!(start, at(2026, 9, 1, 0, 0));
        assert_eq!(end, at(2026, 9, 26, 0, 0));
    }

    #[test]
    fn an_hourly_window_runs_a_week_from_the_start_day() {
        let (start, end) = BucketWidth::Hour.window(at(2026, 9, 10, 12, 0), at(2026, 9, 25, 8, 0));

        assert_eq!(start, at(2026, 9, 10, 0, 0));
        assert_eq!(end, at(2026, 9, 17, 0, 0));
    }

    #[test]
    fn a_window_is_cut_at_the_bucket_the_end_falls_in() {
        let (start, end) = BucketWidth::Hour.window(at(2026, 9, 10, 12, 0), at(2026, 9, 12, 8, 0));

        assert_eq!(start, at(2026, 9, 10, 0, 0));
        assert_eq!(end, at(2026, 9, 12, 9, 0));
    }

    #[test]
    fn a_window_stops_at_its_own_end_when_the_month_turns_over() {
        let (start, end) = BucketWidth::Day.window(at(2026, 9, 10, 12, 0), at(2026, 10, 5, 8, 0));

        assert_eq!(start, at(2026, 9, 1, 0, 0));
        assert_eq!(end, at(2026, 10, 1, 0, 0));
    }

    #[test]
    fn buckets_run_one_to_a_width_across_the_window() {
        let buckets = BucketWidth::Day
            .buckets(at(2026, 9, 1, 0, 0), at(2026, 9, 4, 0, 0))
            .collect::<Vec<_>>();

        assert_eq!(
            buckets,
            [
                (at(2026, 9, 1, 0, 0), at(2026, 9, 2, 0, 0)),
                (at(2026, 9, 2, 0, 0), at(2026, 9, 3, 0, 0)),
                (at(2026, 9, 3, 0, 0), at(2026, 9, 4, 0, 0)),
            ]
        );
    }

    #[test]
    fn the_last_bucket_ends_where_the_window_does() {
        let buckets = BucketWidth::Hour
            .buckets(at(2026, 9, 1, 0, 0), at(2026, 9, 1, 1, 30))
            .collect::<Vec<_>>();

        assert_eq!(
            buckets,
            [
                (at(2026, 9, 1, 0, 0), at(2026, 9, 1, 1, 0)),
                (at(2026, 9, 1, 1, 0), at(2026, 9, 1, 1, 30)),
            ]
        );
    }
}
