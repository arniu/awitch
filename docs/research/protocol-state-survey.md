# Protocol state survey

> Purpose: the protocol-layer facts about **multi-turn state** in the three
> protocol families the gateway serves, so protocol questions are answered from
> the protocol contracts themselves — never from awitch's code or ADRs.
> Research date 2026-09-17; every claim is quoted from a primary source
> (the vendors' own machine-readable specs or generated SDKs), fetched that day.

## Sources

| Family           | Primary source                                                                                                | Location used                                                                  |
| ---------------- | ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| anthropic        | `anthropics/anthropic-sdk-python` (Stainless-generated from the Anthropic spec; no bare OpenAPI is published) | `api.md` (`post /v1/messages`), `src/anthropic/types/message_create_params.py` |
| openai chat      | `openai/openai-openapi` (official OpenAPI)                                                                    | `openapi.yaml` → `CreateChatCompletionRequest`                                 |
| openai responses | same OpenAPI                                                                                                  | `CreateResponse`, `ResponseProperties`, `ConversationParam`                    |

Reproduce:

```
https://cdn.jsdelivr.net/gh/anthropics/anthropic-sdk-python@main/api.md
https://cdn.jsdelivr.net/gh/anthropics/anthropic-sdk-python@main/src/anthropic/types/message_create_params.py
https://cdn.jsdelivr.net/gh/openai/openai-openapi@master/openapi.yaml
https://cdn.jsdelivr.net/gh/openai/openai-python@main/src/openai/types/responses/response_create_params.py
```

## anthropic — Messages

Endpoint: `post /v1/messages`. Required request fields, verbatim:

```
max_tokens: Required[int]
messages:   Required[Iterable[MessageParam]]
model:      Required[ModelParam]
```

Optional fields: `system`, `tools`, `tool_choice`, `thinking`, `stream`,
`metadata`, `output_config`, `service_tier`, `stop_sequences`, `container`,
`inference_geo`, `cache_control`, `user_profile_id`, `workspace_id`.

**No continuation field.** There is no `previous_response_id` and no
conversation id: a multi-turn exchange is the client resending the full
`messages` array. The family is **stateless** at the Messages endpoint.

Server-side state does exist on Anthropic, but outside mainline Messages: the
**beta managed-agents** surface (`post /v1/sessions`, threads, memory stores;
`?beta=true`), which the gateway does not serve.

## openai chat — Chat Completions

`CreateChatCompletionRequest` carries 29 fields:

```
audio, frequency_penalty, function_call, functions, logit_bias, logprobs,
max_completion_tokens, max_tokens, messages, modalities, model, moderation, n,
parallel_tool_calls, prediction, presence_penalty, reasoning_effort,
response_format, seed, service_tier, stop, store, stream, stream_options,
tool_choice, tools, top_logprobs, verbosity, web_search_options
```

**No `previous_response_id` and no `conversation`.** The family is
**stateless**: multi-turn means resending `messages`.

`store` exists here, but means something else, verbatim:

> Whether or not to store the output of this chat completion request for use in
> our model distillation or evals products.

It is unrelated to conversation state — a same-name trap against the Responses
`store`.

## openai responses

`previous_response_id`, verbatim:

> The unique ID of the previous response to the model. Use this to create
> multi-turn conversations. Learn more about conversation state. Cannot be used
> in conjunction with `conversation`.

`conversation` (`ConversationParam`: a conversation id string, or an object),
verbatim:

> The conversation that this response belongs to. Items from this conversation
> are prepended to `input_items` … Input items and output items from this
> response are automatically added to this conversation after this response
> completes.

`store`, verbatim:

> Whether to store the generated model response for later retrieval via API.
> Defaults to true when omitted. If set to true, response data will be stored
> for at least 30 days, subject to the data retention exceptions.

`instructions` under continuation, verbatim:

> When used along with `previous_response_id`, the instructions from a previous
> response will not be carried over to the next response. This makes it simple
> to swap out system (or developer) messages in new responses.

Both continuation fields are optional (`Optional[str]` / `anyOf: [string,
null]`), so a client may be **stateless** (resend full `input`) or **stateful**
(send the new turn plus an id) per request. `previous_response_id` and
`conversation` are mutually exclusive; they are two alternative state
mechanisms.

Transport note, verbatim (WebSocket `response.create`):

> `stream_id` controls routing; `previous_response_id` controls conversation
> lineage, so a new lane can fork from a response created on another lane.

Capability note: the `Model` object exposes only `id`, `created`, `object`,
`owned_by`, `shutdown_date`. **No field advertises statefulness.**

## Not stated by the protocol

These are open at the protocol layer — the specs are silent, so they cannot be
answered from code or from a vendor's successful response:

- whether `previous_response_id` requires `store: true`;
- how a response id is scoped (account / organization / project);
- what happens on an unknown or expired id;
- whether a response stored with `store: false` can be continued;
- any declarative statefulness capability.

## Protocol-level conclusions

1. **Only openai responses carries server-side multi-turn state.** anthropic and
   openai chat clients always carry the full history themselves.
2. **Statefulness is per request, not a session property**, and it is optional —
   a Responses client may switch between stateless and stateful.
3. **The protocol does not expose whether a provider can honour a continuation.**
   Anything the gateway does to learn this (probe, template declaration, learning
   from failures) is an implementation choice, not a protocol capability.
4. Because both translation pairs target openai chat, and openai chat has no
   continuation field, a stateful Responses request has **no translated form**.
   That is a protocol fact, not a gateway policy.

Routing and translation responses to these facts live in ADR-0005 and ADR-0009.
