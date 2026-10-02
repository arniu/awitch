# ADR-0011: Import & export

- Status: **deferred** (edge system)

## Context

Moving to a new machine, backing up, or recovering from a disaster needs the
whole database carried over. Import/export is a one-shot copy of a frozen
snapshot: there is no live peer, so there is no divergence to resolve — import
means replace, and the user owns that decision. The peer machinery — per-item
last-write-wins with the losing version preserved — has nothing to
resolve here.

The snapshot is a **physical copy**, not a logical serialization. Two reasons:
the gateway is the single writer of the database (ADR-0006), so only it can
produce a consistent copy and only it can replace its own database; and a
physical copy settles fidelity — a logical dump loses any table it does not
explicitly map.

## Decision

1. **The snapshot is a physical copy of the database** — produced by the
   gateway as a single self-contained file carrying every table — pool, pins,
   settings, app tokens, balance snapshots, ledger — a complete portable state
   for backup and migration.
2. **Overwrite with no negotiation, no pre-import backup** — no lock, no LWW
   check, no conflict negotiation: the user's explicit replace decision wins.
   The snapshot is still validated before adoption (see 4), so
   "unconditional" applies to the destination's current content.
3. **One-shot replace, not sync** — a full snapshot, not a delta, and not the
   sync format; nothing ever merges against it.
4. **Import is a gateway adoption routine** — validate the snapshot read-only
   (integrity check and schema version) before touching the live file; swap
   atomically (close the live database, replace, reopen). A snapshot from an
   older schema version migrates up, a newer one is rejected — no downgrade.
   In-memory state is derived (ADR-0006) and is rebuilt from the imported
   database.
5. **Delivered over the control plane** — a read-only export endpoint and an
   import endpoint, executed by the single-writer gateway. No separate
   importer, no file-level tool outside the gateway.
6. **Machine files do not travel** — the local config (ports and credentials)
   and the control token are machine-local and regenerate on the new machine.

## Consequences

- The snapshot carries plaintext provider keys; docs state the risk.
- Always a full dump — no incremental, so cheap to implement, heavy on large
  histories.
- The snapshot format is the SQLite file format itself — no serialization
  layer to write or to drift from the schema.
- A transfer is verifiable: identical content under a given binary produces
  identical bytes, so a snapshot can be byte-compared against its source.
- After migration, routing, ledger, and tokens carry over as one unit.
- Snapshotting a busy gateway may hold a transient lock — bounded, and
  acceptable for a one-shot administrative operation.
