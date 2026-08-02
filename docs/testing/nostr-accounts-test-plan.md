# Nostr accounts test plan

## Status

Initial test contract. Record exact commands and completed coverage as the
runtime is implemented.

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

Run the existing desktop regression lane separately until the Makefile combines
the Rust and desktop lifecycles:

```sh
./gradlew --no-daemon :app:desktop:test
```

The capsule intentionally has no `.github/**` workflow and no validation
script. The Makefile will become the combined human-facing command surface
without changing these repository-owned Cargo and Gradle gates.
