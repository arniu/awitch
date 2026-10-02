# Hermes Agent

> Source: `github.com/NousResearch/hermes-agent` @ `v2026.8.3`

## Config files

- `~/.hermes/config.yaml` (YAML, main config; config version v33, `hermes doctor`
  reports at v2026.8.3)
- `~/.hermes/.env` (dotenv, secrets: API keys etc.; `.env` beats the
  corresponding config setting)
- Config-dir control: `HERMES_HOME` env var (`cli-config.yaml.example` and
  `providers/README.md` both mention `$HERMES_HOME/...`)

## Provider definition

Primary source: `cli-config.yaml.example` + `providers/README.md` + local real
config.

```yaml
model:
  default: "deepseek-v4-flash" # default model (key `default` or `model` both work)
  provider: "deepseek" # see the built-in provider id table below
  base_url: "https://api.deepseek.com"
  # api_key: "..."                    # optional: inline (otherwise read from .env)
  # context_length: 131072            # context window (auto-probed if unset)
  # max_tokens: 8192                  # output cap
  # default_headers: {...}            # custom request headers (overrides OpenAI SDK defaults etc.)
  # extra_headers: {...}              # alias, merged into default_headers
```

`model.provider` values (built-in provider ids, from upstream
`cli-config.yaml.example`): `auto`, `openrouter`, `nous` (Nous Portal OAuth),
`nous-api`, `anthropic`, `openai-codex`, `copilot`, `gemini`, `zai`,
`kimi-coding`, `minimax`, `minimax-cn`, `huggingface`, `nvidia`, `xiaomi`,
`arcee`, `ollama-cloud`, `deepinfra`, `kilocode`, `ai-gateway` (Vercel AI
Gateway), `azure-foundry`, `lmstudio`, `custom` (any OpenAI-compatible endpoint;
`ollama`/`vllm`/`llamacpp` aliases all map to custom).

Custom providers have two forms (source: `cli-config.yaml.example` and
`hermes_cli/config.py`):

**New format `providers:`** (keyed map, the form in the example config):

```yaml
providers:
  my-proxy:
    base_url: "https://llm.internal.example.com/v1"
    key_env: "MY_PROXY_API_KEY"
    # optional: extra_headers / request_timeout_seconds / stale_timeout_seconds / models
```

**Legacy format `custom_providers:`** (list, legacy on-disk form, normalized to
the same view at runtime):

```yaml
custom_providers:
  - name: openrouter
    base_url: https://openrouter.ai/api/v1
    api_key: sk-or-...
    model: anthropic/claude-opus-4-7
    models:
      anthropic/claude-opus-4-7:
        context_length: 200000
```

Entry fields (`hermes_cli/config.py` `_normalize_custom_provider_entry`): `name`,
`api`/`url`/`base_url`, `api_key`, `key_env` (alias `api_key_env`), `api_mode`,
`transport`, `model`/`default_model`, `models`, `context_length`,
`rate_limit_delay`, `request_timeout_seconds`, `stale_timeout_seconds`,
`discover_models`, `extra_body`, `extra_headers`, `ssl_ca_cert`, `ssl_verify`,
etc.; camelCase aliases accepted (`apiKey` / `baseUrl` / `apiMode` / `keyEnv` /
`apiKeyEnv`).

## Provider plugin system (`plugins/model-providers/<name>/`)

Each provider is a `ProviderProfile` plugin (built-in in the repo + user overrides
in `$HERMES_HOME/plugins/model-providers/`), registry lazily loaded in
`providers/__init__.py`. A profile carries: `env_var`, `base_url`, `api_mode`
(`chat_completions` / `anthropic_messages` / `codex_responses` /
`bedrock_converse`), `fetch_models()` (live `/v1/models` pull), message
preprocessing, `extra_body`, etc.

## Credentials & switching

- Secrets in `~/.hermes/.env` (e.g. `ANTHROPIC_API_KEY=...`,
  `OPENROUTER_API_KEY=...`)
- `hermes model` (interactive provider + default model pick, live model-catalog
  pulls)
- `hermes auth` (credential pool management), `hermes login / logout`,
  `hermes fallback` (fallback provider)
- `hermes config set <section.key> <value>` / `get / unset / show / check /
migrate` (CLI config edits)
- Additive semantics: all provider definitions coexist; switch = change
  `model.provider` + `model.default` (+ base_url).

## Real example (local `~/.hermes/config.yaml`, redacted)

```yaml
model:
  default: deepseek-v4-flash
  provider: deepseek
  base_url: https://api.deepseek.com
agent:
  max_turns: 150
  reasoning_effort: medium
```

## Sources

- `github.com/NousResearch/hermes-agent`:
  - `cli-config.yaml.example`
  - `providers/README.md`
  - `plugins/model-providers/README.md`
- Local `~/.hermes/config.yaml` (real config example, redacted, structure
  confirmation only, not a source)
- `hermes model --help` / `hermes config --help` / `hermes doctor` (CLI usage
  text)
