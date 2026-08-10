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

- `core/Cargo.toml`, `core/Cargo.lock`, `core/rust-toolchain.toml`, and the
  product crates under `core/crates/**` own the Rust workspace inputs.
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
- The repository tracks durable public product specifications, decisions,
  contributor and security guidance, qualification evidence, and thin CI
  wrappers. Approved public surfaces are `README.md`, `NOTICE`,
  `CONTRIBUTING.md`, `SECURITY.md`, `LICENSE`, `LICENSES/**`,
  `spec/harvestcircle_mvp_v1/**`, `docs/decisions/**`,
  `docs/qualification/**`, and `.github/workflows/{source,package}.yml`.
  These roots are inspected by the same namespace, secret, generated-output,
  credential, and symlink rules as source code. Internal handoffs, RCLDs,
  migration narratives, and execution records remain parent-owned. Other
  `docs/**`, `spec/**`, `.github/**`, and all `.act/**` paths are forbidden.
  CI must remain a thin wrapper around repository-owned Make targets.

Generated UniFFI Kotlin and native libraries are derived build output. Change
the local canonical Rust producer contract/generator first, regenerate into
the active Gradle and Cargo build locations, and inspect the result. Never
hand-edit or check in generated bindings or native binaries as a source
substitute.

## Application and security boundaries

- Keep Compose screens and stores as client presentation and orchestration.
  Do not make them a source of truth for accounts, identities, approvals,
  domain objects, synchronization, reconciliation, or publication state.
- Private keys and signing operations remain behind the generated native
  boundary. Kotlin must handle opaque identifiers and bounded public DTOs; it
  must not persist, log, fixture, or expose raw secret material.
- Recovery and backup operations must be explicit, user-initiated, bounded,
  fail closed, and keep sensitive material out of logs, crash text, analytics,
  filenames, and long-lived UI state.
- Clipboard writes of sensitive output require explicit user action, bounded
  lifetime, ownership-aware clearing, and tests for cancellation and
  replacement. Never clear unrelated clipboard content.
- Native library loading must verify the generated artifact, platform/ABI,
  compatibility baseline, and exact source provenance before application use.
  Do not search ambient library paths or silently fall back to another binary.
- Keep lifecycle and coroutine work structured, cancelable, and scoped. Avoid
  hidden workers, process-global mutable state, unbounded retries, blocking UI
  work, and external mutation inferred from environment state.
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
belongs in Gradle or public producer tools. When extbuild is installed, the
Makefile routes commands through it. Without extbuild, Gradle uses its standard
ignored `build/` directories and Cargo uses the ignored `core/target/` tree;
the same checked-in tasks must remain functional. Set `EXTBUILD=` explicitly
to exercise the standalone lane on a machine that also has extbuild. Run
`make doctor` before the first mutating lane, use `make format`, `make lint`,
and `make test` while iterating, and run `make check` for the complete source
checkpoint. Use `make build`, `make bindings`, `make audit`, `make licenses`,
`make package`, and `make release-check` when their affected artifact or
release scope requires them.

Shared public Git dependencies and package or advisory lanes may require
external services. Do not weaken immutable inputs or silently switch sources
when offline. Never claim a lane passed unless it ran successfully; report
network, toolchain, platform, advisory-service, package, or external-artifact
blockers exactly.

Verify exact manifests/lock agreement, generated binding and native artifact
freshness when affected, compatibility and architecture guards,
license/source policy, zero forbidden roots, `git diff --check`, and final
status and diff. Do not treat parent-only workflow proof as a substitute for
standalone repository validation.

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
