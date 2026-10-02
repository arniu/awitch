# ADR-0003: Vendor set

- Status: **accepted** (grilling session, 2026-08) — amended 2026-08-30:
  the set is now the provider template grouping; vendor is a field.
  Amended 2026-09-02: vendor is no longer a field — templates group under a
  vendor by id naming (the label is the default template's id; variants
  prefix it).
  Amended 2026-09-17: SiliconFlow joins — the set is a dated snapshot of the
  rule's output, not a canon.

## Context

A defined vendor set serves the data supply (Data asset)
and the protocol families (which vendors the gateway must serve natively or by
translation). Without a rule, the set drifts by taste — the same failure the
App set guards against for apps. The evidence is OpenRouter's coding ranking
(`/api/v1/models?sort=coding-high-to-low`, fetched 2026-08-16; 60 vendors, 413
models) plus, for SiliconFlow, a dated protocol probe and pricing catalog
(`docs/research/provider-protocol-survey-data.md`).

## Decision

The set is selected by rule, not taste. The list is a dated snapshot of the
rule's output, never a canon: the evidence sources below are re-run rather
than trusted, and an addition the snapshot missed is recorded as an amendment,
not treated as a claim of eligibility. The shipped set is this rule's output;
this record holds the rules, not the roster.

1. **Protocol anchors** — the vendors whose protocol families the gateway
   natively serves (anthropic / openai chat / openai responses), the families
   the translation pairs are anchored on: **Anthropic / OpenAI**.
   Pinned by protocol, immune to rank drift.
2. **Coding-rank vendors** — vendors with a model in OpenRouter's coding
   ranking top-10 (`sort=coding-high-to-low`). Same source as the App set's
   `/apps` ranking, same root as the data supply (models.dev). The anchors'
   models sit in that top-10; the ranking fills the remaining slots.
3. **Aggregators** — vendors that resell access to other vendors' models
   rather than serving their own. **OpenRouter**, the original member, doubles
   as the data-supply source; **SiliconFlow** joins on its catalog and
   protocol evidence (2026-09-17) as the set's mainland point of access — the
   OpenRouter-derived snapshot covered no mainland aggregator, a gap in the
   evidence source, not a verdict on vendors.

Each vendor ships one or more provider templates of the data asset: the
vendor label is the default template's id, and variant templates prefix
their id with it. The grouping is id naming — never a stored field or entity.

## Consequences

- The set is a scope decision, separate from the data supply chain that
  services it.
- The list is a dated snapshot, never a canon: a vendor joins by a recorded
  decision — the ranking rule or a dated amendment — never by taste, and the
  snapshot stays re-runnable. Rank drift shifts only the rank-filled slots;
  the anchors never move.
- Meta (Llama) is admitted by rank alone — open-weight and typically reached
  through hosting providers, its inclusion is the ranking's call, not a
  direct-key preference.
