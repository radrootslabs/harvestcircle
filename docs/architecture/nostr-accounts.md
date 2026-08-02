# Nostr accounts architecture

## Status

Initial architecture contract. Update this document as each implemented
checkpoint makes paths and behavior concrete.

## Ownership

Rust `AppCore` owns the saved-account registry, selected account, active
signer/session, persistence, recovery, relay/profile work, immutable snapshots,
and safe errors. Kotlin is a thin lifecycle and presentation shell over UniFFI.

## Identity and state

Account identity is canonical lowercase Nostr public-key hex. Npub is a display
encoding, and an optional local label is not identity. Saved, selected, and
active are separate states. Startup restores saved accounts and selection but
starts signed out.

Npub and nsec domain values enforce their expected NIP-19 shape. The Nostr
adapter performs checksum and key conversion validation. Nsec remains redacted
and is exposed only in the import adapter or one-time generated-key receipt.

Activation prepares a candidate signer/session before atomically replacing the
working session. Sign out retains the saved account and credential. Confirmed
removal deletes account-private state and credential, then chooses a
deterministic selected fallback without activating it.

## Data partitioning

Public Nostr events may be cached by event ID and subject pubkey. Local account
metadata and typed private preferences are keyed by owner pubkey. Secret keys
are never stored in SQLite.

## Relay and profile flow

Relays are global WebSocket endpoints from `RADROOTS_NOSTR_RELAYS`. Activation
emits cached profile data before a verified kind-0 refresh. Offline or invalid
relay data retains the cache and produces a nonfatal state.
