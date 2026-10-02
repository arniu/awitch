# ADR-0013: Migration

- Status: **accepted** (2026-08). Amended 2026-08-30: provider template
  matching by base URL (ADR-0008 §1). Amended 2026-09-02: an exact
  canonical base imports as a template-backed record, any deviation as a
  template-free record (ADR-0008 §2.2); no model catalog is ever carried
  from the source (runtime data, ADR-0008 §1). Amended 2026-09-03: post-import
  verification runs by default, with an opt-out.

## Context

Migration replays an external provider switcher's pool into awitch. The
first source is cc-switch, which stores the pool as **app × provider**: its
database holds one row per (provider id, app), each row's payload that app's
settings JSON (base URL, API key, models, model selection). awitch's pool is
**provider**: one key and one protocol set per provider (ADR-0004). The
mapping is not one-to-one, and the keys make the import sensitive — each app
row may carry its own plaintext key for the same vendor, so credentials must
never be merged or dropped silently.

General import/export (ADR-0011) is awitch's own full-db snapshot, not an
external-source import, so migration is its own mechanism. Migration also
stays within the gateway's non-invasive constraint: only awitch's store is
written (ADR-0006 single writer); pointing agents is never an import side
effect.

## Decision

One command per source, read-only on the source; later sources follow the same
contract — read-only source, provider derivation, additive idempotency,
dry-run, verification by default.

1. **Source** — the cc-switch database (`~/.cc-switch/cc-switch.db`) by
   default, with a path override, opened read-only. A row that is not in the
   app set, or whose payload shape awitch does not parse, is reported — never
   silently dropped.
2. **Derivation** — one provider per distinct **(base_url, api_key)** pair
   across the app rows, intersected with the app set (ADR-0002); the id is
   assigned, unique against the existing pool. Protocols = the union of the
   families the rows' endpoint choices exercised.
3. **Record kind by base match** — each derived provider is matched to a
   provider template by its base URL (ADR-0008): the exact canonical base
   first, else the closest path-prefix match, else the vendor-name slug. An
   exact match imports a **template-backed** record — a pure snapshot,
   identical to a template add with the row's key; the record is the
   template's (ADR-0008 §2.2). Any other match imports a **template-free**
   record carrying the row's own fields; the matched template seeds only its
   model-list and balance URLs, one-shot — never a link, never refreshed. No
   template match leaves those URLs unset; both are reported, never invented.
   The model catalog is never carried from the source — it is runtime data
   the gateway syncs from the provider (ADR-0008 §1).
4. **Idempotency** — additive: an existing provider id is skipped and
   reported; nothing is overwritten (ADR-0008 §2.4).
5. **Dry-run** — prints each derived provider (id, base URL, protocols, key
   fingerprint) and writes nothing; the real run writes the same.
6. **Post-import verification (default)** — a real run probes each imported
   provider's protocol endpoints and reports each by the protocol survey's
   status semantics, distinguishing an existing endpoint, a missing one, and an
   unreachable host; the opt-out is a flag. A missing path is re-probed
   credential-lessly — a POST-only endpoint can hide behind a GET 404, and a
   non-404 there means the path exists — with no spend and no credential
   exposure. Verification is CLI-side HTTP — the control surface is unchanged
   (ADR-0010). It reports what derivation produced; a broken row is surfaced,
   not silently imported.

Source usage history (proxy request logs, daily rollups) is not imported;
`provider balance` stays the accounting surface.

## Consequences

- Import replays the source's pool; per-account keys survive as separate
  providers, so `provider balance` answers per account.
- Protocols are derived, not re-typed; the model catalog syncs at runtime
  rather than being carried from the source.
- Verification by default turns an import into a checkpoint — each endpoint's
  probe result is seen before the pool is relied on.
- One provider per key can crowd the pool; merging is a later explicit edit,
  never silent.
- App-specific model _selections_ are not imported — they stay in each app's
  config and route by identity at request time; a selection naming an id the
  provider's synced catalog does not carry is vetoed.
- Source usage history stays in the source if ever needed.
- The source remains installed and usable; awitch owns the pool afterwards.
