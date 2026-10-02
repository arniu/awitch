# ADR-0005: Translation scope

- Status: **accepted** (grilling session, 2026-08); **amended 2026-08** — the
  `openai responses → openai chat` pair shipped (the second pair)

## Context

Translation is the hard asset: agents break against third-party endpoints when
tool calls don't round-trip, so the bar is **lossless round-trip tool calls**,
not approximate fidelity. Proving that bar on every pair at once spreads the
proof thin before any is earned. The source is anchored by the app set
(ADR-0002): the apps fix the protocols the gateway must expose — an app that
cannot be re-pointed to another protocol pins its own — and their usage
ranking orders the work — Claude Code (anthropic) before Codex (openai
responses). The target is anchored by both sides — most apps already
speak openai chat natively, and vendors expose it alongside other protocols
(ADR-0004).

## Decision

**Translation pairs**: `anthropic → openai chat` and
`openai responses → openai chat`, plus same-protocol pairs.

### Acceptance bar

- **Tool-call id survives translation** — OpenAI `tool_call_id` ↔ Anthropic
  `tool_use`/`tool_result`; id fidelity is the round-trip mechanism (verified
  against LiteLLM/Portkey).
- Each pair includes streaming (chunk-by-chunk SSE). Thinking blocks are
  dropped in history; extended thinking on the request is rejected (explicit
  refusal — mainline gateways silently drop unmapped fields).

### Gateway endpoints

The gateway exposes one endpoint per protocol family — anthropic, openai chat,
openai responses. Serving prefers the same-protocol pair; translate
only where implemented: `openai responses` → openai chat when the provider
lacks the native protocol.

## Consequences

- Codex (openai responses) is served natively wherever a vendor speaks
  responses; a chat-only vendor reaches it through the `openai responses →
openai chat` pair.
- All pairs point INTO openai chat, so its translation and streaming are reused
  by later pairs.
- The pairs are proven at tool-call fidelity.
- Routing's "translation availability" hard constraint consults this ADR's
  pair list.
