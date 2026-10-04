# Contributing

## Start with the repository contract

Read `AGENTS.md` before changing product behaviour or architecture. When this
repository is consumed by the Radroots monorepo, also read the relevant
normative material under its `docs/oss/harvestcircle/` tree. Standalone changes
must not add normative documentation roots to this repository.

## Runtime ownership

Native presentation belongs in `app/`; product-specific Rust, SQLx persistence,
native signing and OS keyring orchestration belong in `core/`. Root Gradle and
native configuration retain their existing ownership. Keep generated UniFFI
bindings derived from the canonical Rust producer.

The browser bootstrap exists in `web/` with SvelteKit, strict TypeScript,
static output, and web-owned package inputs. Its IndexedDB, sessions, explicit
extension signing, and Applesauce Nostr adapters are separate from native
storage, custody, and FFI. Use Node 24.21.0 and Corepack with pnpm 12.9.1,
then `make web-doctor`, `make web-install`, `make web-check`, and
`make web-build`; `make web-dev` starts the guarded Vite server. Follow
`web/AGENTS.md`; ordinary browser work must not depend
on Cargo, Gradle, native binaries, or parent artifacts.

Shared public conformance vectors are planned under `contracts/interop/`.
They test the exact pinned Radroots semantics rather than establish new shared
protocol policy. Current static-shell conformance runs through `web-check`;
Food/Message interop and real extension/relay qualification remain pending.

## Development flow

1. Use the current development branch policy and work from this capsule root.
2. Make one coherent change at a time.
3. Add or update tests.
4. Run focused checks for the affected runtime and repository boundaries.
   Native `make native-doctor`, `make native-dev`, `make native-check`,
   `make native-build`, and `make native-package-check` alias the existing
   native targets without changing their prerequisites or evidence rules.
5. In the standalone native lane, run `make native-doctor`, then:

```sh
make format
make lint
make test
make check
```

6. Do not hand-edit generated UniFFI code.
7. Do not include secrets or private event plaintext.
8. Document intentional architecture deviations.

Standalone is the default build mode and requires no extbuild or private
parent. Where governed execution is required, run `cargo extbuild doctor`
first and use `BUILD_MODE=governed` on the native targets. Existing
`governed-check`, `governed-integration-check`, `governed-development-check`,
and `governed-linux-x86_64-development-check` select their governed native
lanes. Follow the README for development qualification; package/release checks
remain deferred and require separate candidate authority.

Parent orchestration may invoke these standalone lanes for integration, but
must not become a build/test prerequisite. Keep normative records in the
consuming parent's documentation; do not add `docs/`, `spec/`, `.github/`, or
`.act/` here. Standalone inputs must not require private contracts, unpublished
artifacts, implicit sibling checkouts, or absolute host paths.

Preserve the artifact-v3 exact clean Git-tree archive contract, including
tracked browser source. Do not relabel a native-only filtered bundle as an
exact-tree archive. Record actual revisions and runtime inputs, and distinguish
fresh checks from reused evidence. Production archives and release qualification
remain deferred and unclaimed.

## Commit style

```text
<scope>: <imperative summary>
```

## Public contracts

Changes to FFI, product coordinates, storage migrations, or future Radroots
event contracts require explicit compatibility review.

Preserve native runtime behavior, persisted paths,
installation/keyring identities, schema v3, storage API v3, FFI v4.5, snapshot
v1, migrations, custody, and the exact Radroots source pin. HCAV-021 and later
desktop availability work remain paused. The existing HCP/HCR checkpoints,
product/design acceptance obligations, and eleven planned browser routes remain
in force; repository guidance does not qualify the product or resume them.
