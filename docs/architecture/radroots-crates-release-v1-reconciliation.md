# Radroots crates release v1 reconciliation

## Decision

The Studio cutover described by Radroots crates release v1 Steps 274–278 is an
evidence-based obsolete-source deviation for the current `studio_app` capsule.
It must not be implemented by restoring the retired desktop runtime or by
adding an unused `radroots_sdk` dependency.

The crates-release review inspected Studio commit
`8d849a204b0b67865603f5c877ad7409fc922d30`. That revision contained the GPUI
workspace, `crates/runtime/src/sdk.rs`, `studio.sqlite`, direct Radroots lower
crate dependencies, and sibling paths into `../lib` and `../sdk`. Commit
`ae0179669c59261fe273a58d781763f69ad2b198` deleted that runtime. The current
head descends from the reviewed revision and implements the later approved
Rust/UniFFI/Compose Nostr-account architecture completed by the capsule's
63-checkpoint runtime sequence.

The current product does not implement the farms, listings, trades, generic
storage, or generic synchronization semantics owned by `radroots_sdk`. Its
canonical Rust `AppCore` owns only Studio account/session state and bounded
kind-zero profile refresh. Adding the SDK without consuming those semantics
would create a false dependency and two lifecycle owners.

## Step reconciliation

- Step 274 is satisfied by removal of all production sibling paths to the
  Radroots crate repositories. Workspace `path` dependencies are capsule-local
  composition between Studio-owned crates. There are no direct dependencies on
  a lower `radroots_*` package. Direct `nostr` and `nostr-sdk` dependencies are
  deliberate upstream protocol adapters for the current product contract.
- Step 275's reviewed supervisor no longer exists. The replacement retains one
  host-owned `AppCore`, a supervised Tokio runtime, explicit async UniFFI
  commands, revision-bound results, closeable observers, and explicit shutdown.
- Step 276 is satisfied by the Studio-owned migration-managed SQLite database
  under `core/crates/storage`. SDK backup or status code cannot own, mutate, or
  report this database because the current graph contains no SDK edge.
- Step 277's generic signing, transport, and sync effects were deleted with the
  reviewed runtime. Current key handling stays behind `SecretStore`; profile
  fetch is bounded, local-relay tested, account/revision bound, and presentation
  state remains host-owned.
- Step 278 is satisfied by the capsule's locked Rust and Gradle checks, native
  loader smoke, complete application tests, and current-host distribution
  package. A local Radroots registry canary is not applicable because the
  resolved graph contains no Radroots registry package.

## Boundary guard

The Studio manifest graph must continue to satisfy all of these conditions:

1. no dependency path escapes the `studio_app` capsule;
2. no `radroots_*` dependency is added merely to claim SDK migration;
3. Studio account, preference, profile-cache, and presentation state remains
   host-owned;
4. secrets remain in the operating-system credential adapter and never enter
   SQLite, DTOs, logs, or generated bindings;
5. any future farms, listings, trades, or generic sync product surface must be
   introduced through the then-current `radroots_sdk` API and its real package
   artifacts, not by restoring the deleted runtime.

This reconciliation preserves the crates-release architecture requirement:
consumers use the correct layer for the semantics they actually consume, and a
new SDK edge is required when Studio first consumes SDK-owned semantics.
