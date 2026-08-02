# Nostr accounts architecture

An import that resolves to an account with an existing credential returns
`AccountAlreadyExists` and does not overwrite either resource. The sole repair
path is an existing account explicitly marked `CredentialMissing` with no
credential present; matching import restores that credential, changes the
public availability state to `Available`, and retains the original metadata.

## Status

Implemented MVP architecture.

## Ownership

Rust `AppCore` owns the saved-account registry, selected account, active
signer/session, persistence, recovery, relay/profile work, immutable snapshots,
and safe errors. Kotlin is a thin lifecycle and presentation shell over UniFFI.
`PersistentAppCore` composes the application ports with one SQLite `Database`;
the FFI `StudioAppCore` adds the OS credential adapter, clock, SDK relay client,
and observer ownership. `RadrootsApplication` creates one native core and one
`StudioAppStore` for the application composition and closes both on disposal.

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

Generate and import durably create and select a signed-out account. Import
accepts nsec or canonical secret-key hex and rejects duplicates without
overwriting credentials. A saved account explicitly marked
`CredentialMissing` is the only repair exception. Removal requires a
revision-bound, single-use confirmation token so stale UI confirmation cannot
delete a changed target.

Public state revisions increase only when public state changes. Observers get
the current snapshot at subscription and later revisions after locks are
released. A relay completion is bound to its initiating active public key; a
sign-out or replacement makes the completion stale and prevents publication.

## Data partitioning

SQLite stores public account metadata, selected public key, verified kind-zero
profile cache, account-owned typed preferences, migration state, and the
non-secret recovery journal. Public Nostr events are keyed by event ID and
author. Account-private values are keyed by owner public key and cascade on
removal. Secret keys are never stored in SQLite.

The production database is exactly
`ProjectDirs::from("org", "radroots", "studio").data_dir()/studio.sqlite3`.
The `directories` crate maps that base to the platform application-data area;
tests inject temporary or in-memory databases instead.

## Relay and profile flow

Relays are global WebSocket endpoints from `RADROOTS_NOSTR_RELAYS`. Activation
loads cached profile state. Refresh then queries kind-zero events using the
production SDK adapter, verifies event ID, signature, author, kind, bounds, and
metadata, and selects newest timestamp with lowest event ID as the tie-breaker.
Offline, timeout, or invalid data retains cache and produces a nonfatal state.

Remote relays require `wss://`. Plain `ws://` is restricted to localhost,
`127.0.0.0/8`, and `::1`. Development mode falls back to
`ws://localhost:8080`; packaged mode fails safely if no valid relay is
configured. Tests use only ephemeral loopback relays or controlled fakes.
