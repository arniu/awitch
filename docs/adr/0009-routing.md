# ADR-0009: Routing

- Status: **accepted** (grilling session, 2026-08); **amended 2026-08** — the
  requirement registry is deferred (identity matching only, pending a
  model-capability data source); **amended 2026-09** — health and
  latency come from per-attempt observations (`attempts`), not the ledger

## Context

With several providers behind one gateway, every request is a fresh
which-provider-which-model decision. It must be cheap and stateless. A pick
driven only by price could land on a provider that cannot serve the request,
is out of quota, or is failing. The model field is ambiguous by design (a
default name can stand for a requirement, a configured name for a concrete
model); routing must decide intent without a second signal.

A continuation is not a fresh decision: it carries provider-side state — an
openai responses `previous_response_id`, or a `conversation` — which the
provider stores by default and the gateway cannot reproduce.

The ranking math, the constraint order and the failover classification live
in `docs/model-matching-spec.md`; this record holds the decisions.

## Decision

### Contract: one request, one ranked list

Routing is a function of the request and the observed pool: given the app, the
requested model and the requested protocol, it returns a **ranked candidate
list** — each candidate a provider, a model, and the protocol that provider
would serve. It holds no session or binding state. The stages are **filter**
(hard constraints), **match** (identity), **rank** (preference — candidates
ordered by the mode's blend) and **fallback** (the top candidate is tried;
retryable failures move down).

### Correctness exception: continuation routing

A continuation is not a candidate to select. It goes to the provider that
holds the state, alone and **natively** — the selection stages do not apply to
it, and translation cannot serve it, because a translated turn would reach the
upstream stripped of the context the id stands for. The model is not part of
the lock: the request carries its own `model`, forwarded as given.

What is servable follows from how the earlier response was served:

- **natively** — the id resolves to that provider, and the request is forwarded
  straight to it.
- **by translation** — the id belongs to a response the gateway rewrote into
  another protocol, which carries no continuation of its own; the request is
  refused, never sent upstream with its context missing.
- **not at all** — the id names no provider the gateway knows; the request is
  served natively only, never translated, because a native provider may still
  hold the state.

`conversation` is the contract's other continuation mechanism; the gateway keeps
no conversation index, so it can no more resolve one to a provider than an
unknown id, and takes the same native-only rule.

### Filter: hard constraints (any mode)

A provider that cannot serve the request is out, in any mode:

- **Provider preference** — a routing pin restricts the request to the pinned
  candidate set.
- **Translation availability** — the requested protocol is native to the
  provider, or covered by a translation pair.
- **Ejection** — consecutive failures reach the threshold, unless the provider
  has been quiet past the cooldown; both are gateway settings. Ejection is read
  from per-attempt observations, not the ledger, and the provider returns to
  the pool on its own.
- **Balance / quota** — balance below the threshold **and** no quota-free
  model.
- **Model identity** — the requested model id exists in the pool (see Match).

### Match: identity only (requirement decode deferred)

The model field can carry two intents the gateway cannot tell apart from the
string alone — identity (a concrete named model) and requirement (a logical
name standing for a capability tier). Matching runs once over the filtered
pool, and today it is **literal only**: `model id == requested`. Identity is
the strongest signal, and a request whose name matches no model id in the pool
is unroutable.

The **requirement-decode registry is deferred**: a logical name would consult a
user-declared name→requirement registry and match by capability. The design
intent stands — matching knowledge is explicit stored data, **no built-in
name→requirement grammar** (no tier words, no `claude-*` family decoding, no
`[1M]` / `-thinking` suffixes) — but it is not implemented: its value is gated
on a model-capability data source, and without that data a requirement
predicate would silently pass everything (unknown → neutral). The semantics a
re-enabled registry would have are in `docs/model-matching-spec.md`.

- Registry entries never override a literal hit (a user-written name is the
  strongest identity signal).
- No auto-learning of mappings; no request-content classifier (routing uses
  app type + model name).
- Until the registry is re-enabled, a bare tier word matches nothing.

### Rank: mode-based blend

Eco, balanced and speed weight price against latency; the mode sets a slope,
not a hard ordering — at eco a cheaper option wins despite slower latency, at
speed a faster one despite higher price. A provider with no observation yet
ranks neutral: not excluded, not favored. Cost ranking uses **unit price**;
token counts are unknown before the request. The weights and the normalization
are in `docs/model-matching-spec.md`.

### Fallback: candidates in order

Routing returns the full list, not a single choice. The top candidate is tried
first; on a retryable upstream failure the next candidate in order is tried.
A continuation returns a single candidate — the provider the id names — so it
has no fallback: a failure there is terminal.

A failure is retryable when it is **attributed to the candidate**: a connection
failure, or a response the target returned after the request was translated to
its protocol. A failure the gateway itself produced from the request — request
validation, translation — is terminal, and a stream that already began is never
re-issued. The classification is in `docs/model-matching-spec.md`.

## Consequences

- Every request yields provider + model + served protocol (ranked candidates,
  top tried first, fallback on retryable failure).
- The feedback loop is per-attempt observations: delivered latency → ranking,
  failed → ejection.
- Prompt-cache loss across providers is accepted (cost issue, not
  correctness); continuation routing recovers it.
