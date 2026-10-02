# pi agent

> Source: `github.com/earendil-works/pi` @ `v0.84.1`

## Config dir & files

- Global settings: `~/.pi/agent/settings.json`; project settings:
  `.pi/settings.json` (nested-object merge overrides)
- Custom provider/model: `~/.pi/agent/models.json`
- Credentials: `~/.pi/agent/auth.json` (0600); API keys can also go through env
  vars
- Catalog cache: `~/.pi/agent/models-store.json` (offline provider catalog cache)
- Extensions (strongly-typed custom providers): `pi.registerProvider()`
  extensions, see pi docs `custom-provider.md`

## Built-in providers & credential resolution

- OAuth subscription-style: `/login` picks ChatGPT (Codex), Claude Pro/Max,
  GitHub Copilot, xAI, OpenRouter, Radius; tokens stored in `auth.json` and
  auto-refreshed
- API-key style: env vars or auth.json; the mapping table is in pi docs
  `providers.md` (e.g. `ANTHROPIC_API_KEY` → `auth.json["anthropic"]`,
  `OPENAI_API_KEY` → `"openai"`, `GEMINI_API_KEY` → `"google"`,
  `OPENROUTER_API_KEY` → `"openrouter"`, `OPENCODE_API_KEY` → `"opencode"`, 40+
  entries)
- auth.json key values support: literals, `"$ENV_VAR"` interpolation,
  `"!command"` execution (keychain/1password), `"$$"`/`"$!""` escapes;
  per-credential `env` blocks inject provider-level environment
- Resolution order: CLI `--api-key` > auth.json > env vars > models.json custom
  keys

## Custom providers (`~/.pi/agent/models.json`)

```json
{
  "providers": {
    "ollama": {
      "baseUrl": "http://localhost:11434/v1",
      "api": "openai-completions",
      "apiKey": "ollama",
      "models": [{ "id": "llama3.1:8b", "reasoning": false, "contextWindow": 128000 }]
    }
  }
}
```

- `api` options: `openai-completions`, `openai-responses`, `anthropic-messages`,
  `google-generative-ai` (+ `baseUrl` required)
- Model fields: `id / name / reasoning / input[] / contextWindow / maxTokens /
cost{input,output,cacheRead,cacheWrite}`
- Compat switches: `compat.supportsDeveloperRole` /
  `compat.supportsReasoningEffort` (Ollama/vLLM etc.)
- Can override built-in providers (same id overrides `baseUrl` etc.)
- Cloud services: Azure OpenAI (`AZURE_OPENAI_API_KEY` +
  `AZURE_OPENAI_BASE_URL`/`RESOURCE_NAME`), Bedrock (`AWS_PROFILE`/key/
  `AWS_BEARER_TOKEN_BEDROCK`), Cloudflare AI Gateway/Workers AI, Vertex (ADC) —
  each with dedicated env/auth conventions

## Switching

- `pi --provider <id> --model <id>` (CLI-specified)
- `/model`, `/login`, `/logout` (interactive)
- Within the bash tool, `PI_PROVIDER` / `PI_MODEL` / `PI_REASONING_LEVEL` env
  vars reflect the current selection
- Switching does not rewrite config files (runtime selection + auth.json
  credentials); no live config file to modify

## Sources

- `github.com/earendil-works/pi`:
  - `packages/coding-agent/docs/providers.md`
  - `packages/coding-agent/docs/models.md`
  - `packages/coding-agent/docs/environment-variables.md`
  - `packages/coding-agent/docs/settings.md`
  - `packages/coding-agent/docs/custom-provider.md`
