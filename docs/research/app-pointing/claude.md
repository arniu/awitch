# Claude Code

> Source: `github.com/anthropics/claude-code` @ `v2.1.226`

## Config-file hierarchy

Primary sources: `anthropics/claude-code` repo `examples/settings/README.md`, plus
`claude --help`.

- Enterprise (MDM-managed, highest priority): `managed-settings.json`, path via
  `CLAUDE_CODE_MANAGED_SETTINGS_PATH`
- User: `~/.claude/settings.json`
- Project: `.claude/settings.json`
- Project-local (self only): `.claude/settings.local.json`
- Session: `--settings <file-or-json>` (extra load), `--setting-sources user,project,local`
- Session state/history: `~/.claude.json`

## Provider expression: environment variables (no provider registry)

Claude Code has **no** `providers: {}` section. Providers are expressed entirely
through environment variables, and settings.json's `env` block injects variables
into a scope — that is the standard third-party provider entry point.

Key env vars (`v2.1.226`, verified against a same-version local install):

- `ANTHROPIC_API_KEY`: first-party API key
- `ANTHROPIC_AUTH_TOKEN`: Bearer token (preferred for third-party gateways/proxies)
- `ANTHROPIC_BASE_URL`: API endpoint (point at an Anthropic-compatible gateway)
- `ANTHROPIC_MODEL`: primary model
- `ANTHROPIC_SMALL_FAST_MODEL`: light model
- `ANTHROPIC_DEFAULT_OPUS/SONNET/HAIKU/FABLE_MODEL` (+ `_NAME`/`_DESCRIPTION`/
  `_SUPPORTED_CAPABILITIES`): remap the "opus/sonnet/haiku/fable" aliases to
  third-party model names
- `ANTHROPIC_CUSTOM_HEADERS`: extra request headers
- `ANTHROPIC_CONFIG_DIR`: config dir override
- `CLAUDE_CODE_USE_BEDROCK` / `CLAUDE_CODE_USE_VERTEX` /
  `CLAUDE_CODE_USE_FOUNDRY`: switch to AWS Bedrock / Google Vertex / Microsoft
  Foundry auth
- `CLAUDE_CODE_USE_ANTHROPIC_AWS` / `CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD`:
  Anthropic-hosted AWS/Google endpoints
- `ANTHROPIC_BEDROCK_BASE_URL` / `ANTHROPIC_VERTEX_BASE_URL` /
  `ANTHROPIC_FOUNDRY_BASE_URL`: per-cloud endpoint overrides
- `ANTHROPIC_ENVIRONMENT_ID` / `ANTHROPIC_WORKSPACE_ID` /
  `ANTHROPIC_ORGANIZATION_ID`: enterprise routing

## Real example (local `~/.claude/settings.json`, redacted)

This is the current practice for pointing Claude Code at DeepSeek's
Anthropic-compatible endpoint:

```json
{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "sk-...",
    "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "deepseek-v4-flash",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1M]",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "deepseek-v4-pro",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-v4-pro",
    "ANTHROPIC_MODEL": "deepseek-v4-flash"
  }
}
```

i.e. **base URL (Anthropic-compatible endpoint) + bearer token + model-alias
remapping** is enough to switch.

## Auth & switch commands

- `claude auth login / logout / status` (first-party OAuth / API key)
- `/model` (in-session model pick), `--model <alias|full-name>` (e.g.
  `fable`/`opus`/`sonnet` or full name)
- `claude import <codex|gemini>` (built-in "import config from another agent",
  incl. `--dry-run`)
- `--bare` forces `ANTHROPIC_API_KEY` or `apiKeyHelper` only; no OAuth/keychain

## Switch notes

- Switching provider = rewriting the settings.json `env` block and setting
  `ANTHROPIC_MODEL` / `ANTHROPIC_DEFAULT_*_MODEL` remaps; prefer
  `ANTHROPIC_AUTH_TOKEN` (third-party gateway) over `ANTHROPIC_API_KEY` (1P).
- One effective provider per scope (switch semantics).
- Note: `ANTHROPIC_BASE_URL` pointing at a non-Anthropic host disables Remote
  Control (confirmed in official CHANGELOG); switching to a third-party gateway
  loses that capability.

## Official docs & network notes

- The official docs site `code.claude.com` (all `docs.anthropic.com` claude-code
  pages redirect there) is TCP-blocked on this network. Facts cross-verified via:
  upstream repo source (`v2.1.226`), `CHANGELOG.md`, local real config, CLI help.
  The env var list was additionally verified one-by-one on the local v2.1.226
  install (same version as the upstream tag; install used for verification only).
- CHANGELOG-backed key points:
  - LLM gateway auth = `ANTHROPIC_AUTH_TOKEN` + `ANTHROPIC_BASE_URL` (background
    tasks reuse the same auth)
  - `/login` supports Anthropic-operated public gateway endpoints; custom gateways
    support model discovery
  - 3P auth switches: `CLAUDE_CODE_USE_BEDROCK` / `_VERTEX` / `_FOUNDRY`;
    `ANTHROPIC_BEDROCK_REGION_PREFIX` controls Bedrock regions
  - `ANTHROPIC_BASE_URL` pointing at a non-Anthropic host disables Remote Control
    (consistent with the 3P switches)
  - Org default models: set in the org console, shown as "Org default" in `/model`

## Sources

- `github.com/anthropics/claude-code`:
  - `examples/settings/README.md` (settings hierarchy)
  - `CHANGELOG.md` (gateway / 3P provider behavior)
- `claude --help` / `claude import --help` / `claude auth --help` (CLI usage text)
- Local `~/.claude/settings.json` (real config example, redacted, structure
  confirmation only, not a source)
- Note: `code.claude.com` / `docs.anthropic.com` unreachable on this network (TCP
  block, not transient); the primary sources above stand in.
