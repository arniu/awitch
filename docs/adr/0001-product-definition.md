# ADR-0001: Product definition

- Status: **accepted** (grilling session, 2026-08)

## Context

Coding agents (Claude Code, Codex, …) hold a single vendor in their native
config, and nothing routes across vendors: every request goes to whichever
vendor the config names. An expensive, slow, or failing vendor cannot be
avoided — it is the only option. Switching vendors means hand-editing every
agent's config and juggling keys.

The user is a single developer driving agents from the terminal, holding
keys for several vendors, weighing cost and speed, and treating keys as
credentials that stay on the machine.

Manual switchers (cc-switch) swap per-app configs but do not route. Server
proxies (LiteLLM) need deployment. Managed gateways (Portkey, Cloudflare)
relay the user's keys and traffic; aggregators (OpenRouter) resell access.

## Decision

awitch is a personal, local gateway: the user points each agent at it once,
and it routes every request to the cheapest or fastest vendor across the
user's own keys, which never leave the machine. It is not a team proxy, not
a SaaS, not a key reseller.

### Product boundaries

- **Non-invasive** — never touch agent runtime. The only interface is each
  agent's native config file (one-time `point`). Zero plugins, hooks, wrappers.
- **Local only** — single binary, single user, keys stored in local SQLite.
  No web UI, no multi-user, no Docker deployment.
- **No content classifier** — routing uses app type + model name, not
  request-content analysis.

### Platform & form

1. **CLI is the only form.** No GUI, no TUI, no web dashboard. The user is a
   CLI-agent user; the control surface is the terminal.
2. **macOS, Linux, and Windows are first-class.** CI matrix covers all three.
   Cross-platform config paths on all three; no "best-effort Windows."
   Shell completions include PowerShell.

## Consequences

- No billing, no refunds, no org metering.
- The CLI is the only surface, and its help is authoritative for commands.
- Dependencies must be cross-platform.
- Config dirs converge to one layout across platforms; docs state per-platform
  paths.
- Keys live in awitch's own SQLite; no OS keychain integration.
