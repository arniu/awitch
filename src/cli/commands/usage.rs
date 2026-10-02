use chrono::{DateTime, NaiveDate};
use money::{Amount, Currency, Money};

use crate::api_types::{UsageBucket, UsageReport, UsageRow};
use crate::cli::args::UsageArgs;
use crate::cli::ctx::Ctx;
use crate::utils;

use super::money_str;

pub fn run(ctx: &Ctx, args: UsageArgs) -> anyhow::Result<()> {
    let client = ctx.client()?;
    let width = args.by;
    let now = utils::now_unix_secs();
    let start = since(args.since.as_deref(), now)?;

    // Each call serves one window and the walk runs to the one now falls in:
    // skip the leading buckets the start is inside, not past.
    let mut totals = Totals::default();
    let mut cursor = start;
    let mut covered = start;
    loop {
        let report = client.usage(cursor, width, args.app.as_deref(), args.provider.as_deref())?;
        let mut printed = false;
        for bucket in &report.buckets {
            if bucket.end <= covered {
                continue;
            }
            covered = bucket.end;
            if bucket.rows.is_empty() {
                continue;
            }
            if !printed {
                print_page(&report);
                printed = true;
            }
            print_bucket(bucket);
            for row in &bucket.rows {
                totals.add(row);
            }
        }

        if report.end > cursor && report.end < now {
            cursor = report.end;
        } else {
            break;
        }
    }

    totals.print();

    Ok(())
}

/// The instant to report from: the start of this month, or of the month, day
/// or instant `--since` names. Where it ends is the gateway's — the calendar
/// period the walk reaches.
fn since(arg: Option<&str>, now: i64) -> anyhow::Result<i64> {
    let since = match arg {
        Some(since) => since.to_string(),
        None => utils::utc_date(now).format("%Y-%m").to_string(),
    };

    if let Ok(day) = NaiveDate::parse_from_str(&since, "%Y-%m-%d") {
        return Ok(utils::utc_midnight(day));
    }
    if let Ok(first) = NaiveDate::parse_from_str(&format!("{since}-01"), "%Y-%m-%d") {
        return Ok(utils::utc_midnight(first));
    }
    if let Ok(at) = DateTime::parse_from_rfc3339(&since) {
        return Ok(at.timestamp());
    }

    anyhow::bail!("invalid time '{since}', expected YYYY-MM, YYYY-MM-DD or RFC 3339")
}

fn print_page(report: &UsageReport) {
    println!(
        "{} → {}  ({})",
        utils::iso8601(report.start),
        utils::iso8601(report.end),
        report.bucket_width.as_str()
    );
}

fn print_bucket(bucket: &UsageBucket) {
    println!("{}", utils::iso8601(bucket.start));
    for row in &bucket.rows {
        println!(
            "  {:<14} {:<20} {:<24} {:>6} req  {:>10} in  {:>10} out  {}",
            row.app,
            row.provider_name.as_deref().unwrap_or(&row.provider_id),
            row.served_model,
            row.requests,
            row.input_tokens,
            row.output_tokens,
            money_str(&row.cost)
        );
    }
}

/// A running total over the printed rows.
#[derive(Default)]
struct Totals {
    requests: u64,
    input_tokens: u64,
    output_tokens: u64,
    cost: Amount,
    currency: Option<Currency>,
}

impl Totals {
    fn add(&mut self, row: &UsageRow) {
        self.requests += row.requests;
        self.input_tokens += row.input_tokens;
        self.output_tokens += row.output_tokens;
        self.cost = self
            .cost
            .checked_add(row.cost.amount)
            .unwrap_or(Amount::ZERO);
        self.currency = self.currency.or(Some(row.cost.currency));
    }

    fn print(&self) {
        let Some(currency) = self.currency else {
            println!("no usage");
            return;
        };
        let cost = Money {
            amount: self.cost,
            currency,
        };
        println!(
            "total: {} req, {} in-tok, {} out-tok, {}",
            self.requests,
            self.input_tokens,
            self.output_tokens,
            money_str(&cost)
        );
    }
}
