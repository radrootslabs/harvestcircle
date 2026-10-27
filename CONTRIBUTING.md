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

## Development flow

1. Use the current development branch policy and work from this capsule root.
2. Make one coherent change at a time.
3. Add or update tests.
4. Run focused checks for the affected runtime and repository boundaries.
   `make native-doctor`, `make native-dev`, `make native-check`,
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

Public clones default to standalone mode without extbuild or a private
parent. Select `BUILD_MODE=governed` and an optional
`EXTBUILD_MACHINE_CONFIG` directly on the root Make command; see README.
Where governed execution is required, run `make governed-doctor`
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
all tracked source. Do not relabel a filtered bundle as an
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
product/design acceptance obligations remain
in force; repository guidance does not qualify the product or resume them.
