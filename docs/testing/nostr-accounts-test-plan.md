# Nostr accounts test plan

## Status

Implemented test contract. There is no numeric coverage threshold; the suite
uses behavior-focused best-effort coverage and fails if no Kotlin tests are
discovered.

## Required lanes

- Rust domain validation, canonicalization, state invariants, and transition
  tables.
- SQLite migrations, restart restoration, account isolation, corruption, and
  transaction failure injection.
- SecretStore contract, platform smoke tests, and no-secret guards.
- Nostr key vectors, signed-event verification, kind-0 ordering, and bounded
  profile parsing.
- Local ephemeral relay, cached-first refresh, timeout, invalid data,
  cancellation, and stale-result behavior.
- UniFFI generation, DTO mapping, callback ordering/re-entry/deregistration,
  object disposal, and native loading.
- Kotlin store lifecycle and Compose UI coverage for generate, import, backup,
  selection, activation, refresh, switch, sign out, removal, and safe errors.
- End-to-end restart, recovery, account isolation, and packaged-runtime smoke.

No test uses a public relay. There is no numeric coverage threshold; tests must
provide strong best-effort behavioral coverage throughout RCL development.

## Rust validation gates

Run from the capsule root:

```sh
cargo fmt --manifest-path core/Cargo.toml --all --check
cargo clippy --manifest-path core/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path core/Cargo.toml --workspace
```

Run the desktop, generated binding, native loader, store, and Compose lanes:

```sh
./gradlew --no-daemon :app:desktop:test
```

Focused integration paths are:

```sh
cargo test --manifest-path core/Cargo.toml -p radroots-studio-storage local_relay_e2e
cargo test --manifest-path core/Cargo.toml -p radroots-studio-storage restart_restores_selection
cargo test --manifest-path core/Cargo.toml -p radroots-studio-ffi ffi_callback_receives_async_profile_refresh
./gradlew --no-daemon :app:desktop:test --tests org.radroots.studio.ffi.NativeLoaderTest
```

`local_relay_e2e.rs` imports and activates a deterministic signer, reads signed
kind-zero metadata through an ephemeral loopback relay, observes loading/fresh
revisions, verifies SQLite cache, and checks public redaction.
`restart_isolation.rs` reopens the database, restores selected accounts, proves
owner-scoped values remain isolated, and scans bytes for known secrets. FFI
tests cover callback re-entry, asynchronous refresh, unsubscribe, and shutdown.
Compose tests cover the complete minimal UI surface and use fake actions rather
than credentials or public relays.

The real OS keyring smoke test is ignored by default and must be explicitly run
on each supported target in an isolated test account. Packaging and native
loader smoke are current-host checks; cross-platform results belong in the
final validation ledger. The capsule intentionally has no `.github/**` workflow
and no `scripts/**` command surface.
