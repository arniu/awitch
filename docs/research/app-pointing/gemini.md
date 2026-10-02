# Gemini CLI

> Source: `github.com/google-gemini/gemini-cli` @ `v0.54.4`

## Config-file hierarchy (source: `github.com/google-gemini/gemini-cli/docs/reference/configuration.md`)

Priority low → high:

1. Hardcoded defaults
2. System defaults file: `/etc/gemini-cli/system-defaults.json` (macOS:
   `/Library/Application Support/GeminiCli/system-defaults.json`; path overridable
   via `GEMINI_CLI_SYSTEM_DEFAULTS_PATH`)
3. User settings: `~/.gemini/settings.json`
4. Project settings: `.gemini/settings.json` (project root, overrides user
   settings)
5. System settings: `/etc/gemini-cli/settings.json` (above all settings files,
   path overridable via `GEMINI_CLI_SYSTEM_SETTINGS_PATH`)
6. Environment variables (auto-loaded from `.env`, see below)
7. CLI args

### `.env` loading (configuration.md)

Load order: current-dir `.env` → walk up parent dirs to project root (a dir with
`.git`) or home → `~/.env`. Variables in `.gemini/.env` are **not** filtered by
`advanced.excludedEnvVars`. settings.json strings support `$VAR` / `${VAR}` /
`${VAR:-default}` env interpolation (e.g. `"apiKey": "$MY_API_TOKEN"`).

## Provider expression: auth method + model setting (no provider registry)

Gemini CLI has **no** `providers: {}` section. The provider (backend/auth) is
decided by environment variables; the model choice is written to settings.json's
`model.name` or `GEMINI_MODEL`. The settings schema is officially hosted at
`schemas/settings.schema.json` (in-repo + `raw.githubusercontent.com`).

### Auth methods (source: `github.com/google-gemini/gemini-cli/docs/get-started/authentication.mdx`)

- **Google account login (OAuth)**: recommended (`gemini` interactive login /
  first-run wizard)
- **Gemini API key**: `GEMINI_API_KEY` (key from Google AI Studio)
- **Vertex AI - ADC**: `GOOGLE_CLOUD_PROJECT` + `GOOGLE_CLOUD_LOCATION` +
  `GOOGLE_APPLICATION_CREDENTIALS` (`gcloud auth application-default login` or a
  service-account key; **must unset `GOOGLE_API_KEY`/`GEMINI_API_KEY` first**)
- **Vertex AI - Google Cloud API key**: `GOOGLE_API_KEY` (+ `GOOGLE_CLOUD_PROJECT`)
  (Vertex express mode)

- Vertex AI requires `GOOGLE_CLOUD_PROJECT` (project ID) and `GOOGLE_CLOUD_LOCATION`
  (e.g. `us-central1`); `GOOGLE_APPLICATION_CREDENTIALS` points at a service-account
  JSON or ADC.
- Headless mode (CI etc.) uses a Gemini API key or Vertex AI.

### Model settings (settings.json / env vars)

```json
{
  "model": { "name": "gemini-3-flash-preview" },
  "modelConfigs": {
    "aliases": {
      "my-alias": {
        "extends": "base",
        "modelConfig": { "generateContentConfig": { "temperature": 0 } }
      }
    },
    "customAliases": {},
    "customOverrides": [],
    "overrides": [],
    "modelDefinitions": {},
    "modelIdResolutions": {},
    "classifierIdResolutions": {},
    "modelChains": {}
  },
  "billing": {
    "overageStrategy": "ask",
    "vertexAi": { "requestType": "dedicated", "sharedRequestType": "priority" }
  }
}
```

- `model.name`: the Gemini model used by the session (source: settings schema +
  `github.com/google-gemini/gemini-cli/docs/cli/settings.md`)
- `modelConfigs`: model aliases (`extends`-able), overrides, model definitions, id
  resolution, classifier resolution, fallback chains
- `billing.overageStrategy`: quota-exhaustion policy (`ask`/`always`/`never`)
- `billing.vertexAi`: Vertex request-type headers

### Model-selection priority (source: `github.com/google-gemini/gemini-cli/docs/cli/model-routing.md`)

1. `--model` CLI flag
2. `GEMINI_MODEL` env var
3. `model.name` (settings.json)
4. Local Gemma model routing (experimental)
5. Default `auto` (auto-selects between Pro/Flash by task complexity)

`/model` switches in-session; Auto (Gemini 3 / 2.5) or Manual picks a specific
model. On model failure, `ModelAvailabilityService` falls back per policy (enabled
by default, prompts the user on failure).

### Local models (experimental)

`gemini gemma setup` (downloads LiteRT runtime + Gemma models + config + start)
enables local model routing; `gemini gemma status / start / stop / logs`, in-session
`/gemma`. Local endpoints configured via `experimental.gemmaModelRouter`.

## Credentials

- `GEMINI_API_KEY` / `GOOGLE_API_KEY` and other secrets usually live in `.env`
  (`~/.gemini/.env` or project `.env`) or the shell profile; settings.json can
  reference them via `$VAR` interpolation.
- Google-account OAuth credentials are managed by the CLI
  (`code_assist/oauth-credential-storage`).
- `GEMINI_CLI_HOME`: user-level config root (default `~`, `.gemini/` created under
  it); handy for isolating test environments.

## Switch notes

- **Switching provider (backend) = switching auth**: change env vars
  (`GEMINI_API_KEY` ↔ `GOOGLE_API_KEY` ↔ ADC); note **ADC and API keys are
  mutually exclusive** (unset the key vars before ADC); `GOOGLE_CLOUD_PROJECT`
  can be overridden in `.env` (Cloud Shell scenario).
- **Switching model = write `model.name` (settings.json) or set `GEMINI_MODEL`**;
  `GEMINI_MODEL` outranks settings, so write settings' `model.name` when switching
  (same semantics as `--model`, persistent).
- Hierarchy note: project `.gemini/settings.json` overrides user settings; after
  writing user settings, same-named project keys win.
- No doctor-like command; verify a switch with interactive start / `/model` or
  `gemini -p "..."`.

## Sources

- `github.com/google-gemini/gemini-cli`:
  - `docs/reference/configuration.md` (hierarchy, `.env` loading, env table,
    modelConfigs)
  - `docs/get-started/authentication.mdx` (auth methods)
  - `docs/cli/settings.md` (settings reference, `/settings` command)
  - `docs/cli/model.md`, `docs/cli/model-routing.md` (`/model`, selection
    priority, fallback)
  - `docs/core/gemma-setup.md` (local model routing)
  - `schemas/settings.schema.json` (settings JSON schema)
- Note: no local gemini CLI install; no same-version local verification; all
  facts come from the upstream repo.
