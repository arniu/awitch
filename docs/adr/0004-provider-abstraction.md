# ADR-0004: Provider abstraction

- Status: **accepted** (grilling session, 2026-08).

## Context

Vendors expose protocols unevenly: one vendor may serve several, another
a single one, in different shapes. Routing and translation must ask the same
question of every provider — "can it serve this request's protocol?" — and
get an unambiguous, uniform answer.

## Decision

### Providers are abstracted by protocol family

Provider capability is expressed per protocol family, not per vendor API
shape. Two providers that serve the same family are interchangeable to
routing and translation at the family level, whatever their underlying
endpoints look like.

### The family set is closed

The gateway recognizes `anthropic`, `openai chat`, and `openai responses`.
Extending the set is an ADR-level change.

### Serving is a per-provider fact

A provider may serve any subset of the families. A request is eligible for a
provider only through a family that provider serves. What serving means for
each family — where and how it is reached — is provider data with fixed
semantics, never something a provider derives from its own name or identity.

### Pricing is a model fact

Cost attaches to the vendor × model pair, independent of how many families a
provider serves.

## Consequences

- Routing filters candidates by the requested family alone, against a closed
  vocabulary.
- A provider's served families can change without touching routing logic or
  the family set.
- Per-family serving data stays provider-specific: the abstraction keeps
  vendors interchangeable without flattening their differences away.
