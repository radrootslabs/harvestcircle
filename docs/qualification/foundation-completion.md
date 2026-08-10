# Corrective foundation completion report

## Repository

- Repository: HarvestCircle standalone capsule at the configured public origin
- Branch: `dev`
- Starting SHA: `1f8e5728f4815961d7dac0545c2b03464991b592`
- Qualified source SHA: `5186bd45d9a368a8ae28c76d36bab45569325a26`
- Qualification record: the commit containing this file
- Final status: source and package workflows are locally qualified through the
  consuming monorepo's governed `act` launcher on macOS; the NVD-backed
  dependency scan, Developer ID signing, and notarization remain outstanding

## Commits

| # | SHA | Message | Verification |
|---|---|---|---|
| 01 | `d0d70bceec67` | `repo: publish the HarvestCircle specifications and governance` | Public-boundary tests and governed source checks passed. |
| 02 | `37cf617f7960` | `ci: execute the public verification lanes` | Workflow contract and verification-lane tests passed. |
| 03 | `6405b6a32ac4` | `build: canonicalize product and provenance digests` | Cross-language canonicalization fixtures and governed checks passed. |
| 04 | `f00a0c900f96` | `product: make the coordinate manifest authoritative` | Coordinate mutation, authority, and governed checks passed. |
| 05 | `a6c7eb3daee5` | `ffi: generate Kotlin compatibility expectations` | Clean generation, mismatch tests, and both binding lanes passed. |
| 06 | `8f5156a06cf6` | `runtime: make snapshot delivery gap-aware` | Delivery, resnapshot, failure, and lifecycle tests passed. |
| 07 | `aa1c9b5a62ea` | `policy: require revision-pinned Git dependencies` | Source-policy tests and cargo-deny source checks passed. |
| 08 | `6c61b08a682f` | `release: complete build information and readiness` | Build-information and strict readiness tests passed. |
| 09 | `23ddedd8f498` | `lifecycle: close application scopes on disposal` | Normal, abrupt, and repeated disposal tests passed. |
| 10 | `77c2e8c31294` | `identity: make signer binding access capability-safe` | Capability and unchanged local-signer behavior tests passed. |
| 11 | `56ff9bcaabc2` | `network: type relay destination policies` | Rust, FFI, desktop parser, compatibility, and binding checks passed. |
| 12 | `0fcc40ffb626` | `architecture: record transport and tooling decisions` | Public documentation and foundation-boundary checks passed. |
| QF-1 | `aff09a20eb5b` | `test: stabilize observer qualification timing` | Observer tests passed 20 consecutive runs; the full governed gate passed. |
| 13 | This record | `release: qualify the corrected HarvestCircle foundation` | Complete local matrix recorded below; external gates remain explicit. |
| QF-2 | `5186bd45d9a3` | `ci: return workflow authority to local orchestration` | The capsule gate and both root-owned local `act` workflows passed. |

QF-1 is a qualification-time corrective deviation between planned checkpoints
12 and 13. It changes only test timing and observer-cleanup synchronization; it
does not change the public runtime contract.

## Issue resolution

| Issue ID | Resolution | Tests/evidence |
|---|---|---|
| HC-FC-001 | Published the bounded public specification/governance surface, removed forbidden capsule workflows, and retained portable standalone verification. | Foundation-boundary, archive, standalone-lane, and root-owned local `act` source/package workflows passed. |
| HC-FC-002 | Rust and Gradle now hash a shared semantic canonical form for product coordinates and provenance. | Cross-language vectors passed, including newline and field-order variants. |
| HC-FC-003 | Snapshot delivery is conflated-latest, revision-aware, and gap-recovering. | Duplicate, stale, burst, gap, refresh-failure, unsubscribe, and observer-cleanup tests passed. |
| HC-FC-004 | The product coordinate manifest is the sole approved-value authority. | Mutation propagation and duplicate-authority audits passed. |
| HC-FC-005 | Kotlin compatibility expectations are generated into ignored build output. | Generation freshness, mismatch tests, tracked-output audit, and both binding lanes passed. |
| HC-FC-006 | This public qualification report records local results and remaining external gates. | Report boundary checks and exact-candidate local workflow proof passed. |
| HC-FC-007 | Git dependencies must be immutable revision pins. | Positive source scan and negative policy fixtures passed. |
| HC-FC-008 | Build information is complete and release readiness fails closed. | Unknown, dirty, malformed, mismatched, and exact clean-provenance cases passed. |
| HC-FC-009 | Application-owned scopes and clipboard resources close on normal and abrupt disposal. | Normal, abrupt, repeated, and late-callback tests passed. |
| HC-FC-010 | Local signer binding access is capability-safe. | Optional capability and unchanged local keyring behavior tests passed. |
| HC-FC-011 | Every relay endpoint has a typed destination and read/write policy. | Configuration, DTO, parser, packaged-policy, mixed-development, FFI, and binding tests passed. |
| HC-FC-012 | ADR-0011 records the direct rust-nostr transport decision and re-adoption criteria. | Public documentation and dependency-source checks passed. |
| HC-FC-013 | ADR-0012 records a bounded Detekt compatibility exception and exit criteria. | Public documentation and Kotlin lint/check lanes passed. |

## Canonical digests

- Product digest:
  `93bf10e334e989b20ba5fb8ed05e5d55b83f4502efba5f893aef4dc1a66c8223`
- Provenance digest:
  `db238195b4a5938a8d4d9ac5681c4b125e65c57aa8133ad03e59da4e4bd062bc`
- Foundation baseline:
  `a2038b3e25b9e34f0b8fd001f26a8ed10b5772cb`
- Canonical Radroots revision:
  `09065a610d95e57acdc895a14c07580fa099e7c3`
- LF/CRLF vector result: equivalent semantic inputs using LF, CRLF, no final
  newline, permitted surrounding whitespace, or reordered fields produced the
  same digest. UTF-8 BOM input was rejected as required.

## FFI

- Contract ID: `harvestcircle-desktop-ffi-v4`
- Major: `4`
- Minor: `1`
- Hash:
  `c7a84960e53cd9df35d676bab28294eb048a8b86c766d81cded2635b64a7f3d6`
- Snapshot schema: `1`
- Storage schema: minimum `5`, current `10`
- Generated Kotlin source:
  `app/desktop/build/generated/uniffi/kotlin/org/harvestcircle/ffi/harvestcircle_ffi.kt`
  (ignored and reproducible; the governed extbuild lane writes the equivalent
  path under its routed build root)

## Change delivery

- Conflation strategy: Kotlin consumes a bounded, conflated-latest stream.
- Gap detection: duplicate and stale revisions are ignored; a change whose
  previous revision does not equal the accepted revision is a gap.
- Resnapshot behavior: a gap calls `currentSnapshot()` and accepts only a
  refreshed revision at or beyond the announced revision. Failure is surfaced
  as a typed application problem.
- Burst test: passed, including delayed consumers, stale/duplicate delivery,
  resnapshot, observer failure, unsubscribe, and cleanup behavior.

## BuildInfo

- Product: HarvestCircle `0.1.0-alpha`; distribution package `1.0.0`
- Toolchains: Rust `1.97.1`, Gradle `9.5.0`, Java `21.0.11`, Kotlin `2.4.10`
- Compose Multiplatform: `1.11.1`
- Registry state: typed `NotApplicable`
- Exact-candidate provenance: source
  `5186bd45d9a368a8ae28c76d36bab45569325a26`, clean source, Radroots revision
  `09065a610d95e57acdc895a14c07580fa099e7c3`, source epoch `1786389775`
- Release-ready result: passed for the exact clean candidate. Missing or
  malformed provenance failed closed as designed.

## Local workflow proof

- Authority: workflow definitions are forbidden in this OSS capsule. The
  consuming monorepo owns `harvestcircle-source.yml` and
  `harvestcircle-package.yml` under its root `.act/workflows/**` surface.
- Launcher: the guarded root launcher requires `act 0.2.89`, rejects arbitrary
  lanes, maps only the local macOS runner, derives provenance from the clean
  nested Git repository, and invokes this capsule's Make targets.
- Source result: passed for `5186bd45d9a368a8ae28c76d36bab45569325a26`
  through `make source-check` in approximately 90 seconds.
- Package result: passed for the same source through `make package-check` in
  approximately 98 seconds and produced `HarvestCircle-1.0.0.dmg`.
- Platform scope: macOS is proven on this machine. Linux and Windows package
  production require corresponding local hosts or explicitly governed local
  virtualized runners; no remote workflow result is claimed.

## Commands

| Command or lane | Result | Notes |
|---|---|---|
| `make doctor` | Pass | Extbuild routing and project configuration were healthy. |
| `make format` | Pass | Repository formatting gate passed. |
| `make lint` | Pass | Rust and Kotlin lint gates passed. |
| `make test` | Pass | Full repository test surface passed. |
| Governed and standalone `make check` lanes | Pass | The standalone lane confirms contributor builds do not require extbuild. |
| Governed and standalone binding lanes | Pass | UniFFI bindings and generated compatibility sources were reproducible. |
| `make build` | Pass | Governed build completed. |
| `make licenses` | Pass | Licence report and policy completed. |
| `make source-check` | Pass | Source provenance and source-policy checks completed. |
| `make package` | Pass | macOS produced and verified `HarvestCircle-1.0.0.dmg` (approximately 72 MiB). |
| `make package-check` without provenance | Expected fail | Failed closed because the source commit was unknown. |
| `make package-check` with exact candidate provenance | Pass | Clean source SHA, Radroots revision, and source epoch above were injected explicitly. |
| Root `cto.harvestcircle.all` | Pass | Local `act` source and package jobs both completed successfully on macOS. |
| Rust format, workspace check, Clippy, and workspace tests | Pass | Exact candidate; locked workspace; all targets for Clippy; warnings denied. |
| `cargo deny` source and licence checks | Pass | Revision policy and licence policy passed. |
| Product, compatibility, generated-source, boundary, archive, lane, provenance, shared desktop, and desktop Gradle verification | Pass | Actual scoped Gradle tasks passed together on the exact candidate. |
| Four final prohibited-pattern audits | Pass | No matches in their governed scopes. |
| Generated-output tracking audit | Pass | Desktop/shared/buildSrc outputs and `core/target` were ignored and untracked. |
| `git diff --check` | Pass | No whitespace errors. |
| `make audit` | External blocker | The shared workstation RustSec cache had inconsistent advisory paths; it was not mutated. |
| Isolated fresh RustSec database scan | Pass with warning | No actionable vulnerability failure; `instant 0.1.13` was reported as unmaintained through rust-nostr. |
| OWASP dependency analysis | External blocker | No NVD API key was available and the feed update made no progress during the bounded run; the run was stopped without weakening policy. |

Two task names in the planning matrix were stale. Repository policy is
integrated into the foundation-boundary task, and generated-source verification
is scoped to the desktop project. The actual authoritative tasks were run and
passed.

## Signing/notarization

- Signing: blocked by external release credentials. The local application is
  ad-hoc signed; no Developer ID Application signature is claimed.
- Notarization: blocked by external release credentials and service access.
  The local DMG has no stapled notarization ticket.

## Deviations

- Qualification exposed two load-sensitive observer-test assumptions: a fixed
  cleanup delay and an unrealistically short local-relay timeout. QF-1 waits
  for the observable cleanup condition and uses a bounded two-second relay
  timeout. Twenty consecutive focused runs and the full governed gate passed.
- The planned public-contract and generated-source Gradle task names did not
  match the implemented authority. Their integrated/scoped equivalents were
  run and passed; no check was removed.
- The shared RustSec cache failure was isolated from source correctness. A
  fresh official advisory database supplied supplemental evidence without
  mutating shared workstation state.
- The NVD-backed scan remains externally blocked by feed access. It is not
  represented as green.
- Package readiness intentionally requires explicit source provenance. The
  unqualified invocation failed closed, and the exact-candidate invocation
  passed.
- The original remote workflow design was superseded by the repository-wide
  rule that forbids `.github/**` in every OSS capsule. QF-2 removes those files
  and moves orchestration to guarded root-owned local `act` workflows without
  moving build behavior out of this capsule.

## Unresolved issues

- Push the qualification lineage to `origin/dev` under separate authorization.
- Exercise Linux and Windows package production on corresponding governed
  local hosts or local virtualized runners if cross-platform package proof is
  required before release.
- Provide reliable NVD feed access or an API key, then rerun the OWASP-backed
  dependency analysis.
- Provide Developer ID credentials and notarization service access before any
  release claim that requires signed and notarized macOS media.
- Update this evidence after any additional platform or external release gates
  complete.

## Safe to begin next handoff

Yes, for separately authorized product-shell source work. The exact source and
macOS package workflows are proven locally through the governed root `act`
launcher. This does not authorize a production release, and the external
security-feed, signing, notarization, Linux, and Windows gates above remain
explicit.
