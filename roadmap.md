# Roadmap

## M1 — replace cc-switch

The gateway already covers the switcher's core value (auto-routing instead
of manual switching). Two pieces complete the replacement:

1. **`awitch migrate cc-switch`** (ADR-0013) — import the cc-switch pool.
2. **Post-migration verification** — probe migrated endpoints; re-verify
   the balance endpoints the survey marks second-hand (`†`).

## M2 — data freshness

1. **Pricing acquisition** — per-model pricing in the ADR-0007 shapes (flat /
   timed / quota), seeded from official price pages and catalogs with
   official-page verification and price-page scraping. (The model-id catalog
   is already a runtime pull from each provider's `models_url` — ADR-0008,
   amended 2026-09-02. That 24 h refresh replaces a provider's models
   wholesale, so M2 pricing must land in a store the refresh does not
   overwrite.)
2. **Stale-price signal** — ledger-vs-balance divergence auto-triggers the
   vendor's pricing refresh (ADR-0007 §3).

## M3 — accounting

- Ledger 90-day detail + monthly rollup.

## M4 — hardening

- Passphrase encryption.

## Deferred

- Import & export (ADR-0011)
- WebDAV + S3 sync (ADR-0012)
- Self-update
- Session affinity
- Gateway-held conversation state — store input/output items so a stateless or
  translated provider can continue a Responses conversation (ADR-0006/0009;
  protocol facts in `docs/research/protocol-state-survey.md`)
- Route sync through the chat hub — collapse `extract_*`/`observe_*`
