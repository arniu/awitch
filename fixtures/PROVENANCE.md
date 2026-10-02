# Provenance

Every file in this directory except `sources.toml` is fetched or derived by
`cargo xtask fixtures sync` from the revisions `sources.toml` names; a
fixture is never written by hand. `CHECKSUMS.sha256` carries each vendored
file's digest, and `licenses/` carries each source's LICENSE notice.

The corpus is a dated snapshot of those revisions, not a canon: its date is the
date of the commit that last changed a fixture, and a revision bump is a
reviewable commit rather than a download. `kind` records the evidence grade,
strongest first.

| Kind | Evidence |
|---|---|
| `recorded` | bytes the upstream SDK captured from the live API |
| `synthetic` | bytes the upstream authored, with fabricated ids |
| `spec-schema` | a `required` list the published spec carries, extracted and never transcribed |
| `spec-example` | an example the published spec carries at the revision its source names |
