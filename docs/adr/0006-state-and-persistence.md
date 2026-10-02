# ADR-0006: State & persistence

- Status: **accepted** (grilling session, 2026-08); **amended 2026-09** — per-forward
  observations (`attempts`) are best-effort rows written alongside the ledger

## Context

The gateway's state must survive a restart: agents are pointed once, so a
crash never re-runs pointing. SQLite allows one writer at a time — two
processes writing the same database corrupt it.

## Decision

- **All-SQLite single database**: all gateway state lives in one SQLite
  database.
- DB location supports an env override for the config directory (sandbox
  guardrails); three-platform paths converge to one layout (ADR-0001).
- **One writer**: the gateway process owns the database; WAL for restart
  safety. A second process must fail before it touches the file.
- **The database is the only truth**: everything the gateway holds in memory is
  derived, rebuildable from the database, and never authoritative.
- **Ordered access**: writes are applied in submission order, and a read
  observes every write submitted before it — ordering, not elapsed time,
  defines visibility.
- **Best-effort hot-path rows**: the ledger and the per-forward observations
  are written alongside the response and may land after it; a row that cannot
  be written is dropped with a log. They are accounting and status, never
  correctness.
- **Clean shutdown flushes pending writes** before the process exits.

## Consequences

- All state is recoverable by restarting the process; no live-config recovery.
- A crash may lose hot-path rows that were accepted but not yet written; the
  next start serves exactly what the database holds and derives nothing else.
