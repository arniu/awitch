# ADR-0002: App set

- Status: **accepted** (grilling session, 2026-08)

## Context

Only a handful of CLI coding agents matter, but choosing them by taste
invites churn and drift — the choice needs a repeatable rule, applied
uniformly as the set grows (a new app follows the same selection criteria).
The evidence is OpenRouter's `/apps` Global Ranking (fetched 2026-08).

## Decision

The gateway supports the CLI coding agents that meet these criteria: (1) can
configure a baseURL + auth token, (2) speaks a protocol the gateway serves,
(3) has significant OpenRouter usage as a signal of real-world relevance —
and the set as a whole covers every protocol the gateway serves. The shipped
set is what awitch can point; this record holds the rule, not the roster.

## Consequences

- Each app = one-time pointing: baseURL + app key. Non-invasive —
  only managed keys are touched.
- Adding a new app follows the same selection criteria.
