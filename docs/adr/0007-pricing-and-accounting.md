# ADR-0007: Pricing & accounting

- Status: **accepted** (grilling session, 2026-08); **amended 2026-09** —
  health and latency come from per-forward observations (`attempts`), not the
  ledger

## Context

Routing ranks by cost, but cost is not one number: it depends on
vendor × model × time-of-day, and the pre-request estimate (unit price) can
diverge from what the vendor actually bills. Ranking and truth-telling need
different things from the same data.

## Decision

### Price model

Per **vendor × model**: a union of heterogeneous price shapes — a flat rate, a
rate tiered by time of day (peak / off-peak), or a plan with a quota and an
optional metered rate behind it — user-overridable. Quota is consulted first:
remaining quota prices at zero, an exhausted quota falls through to the
metered rate, and with no metered rate the model is unavailable (routing's
hard constraint). Approximation accepted: a request may exceed remaining
quota; the vendor bills the excess at the metered rate, the ledger records
actual spend.

### Accounting (per-request ledger)

1. **Recorded while forwarding** — the ledger entry is written alongside the
   response; recording is **ordered but eventually durable** (ADR-0006): a
   response may return before its entry lands, and an entry that cannot be
   written is logged and dropped — never wrong tokens, only possibly a few ms
   late.
2. **Cost computed post-hoc**: actual tokens × effective price — routing ranks
   by unit price, the ledger tells the truth.
3. **Balance snapshots** — polled from vendor APIs; coding-plan quota is read
   from the pricing data, not polled.

## Consequences

- The ledger feeds cost ranking and accounting; per-forward observations
  (ADR-0006) feed health and latency.
- Built-in pricing for the vendor set (ADR-0003), maintained by the built-in
  data asset.
