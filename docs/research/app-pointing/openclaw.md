# OpenClaw

> Source: `github.com/openclaw/openclaw` @ `v2026.7.1-2`

## Config file

`~/.openclaw/openclaw.json` (JSON5). Structure:

```json5
{
  models: { mode: "merge", providers: {} },
  agents: { defaults: { model: { primary: "anthropic/claude-opus-5" } } },
}
```

## Provider definition: `models.providers.<id>`

```json5
{
  models: {
    mode: "merge",           // merge: built-in catalog + custom entries; replace: explicit entries only
    providers: {
      moonshot: {
        baseUrl: "https://api.moonshot.ai/v1",
        apiKey: "${MOONSHOT_API_KEY}",     // env interpolation supported
        api: "openai-completions",         // protocol, see below
        models: [{ id: "kimi-k2.6", name: "Kimi K2.6", contextWindow: 262144 }],
        headers: { "X-Custom": "..." },    // extra headers
        compat: { ... }                    // capability declaration (custom/proxy endpoints)
      }
    }
  }
}
```

- `api` values (source: `github.com/openclaw/openclaw/docs/gateway/config-tools.md`): `openai-completions`,
  `openai-responses`, `openai-chatgpt-responses`, `anthropic-messages`,
  `google-generative-ai`, `google-vertex`, `github-copilot`,
  `bedrock-converse-stream`, `ollama`, `azure-openai-responses`; with only
  `baseUrl` set (no `api`), defaults to `openai-completions`
- Model entries support `id / name / reasoning / input[] / cost{input, output,
cacheRead, cacheWrite} / contextWindow / contextTokens / maxTokens` (source:
  `github.com/openclaw/openclaw/docs/gateway/config-tools.md` model entry schema)
- Official provider plugins ship a built-in catalog; `models.providers` is for
  **custom providers or overriding base URL / model metadata**
- Model reference format: `provider/model` (e.g. `opencode/claude-opus-4-6`,
  `google/gemini-3.1-pro-preview`)

## Default model & fallbacks

- `agents.defaults.model.primary` (or string form `agents.defaults.model`)
- `agents.defaults.model.fallbacks` (ordered fallbacks)
- `agents.defaults.utilityModel` / `imageModel` / `pdfModel` / `mediaModels`
  (auxiliary models)
- `agents.defaults.models["provider/model"]` (aliases and per-model params, e.g.
  `params.transport`, `params.serviceTier`, `params.fastMode`,
  `params.cachedContent`)
- `agents.defaults.modelPolicy.allow` (optional model whitelist, `provider/*`
  prefix wildcard supported)

## Credentials

- Env vars are primary: `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`,
  `OPENCODE_API_KEY`, `ZAI_API_KEY`, `MOONSHOT_API_KEY`, `DEEPSEEK_API_KEY`, etc.
- Multi-key rotation: `<PROVIDER>_API_KEYS` (comma/semicolon list),
  `<PROVIDER>_API_KEY_1/2...`, `OPENCLAW_LIVE_<PROVIDER>_KEY` (highest priority)
- Auth profiles: `auth.profiles.<id>` (`api_key`/`token`/`oauth`, supports
  SecretRef, `expires`, explicit `auth.order`); stored credentials only allow
  api_key/token/oauth
- External CLI credential discovery: `claude-cli` (reads Claude's key), `openai`
  (reads Codex's key), etc.
- Probing: `openclaw models status --probe`, `openclaw doctor`

## Switch commands

`openclaw onboard` (wizard), `openclaw models list`,
`openclaw models set <provider/model>`, `openclaw models auth login --provider
<id> [--set-default]`, `openclaw configure`, `openclaw doctor --fix`

## Switch notes

- Additive semantics: all providers coexist; switch = change
  `agents.defaults.model.primary` (and/or fallbacks).
- Adding provider auth does not touch the existing primary (unless
  `--set-default` / `models set`).

## Sources

- `github.com/openclaw/openclaw`:
  - `docs/concepts/model-providers.md`
  - `docs/concepts/models.md`
  - `docs/auth-credential-semantics.md`
  - `docs/gateway/config-tools.md` (`models.providers.*.api` protocol table,
    compat declaration)
