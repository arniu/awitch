# CONTEXT.md

A glossary of terms that differ from general usage or span multiple modules.

An entry defines one of this project's terms: what it is — never how it works,
what it holds, or what values it takes. A term another entry defines is bold
where the reader would go look it up. Entries are listed by term,
alphabetically.

- **app key** — a per-app API key the gateway issues.

- **attempt** — one forward attempt's terminal outcome, attributed by cause.

- **ledger** — the per-request accounting entry: what was requested, what served
  it, and the price applied.

- **managed key** — a key in an agent's native config that a **point** writes and
  restores.

- **model intent** — what the request's model field asks for: a concrete model,
  or a **requirement**.

- **model matching** — resolving a **model intent** against the **model pool**.
  The gateway reads no name grammar of its own.

- **model pool** — the provider × model pairs the user has added to the gateway.

- **point** — the write that points an agent at the gateway.

- **pointing record** — what a **point** leaves behind: the **managed key**s it
  covered, and what each held before and after.

- **pointing state** — whether an app's native config is pointed at the gateway,
  read from its local files alone.

- **protocol family** — one protocol shape, spoken by more than one provider.

- **provider** — one upstream service the gateway forwards to, with credentials
  of its own and the **served protocol**s it speaks.

- **provider template** — one self-contained provider shape, deployable as it
  stands.

- **requirement** — a model attribute a request demands, on a dimension its
  protocol defines (e.g. context length).

- **routing mode** — the per-request bias between cost and latency.

- **routing pin** — a constraint restricting a request to a chosen set of
  providers.

- **served protocol** — a protocol a provider serves, at the endpoint the
  provider declares.

- **template-backed provider** — a provider record wholly derived from one
  **provider template**.

- **template-free provider** — a provider record with no template link, owning
  all its fields.

- **translation** — rewriting one protocol's requests, responses, and stream
  events into another's, with tool calls round-tripping intact.

- **translation pair** — a directed source→target protocol pairing the gateway
  serves, including a protocol paired with itself.
