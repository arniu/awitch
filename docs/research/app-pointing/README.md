# App pointing research

Documents how each AI agent stores its model-provider config and where the
provider switch write point is.

## Layout

```
docs/research/app-pointing/
├── README.md
└── <app>.md               # one file per agent
```

## Index

- **[Claude Code](claude.md)**: settings.json `env` block + `ANTHROPIC_*` env vars
  (switch-type)
- **[Codex](codex.md)**: config.toml `model_provider` pointer +
  `[model_providers.<id>]` (switch-type)
- **[Gemini CLI](gemini.md)**: auth env vars (`GEMINI_API_KEY` / Vertex) +
  settings `model.name` (switch-type)
- **[Hermes Agent](hermes.md)**: config.yaml `model` section +
  `providers` (coexistence)
- **[OpenClaw](openclaw.md)**: `models.providers.<id>` +
  `agents.defaults.model.primary` (coexistence)
- **[OpenCode](opencode.md)**: `provider.<id>` section + npm package
  (coexistence)
- **[pi](pi.md)**: `models.json` + `auth.json` (runtime selection)

## Core findings

### Three switch semantics

- **Switch-type** (Claude Code / Codex / Gemini CLI): one effective provider in
  config; switching = overwrite the effective value.
- **Coexistence** (OpenCode / OpenClaw / Hermes): provider definitions coexist;
  switching = change the default selection.
- **Runtime selection** (pi): config files untouched; selection via CLI args /
  `/model`.

### General points

- **Switching writes two places**: the provider definition + the "current"
  pointer/default model; missing either, it does not take effect.
- **The Anthropic-compatible endpoint (`/anthropic`) is the de-facto standard for
  third-party switching**: `ANTHROPIC_BASE_URL` + `ANTHROPIC_AUTH_TOKEN` + model
  remapping connects any vendor (DeepSeek, Kimi, ...); Hermes / OpenClaw / pi
  also support the `anthropic-messages` protocol.
- **Credentials stay separate from config**: secrets live in their own locations
  (auth.json / .env / env vars); do not mix them into switch writes.

## Maintenance conventions

### Adding an agent

1. Create `docs/research/app-pointing/<app>.md` covering the same ground as the
   existing files: config layout → provider definition → credentials → how the
   switch works → sources. Headings follow each agent's real structure (e.g.
   Claude has no provider registry, so it is "Provider expression").
2. Add an entry to the index above.

### Source rules

- Cite the **upstream repo pinned to the latest stable release tag** (e.g.
  `openai/codex@rust-v0.147.0`) or an official doc URL; **no nightly / preview /
  beta anchors** (transient, untraceable).
- Local installs count only as same-version verification; local real configs only
  as structure confirmation (redacted), never as a source.
- Known network limits: `code.claude.com` (TCP blocked) and
  `developers.openai.com` (regional 403) unreachable; the Claude / Codex entries
  rely on upstream repo source / CHANGELOG instead.
