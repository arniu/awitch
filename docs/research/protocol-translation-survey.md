# Protocol translation survey

> Purpose: calibrate ADR-0005 (translation scope); serve as implementation
> reference for the translation core. Research date 2026-08; all claims from
> primary sources (official docs), fetched 2026-08.

## Objects and sources

| Product               | Inbound format                                                                | Primary source                                                               |
| --------------------- | ----------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Portkey               | **Three formats, mutually translatable** (OpenAI Chat / Responses / Messages) | https://docs.portkey.ai/docs/product/ai-gateway/messages-api / universal-api |
| LiteLLM               | **OpenAI-normalized** (inside SDK/Proxy)                                      | https://docs.litellm.ai/docs/completion/input / providers/anthropic          |
| Cloudflare AI Gateway | OpenAI-compatible                                                             | https://developers.cloudflare.com/ai-gateway/usage/chat-completion/          |
| OpenRouter            | OpenAI-compatible (no translation; vendors must comply)                       | https://openrouter.ai/docs/quickstart                                        |

## How each gateway translates

### Portkey — three inbound formats, translated to any provider

- Three endpoints: `/v1/chat/completions` (OpenAI), `/v1/responses` (OpenAI
  Responses), `/v1/messages` (**Anthropic**). Each works with any provider
  (`@provider/model` switches routing).
- "Portkey translates the Messages format to each provider's native API"
  (source: Portkey messages-api / universal-api docs) — Anthropic format in,
  provider-native out.
- Streaming is normalized back to the inbound format: "normalizes all streaming
  responses to the Anthropic SSE event format" (Messages endpoint).
- Claude Code talks to `/v1/messages` unchanged. The Responses endpoint is
  built on the **Open Responses spec** (open interoperability: unified tool
  calling, semantic streaming).

### LiteLLM — OpenAI-normalized format; SDK/Proxy translates to provider-native

- `completion()` input and output are **OpenAI format throughout** (streaming
  chunks included: `choices[0].delta.content`); the provider side translates
  ("Transforms response_format to Anthropic's output_format", "Converts to
  output_schema", "Creates a tool with the schema and forces the model to use
  it").
- Tool round-trips live in the SDK layer: OpenAI-format tool definitions →
  Anthropic `tool_choice`/`cache_control`; tool results come back with
  `tool_call_id` and are re-translated. **The round-trip key is the call id**
  (the provider side maps to its own `tool_use`/`tool_result` ids; source:
  LiteLLM providers/anthropic).
- Fallback: for models without function-calling support,
  `add_function_to_prompt` embeds tool definitions into the prompt (textual
  round-trip, weak fidelity — degradation, not fidelity).

### Cloudflare AI Gateway / OpenRouter

- CF: one OpenAI-compatible endpoint, `{provider}/{model}` switching.
  Positioning is observability/control; translation is a means, not the point.
- OpenRouter: OpenAI-compatible, **does no translation** — vendors must be
  compatible themselves. Contrast: "vendor-compatible convergence" vs
  LiteLLM/Portkey's "gateway translation".

## Common pattern: one inbound format → provider-native

```
client uses one format ──→ gateway: translate to provider-native ──→ provider
response ──→ gateway: normalize back to the inbound format ──→ client still one format
```

- The inbound format is the **app format** (OpenAI / Anthropic Messages /
  Responses); translation happens inside the gateway, the client only ever
  sees one format — fidelity is guaranteed by "format normalization"
  (symmetric translation).
- Streaming is normalized too; translation is **field mapping**
  (response_format → output_schema, tool → tool_choice); unmapped fields are
  dropped or error (thinking blocks, prompt caching have vendor-specific
  support).

## Tool translation depth (the round-trip mechanism)

- **The round-trip key is the call id**: OpenAI `tool_call_id` ↔ Anthropic
  `tool_use`/`tool_result` ids — lossless round-trip pairs a call with its
  result by id, so id fidelity across translation is the mechanism any
  fidelity guarantee must preserve.
- Mapping surface: tool definitions (JSON schema) ↔ each vendor's
  tool/tool_use schema; `tool_choice` (auto/required/none/specific) ↔ vendor
  tool_choice/force semantics; parallel calls ↔ vendor parallel support.
- Without native support, degradation is prompt injection (weak fidelity).

## Key difference: app convergence vs Awitch's agent-native convergence

|             | Mainstream (Portkey/LiteLLM/CF) | Awitch                                                    |
| ----------- | ------------------------------- | --------------------------------------------------------- |
| Start point | unified app format              | **agent-native protocols** (anthropic/responses/gemini)   |
| Direction   | one format → N providers        | N agent formats → N providers                             |
| Client      | app points at a base URL        | agent unchanged (native protocol straight to the gateway) |

**Portkey's Messages endpoint is the productized counterpart of Awitch's v1
pair** (Claude Code connects directly, translated to any provider); Awitch's
anthropic → openai chat is a subset of it, with passthrough first (same
protocol, no translation).

## Calibration points for ADR-0005

1. **Direction verified**: Anthropic-converged translation to providers is a
   productized mainstream approach (Portkey fully supports Claude Code).
   Awitch's "translate one pair + passthrough" sits between OpenRouter (no
   translation) and LiteLLM/Portkey (translate everything) — the v1 scope is
   sound.
2. **Fidelity positioning**: mainstream implies fidelity by "format
   normalization" (in is out) with no explicit lossless acceptance; Awitch's
   fixture-driven tool-call fidelity acceptance is a differentiating asset
   (ADR-0005 states the bar; the corpus that mechanizes it landed with
   `fixtures/` and `src/protocol/*/tests.rs`).
3. **Tool id fidelity is the round-trip mechanism**: ids must survive
   translation (`tool_call_id` ↔ `tool_use` id) — ADR-0005's "lossless" is
   mechanized as this.
4. **Field-mapping boundary**: mainstream silently drops unmapped fields;
   Awitch refuses explicitly (extended thinking rejected on request, thinking
   blocks dropped in history) — safer (recorded in ADR-0005).
