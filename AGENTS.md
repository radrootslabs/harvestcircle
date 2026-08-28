# AGENTS.md — HarvestCircle

These instructions apply to the complete standalone HarvestCircle repository.
A more specific `AGENTS.md` may refine them for its subtree.

## Repository role and source boundary

This repository owns the public HarvestCircle desktop product: the
Kotlin/Compose presentation shell, desktop lifecycle, client-side state,
generated UniFFI integration, host packaging, product-specific Rust core, and
their tests. The Rust workspace under `core/**` owns HarvestCircle application,
domain, runtime, persistence, Nostr-adapter, preference, native FFI, and UniFFI
binding-generator implementation.

HarvestCircle remains a client of canonical public Radroots library packages.
It does not own shared Radroots identity, transport, signing, wire-contract, or
other reusable library policy. Product-specific Rust code must live in this
repository and may depend on the exact public Radroots library revision selected
by the capsule; it must not depend on an implicit sibling checkout.

Product-owned names use the HarvestCircle identity consistently:

- Human-facing product name: `HarvestCircle`.
- Rust crate directories, Cargo packages, and dependency keys:
  `harvestcircle_*`.
- Kotlin source namespace: `org.harvestcircle`.
- Product-owned Kotlin and UniFFI types: `HarvestCircle*`.
- Product-owned environment variables: `HARVESTCIRCLE_*`.

These product-specific names are intentionally distinct from canonical shared
`radroots_*` library packages, Radroots service terminology, and Radroots
corporate or vendor identity, which retain their existing names.

The repository must remain independently cloneable, buildable, testable, and
packageable through its checked-in command surfaces with or without extbuild.
It must not depend on private parent code, parent-only contracts, unpublished
local artifacts, absolute host paths, or an enclosing monorepo layout.

## Machine authority and generated inputs

- `core/Cargo.toml`, `core/Cargo.lock`, `core/rust-toolchain.toml`,
  `radroots.lib.source-lock.v1.toml`, and the product crates under
  `core/crates/**` own the Rust workspace inputs.
- Gradle settings, build scripts, the version catalog, wrapper properties,
  policy configuration, and `config/product/harvestcircle-v1.properties` own
  the desktop build, dependency, product-coordinate, and package inputs.
  Kotlin and Rust source and tests are implementation evidence.
- `gradlew`, `gradlew.bat`, and `gradle/wrapper/gradle-wrapper.jar` are
  checked-in command implementation and supply-chain inputs, not policy
  authority. Review them with `gradle-wrapper.properties`; keep the launcher,
  distribution URL/version, and distribution checksum aligned, and never
  replace the wrapper binary without explicit provenance review.
- Keep every shared Radroots dependency on the canonical public Git source,
  one exact immutable revision, and the declared exact version. The manifests,
  lockfile, compatibility baseline, and generated native package metadata must
  agree.
- Never use a path dependency, floating branch or tag, private mirror,
  implicit sibling override, dirty source cache, or unrecorded native binary.
  Local product crates are workspace path dependencies; shared Radroots
  packages remain immutable public Git dependencies.
- The repository retains only concise standalone governance and operational
  guidance in `README.md`, `NOTICE`, `CONTRIBUTING.md`, `SECURITY.md`,
  `LICENSE`, and `LICENSES/**`. Authoritative product specifications,
  decisions, reviews, handoffs, and qualification evidence are owned by the
  consuming Radroots monorepo under `docs/oss/harvestcircle/**` and must not be
  required to build or test this standalone source tree. All `docs/**`,
  `spec/**`, `.github/**`, and `.act/**` paths are forbidden here. Local
  workflow orchestration belongs to the consuming monorepo's governed
  `.act/**` surface and must call this capsule's standalone Make targets.

Generated UniFFI Kotlin and native libraries are derived build output. Change
the local canonical Rust producer contract/generator first, regenerate into
the active Gradle and Cargo build locations, and inspect the result. Never
hand-edit or check in generated bindings or native binaries as a source
substitute.

## Application and security boundaries

- Product state is bound only through `radroots_runtime_paths::RuntimeContext`
  for service `harvestcircle` and instance `desktop`; the canonical database
  and lock names are `state.sqlite` and `state.lock`. The exact schema starts
  at v1, future migrations start at v2, and `radroots_service_sqlite` owns the
  governed SQLite mechanics.
- `harvestcircle.sqlite3` is legacy evidence only. Never delete, rename,
  import, dual-read, dual-write, or otherwise treat it as current state.
- SQLx is the only high-level SQLite library. HarvestCircle may use its sealed
  application-schema callback inside `radroots_service_sqlite` initialization,
  but must not create another pool, expose a raw connection, or reintroduce
  Rusqlite, Refinery, arbitrary repair, or a second migration authority.
- Storage bootstrap requires the injected runtime context's canonical state
  root to exist. `RuntimeContext::state_directory_plan` is the only production
  authority allowed to create the exact `services/harvestcircle/desktop`
  suffix. `ServiceSqliteHost::open_or_initialize` alone selects create versus
  existing state under one retained writer authority and returns the actual
  verified database metadata. Do not probe paths, recursively create roots,
  repair permissions, or open a raw SQLx connection during bootstrap.
- Native production qualification is limited to macOS aarch64 and Linux
  x86_64. Do not add or claim another target without an explicit contract
  change and its complete platform evidence.
- Keep Compose screens and stores as client presentation and orchestration.
  Do not make them a source of truth for accounts, identities, approvals,
  domain objects, synchronization, reconciliation, or publication state.
- Private keys and signing operations remain behind the generated native
  boundary. Kotlin must handle opaque identifiers and bounded public DTOs; it
  must not persist, log, fixture, or expose raw secret material.
- Recovery and backup operations must be explicit, user-initiated, bounded,
  fail closed, and keep sensitive material out of logs, crash text, analytics,
  filenames, and long-lived UI state.
- Backup and restore must use the sealed HarvestCircle wrappers over Lib's
  capture, verify, stage, finalize, and marker-recovery protocol. Verification
  requires a trusted manifest digest, current database identity, and positive
  caller-supplied member limit; never accept an arbitrary replacement path.
- Clipboard writes of sensitive output require explicit user action, bounded
  lifetime, ownership-aware clearing, and tests for cancellation and
  replacement. Never clear unrelated clipboard content.
- Native library loading must verify the generated artifact, platform/ABI,
  compatibility baseline, and exact source provenance before application use.
  Do not search ambient library paths or silently fall back to another binary.
- Keep lifecycle and coroutine work structured, cancelable, and scoped. Avoid
  hidden workers, process-global mutable state, unbounded retries, blocking UI
  work, and external mutation inferred from environment state.
- Relay parsing, destination policy, DNS admission, and connection ownership
  come from the pinned `radroots_transport_nostr` boundary. Product code may
  select a governed profile and verify product events, but must not recreate
  relay URL policy or open a second production `nostr-sdk` client.
- The native host owns its Tokio runtime for exactly one application-core
  lifetime. Runtime close is explicit, idempotent, and cancellation-resumable;
  observer work and the bounded keyring worker must finish before terminal
  close is reported. `SecretStore` is an object-safe asynchronous application
  port. Every caller awaits it, Tokio workers await one-shot results, and only
  the dedicated credential thread may drive the blocking platform adapter.
  Authoritative locks fail closed on poison.
- Services-hardening changes use the approved target-state contracts. Do not
  add compatibility aliases, dual reads, dual writes, or fallback behavior for
  prototype surfaces removed by the clean-slate refactor.

## Change and command rules

Inspect status, all relevant manifests and locks, compatibility inputs, Gradle
tasks, native generation logic, source, tests, and package
configuration before editing. Make one coherent, reviewable change at a time;
keep implementation, tests, generated outputs, dependency evidence, and public
behavior aligned while preserving unrelated work.

The root `Makefile` is the standalone command surface and its durable behavior
belongs in Gradle or public producer tools. Standalone targets never invoke or
probe extbuild, even when it is installed. Explicit `governed-*` targets run a
green extbuild doctor and route the same underlying commands through extbuild.
Gradle otherwise uses its standard ignored `build/` directories and Cargo uses
the ignored target trees. Run `make doctor` before the first standalone
mutating lane and `make governed-doctor` before a governed lane. Use `make
format`, `make lint`, and `make test` while iterating, and run `make check` or
`make governed-check` for a complete source checkpoint. The active development
milestone is qualified with `make governed-development-check` on macOS aarch64
and `make governed-linux-x86_64-development-check` for the faithful Linux
x86_64 lane. These development targets must retain source, runtime, generated,
API, source-lock, SQLx-topology, and offline license/source verification without
activating advisory retrieval, package assembly, release evidence, signing,
notarization, Nix, or OCI work. Signing, notarization, and release targets are
governed-only and require a separately declared release candidate and fresh
authority. Unknown build modes fail before build mutation.

Shared public Git dependencies and deferred package or advisory lanes may
require external services. Do not weaken immutable inputs or silently switch
sources when offline. During the active development milestone, do not run or
claim the deferred release integrations merely because they remain available
as explicit targets. Never claim a lane passed unless it ran successfully;
report network, toolchain, platform, or external-artifact blockers exactly.

Verify exact manifests/lock agreement, generated binding and native artifact
freshness when affected, compatibility and architecture guards,
license/source policy, zero forbidden roots, `git diff --check`, and final
status and diff. Parent local-`act` proof is integration evidence only: it must
execute, and never replace, the standalone repository validation commands.

## Git and external gates

Use focused commits in the established `<scope>: <imperative summary>` style.
Do not reset, discard, rewrite, push, tag, sign, publish, deploy, package for
distribution, change signing/notarization identities, or rotate credentials
without the corresponding explicit authority.

The change is complete only when it is implemented at the correct desktop or
public producer boundary, the relevant standalone lanes are green, locks and
generated evidence are fresh, forbidden roots remain absent, and final review
finds no secret exposure, private dependency, stale native artifact, hidden
domain authority, or unreported skipped lane.
