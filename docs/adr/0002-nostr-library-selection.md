# ADR 0002: Nostr library selection

## Status

Accepted dependency direction; implementation compatibility is verified when
the Cargo lockfile is introduced.

## Decision

Use stable `nostr` and `nostr-sdk` `0.44.1` for key generation, NIP-19,
event/signature verification, and relay client behavior. Keep SDK secret types
inside a narrow adapter guarded by Radroots secret wrappers.

Do not use Notedeck source or nostrdb. Both reviewed references are GPL and the
MVP already requires migration-managed SQLite. Do not adopt the current
`0.45.0-alpha` SDK line without a superseding compatibility ADR.

## Consequences

Local relay tests may use a controlled WebSocket fixture rather than depending
on alpha-only SDK relay helpers. Dependency versions and licenses are pinned
and audited before final acceptance.
