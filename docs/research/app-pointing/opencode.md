# OpenCode

> Source: `github.com/anomalyco/opencode` @ `v1.18.18`

## Config files

- Global: `~/.config/opencode/opencode.json` (user-level provider/model/permission
  preferences)
- Project: `opencode.json` (project root, highest priority)
- TUI settings: `~/.config/opencode/tui.json`
- Plugins/agents/commands subdirs use plural names (`agents/`, `plugins/`, ...)
- Config carries `"$schema": "https://opencode.ai/config.json"`

## Provider definition

OpenCode is built on the Vercel AI SDK + Models.dev, with 75+ built-in providers
that rarely need defining; the `provider` section customizes:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "anthropic": {
      "options": { "baseURL": "https://api.anthropic.com/v1" }
    }
  }
}
```

- `provider.<id>.options.baseURL`: endpoint override for any provider
  (proxy/custom endpoint)
- `provider.<id>.blacklist` / `whitelist`: hide/keep models
- `provider.<id>.models`: `{ "<modelId>": { "name": "..." } }` model table
  (required for custom providers)
- `options` can hold provider-specific fields (e.g. Bedrock's
  `region`/`profile`/`endpoint`)
- `disabled_providers` / `enabled_providers`: global provider disable/whitelist
- Top-level `model` / `small_model`: default model and light model
- `experimental.policies`: allow/deny policies on the `provider.use` action

## Custom provider (custom endpoints / local models)

```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "atomic-chat": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Atomic Chat (local)",
      "options": { "baseURL": "http://127.0.0.1:1337/v1" },
      "models": { "<your-model-id>": { "name": "<your-model-name>" } }
    }
  }
}
```

- `npm`: the AI SDK provider package (OpenAI-compatible generally uses
  `@ai-sdk/openai-compatible`)
- `models` ids must match `GET /v1/models`
- Local model support: LM Studio (`http://127.0.0.1:1234/v1`), Ollama, Atomic
  Chat, etc.

## Credentials & switching

- `/connect` stores keys to `~/.local/share/opencode/auth.json`
- Env vars (e.g. `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) also work directly
- Switch = change the `provider` section (baseURL/models) + select the model with
  `/models`; model selection is runtime
- Multiple provider definitions coexist (additive semantics)

## Sources

- `github.com/anomalyco/opencode` @ `v1.18.18` (release-tag anchor; the facts come
  from the official docs pages `opencode.ai/docs/providers/`, `/config/`,
  `/models/`)
