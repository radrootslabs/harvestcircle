# ADR 0002: Nostr library selection

## Status

Accepted and implemented.

## Decision

Use `nostr-sdk` `0.44.1` and its compatible `nostr` `0.44.1` source for key
generation, NIP-19, event/signature verification, and relay client behavior.
The registry release of `nostr` `0.44.1` is yanked, so Cargo pins the upstream
workspace commit `5bba5163eb77107f82c4a8262cf29d7f33a73219`. Keep SDK secret types inside a
narrow adapter guarded by Radroots secret wrappers.

Do not use Notedeck source or nostrdb. Both reviewed references are GPL and the
MVP already requires migration-managed SQLite. Do not adopt the current
`0.45.0-alpha` SDK line without a superseding compatibility ADR.

## Consequences

Local relay tests use the matching `nostr-relay-builder` `0.44.1` fixture and
never require public network access. The direct dependency and license ledger
is maintained in `docs/architecture/reference-research.md`; lockfile or version
catalog changes require renewed compatibility and license review.
