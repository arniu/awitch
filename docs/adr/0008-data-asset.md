# ADR-0008: Data asset (sourcing & verification)

- Status: **accepted** (grilling session, 2026-08)

## Context

Hand-entered pricing keeps the pool under-seeded. Shipped data removes that
friction but brings staleness and trust risks: prices expire, users may hold
private rates. No single catalog is trustworthy alone — a live check
(2026-08) found models.dev and the OpenRouter API disagreeing on 23% of their
shared models' prices — and catalogs carry flat real-time prices only: no
timed/quota schemes, no served protocols, no per-entry verification stamps.

## Decision

The asset ships the data the gateway needs pre-configured — provider templates
for ADR-0003's vendors — built into the binary. The unit of the asset is the
**provider template** (CONTEXT.md); providers link to the provider template,
never to the vendor. The data awitch needs about a provider splits into
separate objects with different sources, verification, and staleness impact —
the provider template ships only the first (§1); the model catalog and pricing
are runtime data.

A template is compiled in: adding or changing one is a code change. The store
never holds templates — a template-backed provider is a snapshot of one, taken
at add time, and the snapshot stays comparable against the built-in template,
so staleness is derivable when the binary (and with it the template) changes.

### 1. Data objects

**Provider template config** — the served protocols (ADR-0004) with
per-family endpoint paths and canonical claims, balance endpoint. No catalog
provides it: the served protocols and their endpoints come from
protocol-survey probing (`†` = second-hand, must re-probe). Verification is
endpoint probing. Extremely stable; an error breaks requests (correctness),
not ranking.

**Model config** — model id and name, context window, capability bits (tool
calling, reasoning, vision). Seeds come from catalogs (models.dev and the
OpenRouter models API); verification is against official model APIs and
docs. Low frequency (new models, context expansions).

**Pricing** — per-model prices in the ADR-0007 shapes (flat / timed / quota).
Catalogs seed only flat values, real-time and possibly lagged; timed and quota
schemes exist only in official docs. Verification is against official price
pages — the only baseline covering all shapes — plus a runtime calibration
loop: the ledger's cost prediction against balance-snapshot actual spend,
divergence being the stale-price signal (ADR-0007). High frequency; staleness
degrades cost ranking, never correctness (health and requirement filters still
protect the user).

### 2. Common rules

1. **Source hierarchy** — catalogs (models.dev / OpenRouter) provide
   unverified default seeds; official first-hand sources (price pages,
   model APIs, probing) are the verification baseline — only they may mark
   an entry verified, per data object: each verifies independently
   (protocols verified while pricing still unverified).
2. **Storage** — a provider is stored as a snapshot, **template-backed** or
   **template-free** by construction (CONTEXT.md). A template-backed provider
   owns no template field individually, so field provenance is never stored:
   its only differences from the template are its routing id, its API key,
   its staleness — read by comparing its connection fields to the template's
   built-in shape — and its synced model catalog (runtime data, not template
   content). No template is ever stored: the template's current shape is
   always the built-in one.
3. **Built-in source and startup rebuild** — the shipped provider templates
   ride in the binary as a fixed list, one file per template; adding one is a
   code change, never runtime discovery or a build step. Because a template
   changes only with a binary upgrade, at startup the store rebuilds every
   template-backed record that is not a pure snapshot of its template's
   current built-in shape, resetting the record's template fields and keeping
   the routing id, the API key, and the synced model catalog. Records still
   pure snapshots, template-free records, and records whose template no
   longer ships are left alone. The same predicate gates every provider write,
   so a deviant record never enters the store.
4. **User ownership is by record kind, not by field** — a template-backed
   record carries no user fields beyond its API key; a template-free record is
   entirely user-owned. Nothing the user sets is ever rewritten: template
   fields on a template-backed record change only by a template upgrade, and
   template-free records are never rebuilt. Private rates and per-record
   deviations therefore live on template-free records.

### 3. Acquisition & refresh

1. **Provider template application** — adding from a template snapshots its
   built-in shape, with only the routing id and the API key supplied at add;
   no per-field flags, no network at add time. Customizing a field means a
   template-free add or a local definition file — a template-backed record's
   template fields are not editable (only its API key is).
2. **Template refresh** — a template's content changes only when the binary
   does (a code change to the shipped file). The change propagates at the
   next startup: records no longer pure snapshots of their template are
   rebuilt (see §2.3); template-free records are never touched. The model
   catalog and pricing are not refreshed by command — they are runtime data
   pulled per provider; how pricing is acquired beyond the seed is a separate
   decision.
3. **Stale-price signal** — ledger-vs-balance divergence auto-triggers a
   pricing refresh for the affected provider's models, recorded and
   traceable, closing the freshness loop without blind polling.

## Consequences

- One id yields a working provider; no protocols/models/pricing typing.
- A template-backed provider is comparable against its template's built-in
  shape, so staleness is derivable per record and a template upgrade rebuilds
  exactly the records whose snapshot predates it; a template-free provider is
  never touched.
- Endpoint evidence lives in the survey matrix; entries cite their row.
- One provider template per file: per-provider-template diffs in review; a
  vendor with multiple provider templates ships one file per variant.
- Adding or changing a provider template is a code change, never runtime
  discovery; the store holds no template copy to drift from it.
- Freshness disclaimer: prices have a shelf life; vendor pages are
  authoritative; catalogs are seeds, never verification.
- Price-page scraping and community contribution are future sourcing
  channels.
