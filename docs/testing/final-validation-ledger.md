# Final validation ledger

## Result

Validation completed on 2026-08-02 for macOS 26.5 arm64. All 30 acceptance
criteria in the authoritative handoff are satisfied by implemented source,
tests, or an explicit documented platform-validation contract. No test uses a
public relay.

The Radroots crates release v1 reconciliation was rerun on 2026-08-03. Locked
Rust formatting, workspace all-target checks, workspace all-target tests,
Gradle desktop checks, native-loader coverage, and the current-host DMG package
were green through the governed extbuild output router. The reviewed legacy SDK
runtime is absent; the evidence-based Step 274–278 deviation is recorded in
`docs/architecture/radroots-crates-release-v1-reconciliation.md`.

## Acceptance reconciliation

| # | Result | Evidence |
| --- | --- | --- |
| 1 | Pass | `core/Cargo.toml` defines the Rust workspace and all five runtime crates build together. |
| 2 | Pass | `radroots-studio-application::AppCore` owns canonical snapshots, transitions, commands, observers, sessions, recovery, and refresh orchestration. |
| 3 | Pass | `StudioAppStore` maps generated UniFFI DTOs into presentation models and forwards commands without duplicating the reducer or persistence policy. |
| 4 | Pass | Domain, persistence, FFI, and Kotlin models use canonical lowercase 64-character Nostr public-key hex; forbidden UUID guards are clean. |
| 5 | Pass | The generate command, UniFFI method, Compose control, and behavior tests create a persisted local Nostr key. |
| 6 | Pass | Masked nsec/hex import is implemented through the Rust key boundary and covered by valid, invalid, duplicate, and repair tests. |
| 7 | Pass | Storage and Compose tests cover multiple saved accounts; the chooser remains scrollable within the available window. |
| 8 | Pass | Activation is available for each saved account and Rust tests prove safe session replacement. |
| 9 | Pass | Sign-out drops the active signer/session while retaining metadata, selection, and credential. |
| 10 | Pass | Removal uses a single-use target-and-revision-bound confirmation token, deterministic fallback, and recovery journal. |
| 11 | Pass | Production wiring uses `OsKeyringSecretStore` with service `org.radroots.studio.nostr` and no file fallback. |
| 12 | Pass | Redaction, schema-byte, DTO, snapshot, error, journal, and source guards keep secrets out of forbidden surfaces. |
| 13 | Pass | Generated nsec exists only in `GenerateAccountReceipt` and the transient backup UI; acknowledgement, replacement, timeout, and disposal clear it. |
| 14 | Pass | SQLite restart tests restore account metadata and selection while startup remains signed out. |
| 15 | Pass | Typed repository and restart-isolation tests prove account-local values are partitioned by owner pubkey. |
| 16 | Pass | Production relay configuration is read from `RADROOTS_NOSTR_RELAYS`. |
| 17 | Pass | Development mode alone defaults to `ws://localhost:8080`. |
| 18 | Pass | Relay parsing accepts governed WebSocket URLs only; generic account-server terms and fields are absent. |
| 19 | Pass | Activation emits cached profile state before verified kind-0 refresh; failure and stale completion preserve safe state. |
| 20 | Pass | The active screen renders `radroots`, pubkey, npub, bounded profile metadata, relay list, and profile/relay status. |
| 21 | Pass | Source guards find no generic server URL field or onboarding language. |
| 22 | Pass | The previous Kotlin `AccountsReducer` and `AccountsStore` are absent; `StudioAppStore` is an FFI adapter only. |
| 23 | Pass | `make check` passed Rust, FFI, storage, security, restart, local-relay, Kotlin-store, native-loader, and Compose UI lanes. |
| 24 | Pass | The RCLD history contains ordered, independently verified commit-sized checkpoints plus separately committed audit fixes. |
| 25 | Pass | Source, tests, generated-boundary policy, package contents, forbidden terms, and all handoff criteria were audited at checkpoint 63. |
| 26 | Pass | The command ledger below distinguishes executed checks from intentionally skipped interactive, destructive, or foreign-platform checks. |
| 27 | Pass | `docs/architecture/reference-research.md` records reviewed revisions, paths, adopted/rejected patterns, license boundaries, and dependency decisions. |
| 28 | Pass | Required ADR, architecture, security, testing, and local-relay runbook documentation exists and matches the runtime. |
| 29 | Pass | `docs/testing/platform-validation.md` defines the current-host evidence and the required Linux/Windows loader, keyring, and packaging matrix. |
| 30 | Pass | Network tests use an ephemeral loopback relay; configuration tests do not contact the network. |

## Commands executed

| Command | Result |
| --- | --- |
| `make check` | Passed Rust formatting, all-target Clippy with denied warnings, all workspace tests, desktop tests, and Gradle check. |
| `make build` | Passed the Rust workspace and Compose Desktop build. |
| `make bindings` | Passed UniFFI Kotlin generation and current-host native-library staging. |
| `make package` | Produced `app/desktop/build/compose/binaries/main/dmg/Radroots-1.0.0.dmg`. |
| `codesign --verify --deep --strict .../Radroots.app` | Passed. |
| `PlistBuddy` bundle inspection | Confirmed bundle ID `org.radroots.studio` and installer version `1.0.0`. |
| Packaged application JAR inspection | Confirmed `darwin-aarch64/libradroots_studio_ffi.dylib`, `icons/radroots.icns`, and `icons/radroots.png`. |
| Tracked-path guards | Found no build output, generated UniFFI source, native binary, `.github/**`, or `scripts/**` path. |
| Forbidden-term guards | Found no old package/version, UUID account, Kotlin reducer/store, generic account-server, or server-URL term. |
| Nested-repository status and history inspection | Confirmed the standalone capsule boundary and local commit sequence. |

`make check` executed 35 application tests plus its redaction test, 21 domain
tests, 6 FFI tests, 6 Nostr tests, 19 passing storage unit tests, three storage
integration tests, the bindgen test, all documentation tests, and the complete
Kotlin/Compose test task. The one ignored storage test is the intentionally
opt-in real operating-system keyring smoke.

## Intentionally not executed

- The real OS keyring smoke was not enabled because it mutates the current
  user's credential store. Its adapter contract tests passed.
- Linux and Windows loader, keyring, and packaging checks cannot run on this
  macOS host. Their required native-host matrix is recorded in
  `platform-validation.md`.
- `make dev` and `make run` are interactive launchers and were not held open.
- `make clean` was not run because it only destroys recoverable build output
  and is not an acceptance behavior.

The application/runtime artifacts retain version `0.1.0-alpha`. The macOS
installer uses `1.0.0` because `jpackage` rejects the prerelease form; this
mapping is deliberate and documented. No source-level acceptance blocker
remains.
