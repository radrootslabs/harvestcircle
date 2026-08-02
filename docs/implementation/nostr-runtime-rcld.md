# Nostr runtime multi-RCLD

## Status

- Program status: in progress.
- Active RCLD: none.
- Active atomic checkpoint: none.
- Repository: `oss/studio_app` standalone Git repository.
- Baseline branch: `master`.
- Baseline commit: `9cd07ced24d62eca4cc89f09c82a3539c7fe9dcf`.
- Implementation commits require a separate execution directive.

## Objective

Replace the Kotlin-only generic account-server proof with a Nostr-only desktop
application whose canonical account and application runtime is implemented in
Rust under `core/**`. Compose Desktop remains a thin native JVM 21 shell over a
UniFFI boundary. The UI proves the complete local Nostr accounts model with
minimal primitives; it does not introduce the final product design system.

## Authority and repository boundary

The sole documentary authority for this program is the handoff package at:

`../../docs/handoff/radroots_studio_app_v1_kotlin_basis_handoff/**`

The relative path above is from the `oss/studio_app` capsule root. This plan is
a derived execution ledger and is subordinate to that package. Other monorepo
documents and the legacy `/studio_app` tree are not implementation authority.

All source changes belong inside this standalone `oss/studio_app` repository.
Do not stage or commit its gitlink from the parent repository as part of these
RCLDs. Do not modify the parent repository or the legacy `/studio_app` tree.

## Execution contract

1. Execute the RCLDs in numeric order.
2. Keep only one RCLD and one atomic checkpoint active at a time.
3. Preserve the 63 checkpoint order defined below. Do not merge, skip, or
   reorder checkpoints merely to reduce commit count.
4. Before each checkpoint, inspect its authority and the nested repository
   status. Reconcile the remaining ledger after each green checkpoint.
5. Implement the smallest complete change for the active checkpoint.
6. Run its narrow verify lane plus every relevant inherited regression lane.
7. Repair or split a red checkpoint. Never commit a red checkpoint.
8. Commit only when an execution directive explicitly authorizes commits.
9. Never push, publish, deploy, or update the parent gitlink without separate
   authorization.
10. Record any necessary deviation in this document before implementation. A
    deviation may clarify an unsafe instruction but may not broaden product
    scope.

## Approved constraints and normalized decisions

### Build and repository policy

- Do not use extbuild in this OSS capsule.
- Do not add `.github/**`, `scripts/**`, empty directories, or placeholder
  future modules.
- Keep the Makefile as the human-facing lifecycle surface. It delegates to
  repository-owned Cargo and Gradle tasks.
- Keep generated UniFFI Kotlin, staged native libraries, Cargo output, Gradle
  output, and packaged artifacts under ignored build directories.
- Preserve package `org.radroots.studio`, Gradle module `:app:desktop`, JVM 21,
  the pinned Kotlin and Compose versions, the application icon, native package
  identity, transparent macOS title bar, 1284 by 795 initial window size, 1080
  by 720 minimum size, and fail-closed Kotlin test discovery.
- Normalize application and package version metadata around `0.1.0-alpha`; use
  an installer-compatible numeric form only where a native packaging tool
  rejects prerelease text, and document that mapping.

### Runtime ownership

- Rust `AppCore` is the sole canonical owner of accounts, selection, active
  session, persistence, recovery, relay/profile work, snapshot revisions, and
  safe errors.
- Kotlin owns native-window lifecycle, generated-binding lifecycle,
  presentation-only state, user input, command dispatch, and immutable DTO
  rendering.
- Kotlin must not reproduce the Rust reducer, session state machine, storage
  policy, or relay policy.
- Mutating AppCore commands are serialized. Public snapshots are immutable and
  revisioned monotonically.
- Observers are never called while an internal lock is held. Notifications are
  serialized in revision order and stop after deregistration.

### Account and session semantics

- Canonical account identity is a lowercase 64-character Nostr public-key hex
  value. `npub` is a human-facing encoding, not a storage key.
- A local label is optional and is distinct from Nostr `name` and
  `display_name` profile fields.
- Generate and import persist and select an account but do not activate it.
- Startup restores saved accounts and persisted selection but starts signed
  out. It does not access a credential or relay until explicit activation.
- Selection and active session are independent. Selecting a candidate does not
  destroy the current active session.
- Activation prepares and validates the candidate credential, signer, cached
  profile, and relay session before atomically replacing a working session.
  Failure preserves the previous session.
- Sign out cancels account tasks and drops signer/session state while retaining
  account metadata, selection, and credential.
- Removal uses a single-use confirmation token bound to the target pubkey and
  snapshot revision. Active removal signs out first and never auto-activates a
  fallback.
- Removing a selected account chooses the next persisted account, then the
  preceding account. Empty registries have no selection.
- An existing account and credential returns `AccountAlreadyExists` without
  overwrite. Matching import into an explicit `CredentialMissing` account is
  the supported repair path.

### Secret boundary

- Production secret keys exist only in the OS credential store through a Rust
  `SecretStore`; there is no file, preference, encrypted-file, or plaintext
  fallback.
- Use credential service `org.radroots.studio.nostr` and canonical pubkey hex
  as its account key.
- Application secret wrappers are non-`Clone`, non-`Debug`, non-`Display`, and
  non-serializable. Cloneable or debug-capable upstream SDK secret types remain
  inside the narrow signer adapter and never cross domain or FFI boundaries.
- Secrets are forbidden in SQLite, profile cache, operation journals,
  snapshots, normal public DTOs, logs, errors, filenames, preferences,
  analytics, fixtures, and golden files.
- `GenerateAccountReceipt.generated_nsec` is the sole transient public DTO
  exception. It never enters `AppSnapshot` or persistence.
- Kotlin holds generated nsec only in non-saveable presentation state. It is
  cleared on acknowledgement, replacement, or application disposal.
- Copying generated nsec is explicit. Clear the clipboard after 60 seconds only
  when it still contains the copied value.
- Imported key text is masked, submitted once, and cleared immediately after
  the FFI call. The JVM limitation is documented and tested as far as the
  platform permits.

### Persistence and recovery

- Use migration-managed bundled SQLite at an injectable OS application-data
  location.
- Persist accounts, selected account, bounded profile cache, typed non-secret
  account preferences, and a non-secret cross-resource operation journal.
- Publish success only after durable state commits. Never update the public
  snapshot first and merely log a persistence failure.
- Add/import recovery covers credential write, metadata commit, compensation,
  compensation failure, and restart.
- Removal recovery records intent, credential deletion, metadata deletion, and
  finalization as distinct phases. A deleted credential is never represented
  as restorable.
- Missing credentials produce an explicit `CredentialMissing` repair state,
  not an implicit read-only signer.
- Corrupt or unsupported databases fail safely and are not destructively
  recreated without an explicit future recovery feature.
- Do not expose arbitrary `set_account_scoped_value` or
  `get_account_scoped_value` methods through shipping UniFFI. Prove account
  partitioning through a typed Rust repository interface and integration tests
  until a real typed preference is required.

### Relay and profile behavior

- Read the ordered relay list from comma-separated
  `RADROOTS_NOSTR_RELAYS`. Trim and deduplicate while preserving order.
- Accept `wss://`. Accept `ws://` only for `localhost`, `127.0.0.0/8`, and
  `::1`. Reject user information, fragments, and non-WebSocket schemes.
- Use `ws://localhost:8080` only as a development/test fallback. A packaged
  runtime without valid configuration reports a safe configuration state.
- Tests use local ephemeral relays or controlled fakes and never public
  internet relays.
- Verify kind-0 event ID, signature, author, and kind before parsing. Select the
  newest `created_at`; break equal timestamps with the lexicographically lowest
  event ID.
- Bound relay messages, raw event content, parsed profile fields, and stored
  profile data.
- Emit cached profile state before asynchronous refresh. Timeout, offline, or
  invalid relay data preserves the cache and produces a nonfatal state.
- Display `nip05` and `picture` as bounded metadata. NIP-05 verification and
  remote-image fetching are out of scope.

### UniFFI and Kotlin lifecycle

- Use UniFFI async operations for work that could block the Compose thread.
- Supervise Rust relay tasks and cancel them on replacement, sign out, and
  AppCore close.
- Bind asynchronous results to their initiating account and revision so stale
  results cannot overwrite newer state.
- Ensure callback delivery is safe for JVM thread attachment and callback
  re-entry. Close observer handles and AppCore explicitly from Kotlin.
- Generated bindings remain under `build/generated` and are never hand-edited.
- Keep one AppCore instance at the application root. The Kotlin store subscribes
  once, exposes read-only Compose state, forwards commands, and closes cleanly.

### Minimal product UI

- Provide an inactive accounts screen with generate, masked import, saved
  accounts, selection, activation, and confirmed removal.
- Provide a one-time generated-key backup panel with npub, nsec, warning, copy,
  and acknowledgement.
- Provide a minimal active-account screen with pubkey, npub, cached profile,
  relay/profile state, configured relays, refresh, switch-account, and sign-out
  controls.
- Use basic Compose primitives, existing limited styling, stable test tags, and
  accessibility descriptions. Do not add a design system.
- Remove UUID account identity, HTTP/HTTPS server URL fields, fake
  `LoginStatus`, the canonical Kotlin reducer/store, and all generic account
  server language from active source and tests.

### Dependency baseline

Pin exact compatible versions in the dependency checkpoint and record their
licenses and primary-source provenance:

- stable `nostr` and `nostr-sdk` `0.44.1`;
- UniFFI `0.32.0`;
- `keyring` `4.1.6`, behind the Radroots `SecretStore`;
- `rusqlite` `0.40.1` with bundled SQLite;
- `refinery` `0.9.2`;
- `secrecy` `0.10.3`;
- `zeroize` `1.9.0`;
- `directories` `6.0.0`.

Notedeck and nostrdb are GPL clean-room references only. Do not copy their
code. Reject optional disk-secret storage, nondeterministic account fallback,
cache-before-persist publication, and delete-without-recovery patterns.
Reject Amethyst's encrypted-file fallback, mutation-before-cleanup transition,
swallowed deletion failures, and memory-cache-before-disk publication.

## Global definition of green

Every checkpoint must satisfy all applicable items below before it can be
committed:

- formatting is clean;
- warnings are denied in first-party Rust code;
- the narrow Rust or Kotlin unit tests pass;
- inherited tests for completed behavior remain green;
- no public-internet relay is contacted by tests;
- no generated or build output is staged;
- no secret or generic server-account concept has crossed a forbidden
  boundary;
- the diff contains only the active checkpoint;
- the nested repository is the only repository being changed;
- this ledger is updated if checkpoint status or remaining scope changed.

There is no numeric coverage threshold. The required standard is strong,
best-effort behavioral coverage across Rust unit, storage, recovery, FFI,
Kotlin store, Compose UI, local relay, restart, native-loader, and end-to-end
lanes.

## RCLD sequence

### RCLD-01: Authority and dependency baseline

Status: completed.

Scope: checkpoint 1. Record live repository state, authoritative handoff files,
reference source SHAs and paths, adopted and rejected ideas, license boundaries,
dependency versions, native platform assumptions, and the approved normalized
decisions in capsule-local research and ADR material.

Definition of green: the recorded facts match the live checkout and primary
sources; GPL references are clean-room only; no runtime source has changed.

Verify lane: Git boundary/status inspection, primary-source version and license
checks, documentation link/path validation, and diff review.

### RCLD-02: Rust workspace and domain

Status: in progress.

Scope: checkpoints 2 through 11. Establish the Rust workspace and implement
safe errors, public keys, secret input/redaction, NIP-19 types, relay URLs,
account metadata, kind-0 profile rules, and immutable application snapshots.

Definition of green: all domain values are validated types; secret wrappers are
redacted and non-cloneable; snapshots contain no secret fields; domain tests
cover valid, invalid, canonicalization, ordering, and invariant cases.

Verify lane: Cargo formatting, clippy with warnings denied, workspace check,
and focused domain tests.

### RCLD-03: Application state machine

Status: pending.

Scope: checkpoints 12 through 14. Define repository, secret, clock, and Nostr
ports; implement in-memory AppCore bootstrap, command serialization, monotonic
revisions, transition helpers, and observer registration/deregistration.

Definition of green: selected and active state are separate; transitions are
deterministic; callbacks occur outside locks; observer lifecycle and command
traces are tested.

Verify lane: application crate formatting, clippy, check, unit tests, callback
re-entry tests, and workspace regression tests.

### RCLD-04: SQLite persistence

Status: pending.

Scope: checkpoints 15 through 20. Add bundled SQLite and migrations, account and
selection persistence, profile cache, typed account-scoped partitioning,
operation journal storage, and persisted AppCore bootstrap.

Definition of green: fresh, migrated, restarted, isolated, corrupt, and
idempotent database cases are covered; no secret appears in schema or data;
snapshots publish only after successful commits.

Verify lane: storage formatting, clippy, migration tests, restart tests,
failure-injection tests, database-byte secret guards, and workspace tests.

### RCLD-05: Credential boundary

Status: pending.

Scope: checkpoints 21 through 24. Add SecretStore, in-memory and
failure-injection fakes, OS keyring adapter, safe platform errors, and global
no-secret guards.

Definition of green: production has no fallback; adapter service/key naming is
stable; unavailable, missing, duplicate, read, write, and delete outcomes map
to safe errors; known secrets are absent from snapshots, persistence, errors,
and captured logs.

Verify lane: secret-store unit and contract tests, platform adapter smoke where
available, redaction tests, Cargo checks, and inherited storage tests.

### RCLD-06: Account generation and import

Status: pending.

Scope: checkpoints 25 through 30. Pin and adapt the selected Nostr library;
implement generation, nsec and secret-hex import, duplicate and repair
semantics, cross-resource rollback, journaling, and persisted commands.

Definition of green: derived pubkeys and NIP-19 values match known vectors;
generated nsec appears only in its receipt; duplicate imports do not overwrite;
every keyring/database failure boundary is tested; partial success is never
published.

Verify lane: Nostr vector tests, application command tests, transaction and
restart failure-injection matrix, no-secret guards, and workspace regression.

### RCLD-07: Account lifecycle and recovery

Status: pending.

Scope: checkpoints 31 through 35. Implement selection, signed-out startup,
safe replacement activation, sign out, revision-bound removal confirmation,
deterministic fallback, and removal journal recovery.

Definition of green: failed replacement preserves the active session; sign out
retains saved state; stale confirmation is rejected; removal is deterministic;
irreversible deletion phases are represented honestly and recover on restart.

Verify lane: state-machine tables, concurrent/stale command tests, activation
failure tests, removal failure-injection matrix, restart recovery, cancellation,
and workspace tests.

### RCLD-08: Relay and profile runtime

Status: pending.

Scope: checkpoints 36 through 40. Add environment relay configuration, exact
WebSocket policy, verified kind-0 parsing, deterministic local relay fixture,
cache-first asynchronous refresh, and manual refresh.

Definition of green: relay parsing follows the approved policy; invalid events
are rejected; equal-time tie-breaking is deterministic; cached state arrives
before refresh; stale, offline, timeout, cancellation, and repeated refresh
outcomes are safe; tests use no public relay.

Verify lane: relay parser tests, NIP event vectors, local ephemeral relay tests,
async stale-result/cancellation tests, profile cache tests, and workspace tests.

### RCLD-09: UniFFI and native build integration

Status: pending.

Scope: checkpoints 41 through 47. Add UniFFI definitions and safe DTO mapping,
expose supported AppCore commands, implement observer handles, build the native
library, generate Kotlin bindings, and stage native artifacts for development,
tests, and Compose packaging.

Definition of green: all shipping commands are callable without the generic
namespace API; generated DTOs contain no ordinary secret field; async calls do
not block Compose; callbacks are ordered and close safely; generated source is
not tracked; development and packaged native loading work on the current host.

Verify lane: Cargo FFI tests, bindgen freshness/generation, ABI and loader smoke,
Gradle compile/test, observer cancellation/re-entry tests, package staging
inspection, and secret-field source guards.

### RCLD-10: Thin Kotlin shell and minimal UI

Status: pending.

Scope: checkpoints 48 through 57. Implement the thin StudioAppStore, create and
close one AppCore at the application root, map public DTOs, implement inactive,
backup, saved-account, active-home, switch, sign-out, refresh, and removal UI,
then remove the canonical Kotlin proof and generic server-account remnants.

Definition of green: the complete MVP is operable through minimal Compose
controls; generated secrets remain ephemeral; all important controls have
stable tags and accessibility descriptions; no UUID, server URL, fake login,
or duplicated Kotlin state machine remains.

Verify lane: Kotlin unit tests, Compose UI tests for success and failure flows,
clipboard fake/timer tests, AppCore lifecycle tests, full Gradle check, source
guards, and native-loader smoke.

### RCLD-11: End-to-end integration hardening

Status: pending.

Scope: checkpoints 58 and 59. Add full local-relay end-to-end coverage plus
restart, account isolation, FFI callback, cancellation, deregistration, stale
completion, and no-secret integration tests.

Definition of green: generate/import, activate, cached profile, relay refresh,
sign out, switch, remove, restart, repair, and callback lifecycle are proven
without public network access; account A cannot observe account B's private
namespace; secret guards pass across persisted and serialized artifacts.

Verify lane: full Cargo workspace test, focused end-to-end suites, Gradle test,
Compose tests, native-loader smoke, and repeated runs for async determinism.

### RCLD-12: Documentation and acceptance reconciliation

Status: pending.

Scope: checkpoints 60 through 63. Complete dependency/license, architecture,
security, testing, local-relay, FFI lifecycle, recovery, and platform validation
documentation; finish the Makefile lifecycle; perform the final source audit
and reconcile every acceptance criterion.

Definition of green: required capsule documentation matches implemented paths
and behavior; Makefile exposes doctor, format, lint, test, check, build, dev,
run, package, and clean without extbuild or scripts; current-host package smoke
passes; portable loader concerns for macOS, Linux, and Windows are documented;
the final ledger records every command and residual platform risk honestly.

Verify lane: all Makefile lifecycle targets applicable without destructive
cleanup, Cargo workspace format/clippy/check/test, Gradle check/test/build,
UniFFI generation, loader smoke, local-relay end-to-end, current-OS packaging,
forbidden-path and forbidden-term guards, Git status, and full diff audit.

## Atomic checkpoint ledger

All checkpoints are pending. Their order and titles are inherited from the
handoff commit sequence.

### RCLD-01

- [x] 01. Audit live repository and reference sources.

### RCLD-02

- [x] 02. Establish root Rust workspace skeleton.
- [ ] 03. Add Rust formatting, lint, and governed check hooks. Use Cargo,
  Gradle, and Makefile; do not add CI workflows or scripts.
- [ ] 04. Define domain module layout and safe error shell.
- [ ] 05. Implement Nostr public key value object.
- [ ] 06. Implement secret input boundary and redacted secret wrapper.
- [ ] 07. Add NIP-19 public/secret display contract types.
- [ ] 08. Implement relay URL parser and policy.
- [ ] 09. Add account public metadata value types.
- [ ] 10. Add profile metadata model and kind-0 selection rules.
- [ ] 11. Define immutable AppSnapshot and state enums.

### RCLD-03

- [ ] 12. Add application ports for repositories, secrets, clock, and Nostr
  client.
- [ ] 13. Implement in-memory AppCore bootstrap and observer registry.
- [ ] 14. Add state transition helpers and command trace tests.

### RCLD-04

- [ ] 15. Create SQLite storage crate and migration runner.
- [ ] 16. Implement account and selected-account persistence.
- [ ] 17. Implement profile cache persistence.
- [ ] 18. Implement typed account-scoped namespace persistence.
- [ ] 19. Add operation journal persistence.
- [ ] 20. Implement storage adapter wiring for AppCore bootstrap.

### RCLD-05

- [ ] 21. Add SecretStore trait and in-memory fake.
- [ ] 22. Add failure-injection SecretStore fake.
- [ ] 23. Implement OS keyring secret adapter.
- [ ] 24. Add global no-secret snapshot and storage assertions.

### RCLD-06

- [ ] 25. Pin Nostr dependency and implement key generation/derivation adapter.
- [ ] 26. Implement generate account command with in-memory storage.
- [ ] 27. Implement import secret key command.
- [ ] 28. Define and test duplicate import and credential-repair handling.
- [ ] 29. Implement add/import transaction rollback across keyring and DB.
- [ ] 30. Implement persisted generate/import using SQLite adapter.

### RCLD-07

- [ ] 31. Implement select account command.
- [ ] 32. Implement activate account with safe replacement ordering.
- [ ] 33. Implement sign out command.
- [ ] 34. Implement revision-bound removal request/confirmation flow.
- [ ] 35. Implement removal journal recovery.

### RCLD-08

- [ ] 36. Add relay configuration source and environment parser.
- [ ] 37. Implement Nostr event verification and kind-0 parsing adapter.
- [ ] 38. Add Nostr client port implementation and local relay fixture.
- [ ] 39. Implement cache-first active profile refresh orchestration.
- [ ] 40. Expose manual refreshActiveProfile command.

### RCLD-09

- [ ] 41. Create UniFFI scaffolding and DTO mapping.
- [ ] 42. Expose approved AppCore commands through UniFFI.
- [ ] 43. Expose observer callback and deregistration through UniFFI.
- [ ] 44. Build native library artifacts from Cargo.
- [ ] 45. Add Gradle task for Cargo build.
- [ ] 46. Add Gradle UniFFI binding generation and generated source set.
- [ ] 47. Stage native library for development, tests, and packaged app.

### RCLD-10

- [ ] 48. Add Kotlin StudioAppStore thin adapter.
- [ ] 49. Bootstrap AppCore once at application root.
- [ ] 50. Create UI model mapping helpers without server fields.
- [ ] 51. Implement inactive account screen generate/import controls.
- [ ] 52. Implement generated-key backup panel.
- [ ] 53. Implement saved-account list, activation, and removal controls.
- [ ] 54. Implement active account home screen.
- [ ] 55. Implement switch account flow in UI.
- [ ] 56. Remove canonical Kotlin account reducer/store usage.
- [ ] 57. Remove generic server-account remnants.

### RCLD-11

- [ ] 58. Add full local-relay end-to-end integration test.
- [ ] 59. Add restart, account isolation, and FFI callback integration tests.

### RCLD-12

- [ ] 60. Add dependency and license documentation.
- [ ] 61. Complete architecture, security, testing, and runbook documentation.
- [ ] 62. Add Makefile-governed final validation tasks and platform ledger. Do
  not add `.github/**` or `scripts/**`.
- [ ] 63. Perform final source audit and acceptance reconciliation.

## Unfinished RCLD ledger

- [x] RCLD-01: Authority and dependency baseline.
- [ ] RCLD-02: Rust workspace and domain.
- [ ] RCLD-03: Application state machine.
- [ ] RCLD-04: SQLite persistence.
- [ ] RCLD-05: Credential boundary.
- [ ] RCLD-06: Account generation and import.
- [ ] RCLD-07: Account lifecycle and recovery.
- [ ] RCLD-08: Relay and profile runtime.
- [ ] RCLD-09: UniFFI and native build integration.
- [ ] RCLD-10: Thin Kotlin shell and minimal UI.
- [ ] RCLD-11: End-to-end integration hardening.
- [ ] RCLD-12: Documentation and acceptance reconciliation.
