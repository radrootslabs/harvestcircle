# Nostr account runtime reference research

## Status

Baseline recorded on 2026-08-02 before runtime implementation. The dependency
and source audit will be reconciled again before final acceptance.

## Local baseline

- Repository: standalone `oss/studio_app` capsule.
- Branch: `master`.
- Baseline commit: `9cd07ced24d62eca4cc89f09c82a3539c7fe9dcf`.
- Runtime: Kotlin JVM 21 and Compose Desktop.
- Current proof: UUID account IDs, user-entered display names, HTTP/HTTPS server
  URLs, Kotlin `LoginStatus`, and a Kotlin-owned reducer/store.
- Current persistence, credential store, relay client, Rust core, and FFI: none.
- Baseline verification: `./gradlew --no-daemon :app:desktop:test` passed.

## Reviewed references

### Nostr protocol

- Repository: `nostr-protocol/nips`.
- Commit: `c53877571f96eb423661fc23c620d629d37b8f19`.
- Paths: `01.md`, `19.md`.
- Adopt: lowercase 32-byte public-key hex in protocol/storage, WebSocket relay
  transport, kind-0 metadata, signed-event verification, replaceable-event
  ordering, and npub/nsec as human-facing encodings.

### Amethyst

- Repository: `vitorpamplona/amethyst`.
- Commit: `bf41e75b78b3891e5434baa4e5778a93988ba2ca`.
- Reviewed paths: `README.md`, `docs/secure-key-storage-migration.md`,
  `docs/plans/archive/2026-04-23-feat-desktop-multi-account-support-plan.md`,
  `docs/plans/archive/2026-04-28-multi-account-testing-sheet.md`,
  `docs/plans/archive/2026-05-14-fix-account-security-hardening-plan.md`,
  JVM `SecureKeyStorage.kt`, `AccountManager.kt`,
  `DesktopAccountStorage.kt`, and their account/keyring tests.
- Adopt: pubkey identity, multiple saved accounts, explicit corruption state,
  account-scoped resources, and transition tests for missing credentials,
  logout, and account replacement.
- Reject: encrypted ordinary-file secret fallback, publishing in-memory cache
  before durable writes, mutating the replacement session before cleaning the
  captured old session, swallowed deletion failures, and silent conversion of
  a missing local credential into a read-only account.

### Notedeck

- Repository: `damus-io/notedeck`.
- Commit: `b41ffeb57636f9de3147803c1313d99dda6cffa2`.
- Reviewed paths: `README.md`, `LICENSE`,
  `crates/notedeck/src/account/accounts.rs`,
  `crates/notedeck/src/account/cache.rs`,
  `crates/notedeck/src/storage/account_storage.rs`,
  `crates/notedeck/src/storage/keyring_store.rs`, and
  `crates/notedeck/src/user_account.rs`.
- License: GPL-3.0-or-later. Architectural comparison is clean-room only; no
  source is copied.
- Adopt conceptually: pubkey-keyed account cache, selected-account persistence,
  in-memory credential fakes, and account-specific resources.
- Reject: constructors that permit secrets on disk, file-first deletion without
  recovery, cache mutation before storage success, a synthetic fallback
  account, hash-map-order selection, and cloneable secret-bearing account types.

### rust-nostr and nostr-sdk

- Repository: `nostrdevkit/nostr`.
- Commit reviewed: `7834dd624dcc8bdce9610988eb0b5e888b73c2eb`.
- Reviewed paths: workspace manifests, `nostr-keyring/**`, key types,
  `nostr-sdk/src/local_relay/**`, and SDK client/profile APIs.
- License: MIT.
- Dependency baseline: pin stable `nostr` and `nostr-sdk` `0.44.1`; do not adopt
  the reviewed repository's `0.45.0-alpha` line without a later ADR change.
- Adopt: maintained key/event/NIP implementations and SDK relay behavior.
- Contain: upstream secret/key types may implement `Clone` or `Debug`; keep them
  behind a non-cloneable, redacted Radroots adapter boundary.

### nostrdb

- Repository: `damus-io/nostrdb`.
- Commit: `f4591db9524bc4936af76af4750ec425e67700be`.
- Reviewed paths: `README.md`, `LICENSE`, and build/storage entry points.
- License: GPL-3.0-or-later.
- Decision: do not use. The handoff requires migration-managed non-secret
  SQLite, and the additional database and license are unnecessary for the MVP.

### UniFFI

- Repository: `mozilla/uniffi-rs`.
- Commit: `2ccd07e219161c51a5642b4d7be8f174a846462f`.
- Reviewed paths: `README.md`, `LICENSE`, async internals/overview,
  `docs/manual/src/kotlin/**`, callback-interface documentation, Kotlin future
  and callback binding tests, and JNI thread-attachment runtime code.
- Release baseline: `0.32.0`, MPL-2.0.
- Adopt: generated Kotlin bindings, asynchronous operations, explicit object
  disposal, and callback handles.
- Guard: invoke no observer while holding Rust locks; serialize revisions;
  verify JVM thread attachment, cancellation, deregistration, and close races.

## Dependency baseline

- `nostr` and `nostr-sdk` `0.44.1`.
- `uniffi` `0.32.0`.
- `keyring` `4.1.6` behind the Radroots `SecretStore`.
- `rusqlite` `0.40.1` with bundled SQLite.
- `refinery` `0.9.2`.
- `secrecy` `0.10.3`.
- `zeroize` `1.9.0`.
- `directories` `6.0.0`.

Exact transitive resolution is committed through `core/Cargo.lock`. Platform
features and license metadata must be recorded before final acceptance.
