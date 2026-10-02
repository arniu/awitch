# ADR index

> Status: `accepted` = in force; `deferred` = decided, not shipped (out of
> scope for now).
>
> Numbering encodes the source chain: lower numbers sit further upstream
> (authoritative sources); higher numbers consume them.

| #    | Title                                              | Status   |
| ---- | -------------------------------------------------- | -------- |
| 0001 | Product definition                                 | accepted |
| 0002 | App set                                            | accepted |
| 0003 | Vendor set                                         | accepted |
| 0004 | Provider abstraction                               | accepted |
| 0005 | Translation scope                                  | accepted |
| 0006 | State & persistence (all-SQLite)                   | accepted |
| 0007 | Pricing & accounting (pricing + ledger)            | accepted |
| 0008 | Data asset (sourcing & verification)               | accepted |
| 0009 | Routing (per-request, stateless, cost-aware)       | accepted |
| 0010 | Supervised service                                 | accepted |
| 0011 | Import & export (full-db snapshot)                 | deferred |
| 0012 | Cloud sync (user-config convergence, LWW + backup) | deferred |
| 0013 | migration (source-specific provider import)        | accepted |

---

## Writing an ADR

Form: `Status` → `Context` (the problem and its constraints — not the
solution) → `Decision` (the choice) → `Consequences` (the resulting context
— positive, negative, and neutral). Status is the only header bullet; the
rest are `##` sections.

The decision is recorded; its artifact is not. Field lists, key names, CLI
shapes, schedules and algorithm steps live in the code — restating them here
only grows a copy that rots. Exception: a value may be pinned when a wrong
one inverts the decision.

A set an ADR decides on is stated by its rule and its one source of truth,
never re-copied as a roster — a copied roster drifts from the source. A term
`CONTEXT.md` defines is named, not restated.

A new ADR's number places it in the source chain: downstream of the ADRs it
cites, upstream of the ADRs that cite it — cite upstream only.
