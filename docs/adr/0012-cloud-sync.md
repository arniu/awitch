# ADR-0012: Cloud sync

- Status: **deferred** (edge system)

## Context

A single user runs awitch on several machines and expects the same routing
policy everywhere. The pool is worth carrying across machines; the ledger and
tokens are not. Sync has live peers — both sides evolve between syncs — so a
conflict is not a negotiation between users, but neither side may be lost.

Ruling test: **"would the user do it twice?"** — what one user sets up once
and expects on every machine is user config; what a machine derives by itself
is machine state. Router policy (mode, balance threshold, per-app pins) is the
"set it up once" kind; the ledger, balance history, and tokens are usage
artifacts, local to the machine that produced them.

## Decision

1. **Sync "user config", keep "machine state" local.**
   - Synced: the provider pool; the per-app pins; the settings (mode, balance
     threshold).
   - Device-local, never synced: ledger, balance snapshots, app tokens (ports
     live in the never-synced local config, #4).
   - Pins reference provider ids in the same synced set: within a cycle the
     pool applies before pins, and a pin referencing an absent provider is
     **dangling** — it keeps pin semantics (an empty pinned candidate fails
     the route), never silently dropped.
2. **Resolve per item, never lose.** Each synced item — one provider, one pin
   row, one setting key — fast-forwards when only one side changed it; both
   changed → last-write-wins keeps the newer version, the losing version is
   preserved (nothing deleted) and the user is told. Disjoint changes on both
   sides survive.
3. **Incremental, transactionally** — a cycle transfers only what changed
   since the previous one and advances its bound only on a fully successful
   cycle (pull, merge, push). A partial cycle retries from the unadvanced
   bound — a change is never dropped (#2).
4. **Sync-transport config is meta-config** — it cannot be synced (you need
   the credentials to fetch the sync). It lives in the local config file
   beside the ports (ADR-0006), with the transport credentials. The local
   config never syncs; the synced data is purely user config.
5. **Plaintext over HTTPS.**
6. **Two transports: WebDAV and S3.**
7. **Automatic background sync** — a task in the gateway process, each cycle
   pulling remote changes and pushing local (#3). An explicit trigger may
   survive as an escape hatch, but is not the interface.

## Consequences

- Sync converges only the "should agree" subset — orthogonal to
  import/export (ADR-0011): sync never carries device-local state, while
  import/export carries everything once.
- Routing policy travels with the pool; a fresh machine matches the source's
  routing decisions without reconfiguration.
- Deterministic serialization underpins diffing and LWW detection.
- The sync payload carries plaintext keys; docs state the risk.
- Configured once, machines converge automatically: the surface is transport
  configuration (WebDAV/S3), not manual transfer steps.
