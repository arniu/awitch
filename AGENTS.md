# AGENTS.md

## Positioning

Awitch is a personal, local AI gateway.

## Sources of truth

Truth flows upstream: `CONTEXT.md` defines terms, `docs/adr/` decides, code
conforms. Before writing code or docs, check `CONTEXT.md` for every term the
change uses and `docs/adr/` for the ADR governing each decision — follow the
index, `docs/adr/README.md`. Where they conflict, fix the code, not the
decision.

## Cohesion

One law for everything: **A unit is about its own scope, and only that**.
Anything beyond it — a second subject, or content that no longer holds — is
split into a unit of its own, or deleted.

## Before editing

A change stays inside the request's boundary. When it would add a type or
public API, reach outside the module named, or span more than three files,
state the plan first — the files you will touch, the minimal change in each,
and what you will not touch — then wait for approval. A change that needs an
edit outside that plan stops and asks.

## Comments

First principle: **the code speaks for itself.** Write a comment only when
explicitly asked, and only where the code cannot be understood without it:
an algorithm's steps, background knowledge, a decision's rationale.

**How to write:**

- only _why_ and the contract, never _what_, _how_, or the implementation;
- stay consistent: the comment agrees with the code it refers to.

## Isolation

Before any test or command that writes app config (pointing), isolate it
with temp dirs and env overrides — pointing at the real agent config
rewrites the user's pointing state.

## Commits

Leave every change in the working tree; commit only when explicitly asked.
Report the change and whether `./script/check` is green.

[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):
`<type>(<scope>): <subject>` — the types and the breaking-change marker come
from the spec. Pinned here:

- `<scope>`: the modules the change belongs to — `fix(db, server): …`. Omit it
  only when no module owns the change.
- `<subject>`: what the change achieves, lowercase, no trailing period.

## Blind review

A change set going out for independent review — the reviewers, the rounds,
and when to escalate: `docs/blind-review.md`.
