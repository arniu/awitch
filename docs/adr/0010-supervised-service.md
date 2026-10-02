# ADR-0010: Supervised service

- Status: **accepted** (2026-08; amended 2026-08-13).

## Context

After initial setup, daily use is agent-only — the gateway must be available
with zero CLI invocation. The original model — the first CLI command spawns a
detached orphan that lingers until explicitly stopped — broke that promise:
no supervision or crash restart (stderr dropped to null, crashes
undiagnosable); the stop endpoint reported success without stopping; after a
reboot the gateway stayed down until a CLI command ran, so agents hit
connection-refused; SIGTERM was unhandled, so even a manual `kill` was a hard
kill.

Alternatives rejected: keep the lazy-spawn orphan (no supervision, no
restart); idle auto-exit (breaks the agent data plane — agents cannot respawn
the gateway, only the CLI can); a separate `install-service` ceremony
(violates no-ceremony, ADR-0001).

## Decision

1. **One binary, one role.** `serve` (the supervised service) and every other
   command (the control client) are the same executable — not a daemon +
   client like `dockerd`/`docker` or `tailscaled`/`tailscale`. Exactly one
   state-owning process: the service itself (#6).
2. **OS login-session supervisor** launches the gateway at login and restarts
   it on crash (prior art: Tailscale / Ollama / Docker / Homebrew services /
   cloudflared):
   - **macOS** — per-user LaunchAgent.
   - **Linux** — systemd user unit.
   - **Windows** — per-user logon-triggered autostart (Task Scheduler
     `onlogon` task / `shell:startup` shortcut), **not** a Windows Service
     (LocalSystem would resolve `~/.awitch` against the wrong profile).
3. **Every CLI command = control plane**, over a local control API on TCP
   (`127.0.0.1`, dedicated control port), authenticated by a per-install token
   in a 0600 file. Gateway down → the CLI reports it and names the start
   command. The `service` verbs go through the supervisor (launchctl /
   systemctl / schtasks), not the control API, so they work even when the
   running server's version differs from the CLI's.
4. **Data plane** serves agents on `127.0.0.1` (dedicated data port,
   configurable); the app key in the request's credential identifies the
   app for routing. The key is a **bearer credential** — it authorizes spend
   against the user's keys and must be treated as a secret.
5. **Stateless routing** — every request routed independently, no session or
   binding state (ADR-0009).
6. **Single writer** — SQLite is process-exclusive: one process holds the
   database (ADR-0006); WAL is crash-safe, so supervisor restart-on-crash is safe.
   Single-instance additionally enforced by an OS advisory lock held for the
   process lifetime; pidfile and TCP binds are secondary guards.
7. **Lifecycle surface** — install, uninstall, start, stop, restart, status
   and logs. Install registers the unit, enables login-autostart and starts
   now (idempotent); uninstall stops, disables, removes, warning when agents
   are still pointed.
8. **Security posture** (always-on makes this load-bearing): config dir `0700`,
   database (with WAL/shm sidecars) `0600` — vendor keys are plaintext and
   must not be world-readable; the control token is key-equivalent (it reads
   provider records, keys included), stored 0600, compared constant-time.
   Loopback TCP + tokens is accepted (strictly better than unauthenticated
   localhost).

### Pinned values

The restart policy is load-bearing — a wrong value inverts it — so exact keys
are pinned, each carrying the behavioral contract that rides on its values:

- **macOS**: `RunAtLoad = true`; `KeepAlive = { SuccessfulExit = false }`
  (restart on crash, stay stopped after a clean exit — not boolean `true`);
  `ThrottleInterval` for crash-loop backoff. launchd cannot stop a `KeepAlive`
  job while loaded, so `service` verbs act through the pid: the server drains
  on SIGTERM and exits cleanly, and the pinned dict keeps it stopped.
  `restart` = clean stop then `kickstart`; `start` = `kickstart`; `status` =
  supervisor state + health endpoint.
- **Linux**: `Type=exec`, `Restart=on-failure`, `RestartSec=2`,
  `[Install] WantedBy=default.target`; `enable-linger` not used (the gateway
  may stop at logout).
- **Windows**: Task Scheduler `onlogon` task; crash-restart via the task's
  "restart on failure" setting.

`install` is idempotent and doubles as the upgrade path.

## Consequences

- The gateway process runs in the foreground; the supervisor owns its
  lifecycle, logging, and restart.
- Graceful shutdown via SIGTERM/SIGINT: in-flight requests drain, then queued
  writes flush before exit. Stop is a supervisor signal, clean, never a
  restart.
- Logs to stderr, captured by the supervisor (journald on Linux, log file on
  macOS/Windows); the macOS log file is size-capped/rotated.
- The CLI is a pure client.
- Upgrade = a re-install (idempotent) then a restart;
  the health version check prompts this instead of a manual stop-then-restart.
- Windows autostart (logon task) is its own milestone; the least
  cross-platform piece.
