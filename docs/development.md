# Development and verification

Run commands from the repository root or use `make -C /path/to/harvestcircle`.
The root Makefile is the only Make configuration surface. `make doctor` diagnoses
selected tooling; the Gradle wrapper and native build use pinned inputs.

| Command | Purpose |
| --- | --- |
| `make dev` / `make run` | Hot-reload development / normal desktop launch |
| `make build` | Build product Rust and the desktop application |
| `make format` / `make format-fix` | Check / apply formatting |
| `make lint` / `make test` | Static analysis / Rust and Kotlin tests |
| `make check` | Aggregate routine source verification |
| `make repo-check` | Repository source and dependency boundaries |
| `make build-logic-check` | Gradle contract and convention-plugin tests |
| `make bindings` | Native library and generated binding consistency |
| `make api-check` | Storage public API comparison |
| `make licenses` / `make audit` | Licence/source policy / network advisory checks |
| `make integration-check` | Native application integration and isolation |

Advanced checks require their corresponding tools: `cargo-deny` for licence and
source policy, `cargo-audit` for Rust advisories, and `cargo-public-api` with the
pinned nightly named in `tools/verify-storage-api.sh` for API comparison. Advisory
checks may need external services. Select these lanes explicitly and report
unavailable tooling rather than interpreting it as a pass.

Shared Lib identity comes from `core/Cargo.toml` and `core/Cargo.lock`. Native
and Gradle producers validate exact public Git source, revision and version
agreement; there is no separate source-lock file or implicit sibling override.

## External build storage

Standalone is the default and uses standard ignored build directories. To select
an approved host profile, add `BUILD_MODE=governed` and, if needed,
`EXTBUILD_MACHINE_CONFIG=/absolute/path/to/profile.toml` to the Make command.
Governed workloads require a successful extbuild doctor and route caches/output
externally. Machine configuration and cache links are workstation-local state.

## Verification boundaries

Source checks inspect every source input, including untracked files and unknown
paths. There is one application runtime; no ownership selection or affected-only
shortcut can exempt files from source safety.

Use tests appropriate to the changed owner and preserve meaningful negative
cases. Native lifecycle tests must use isolated data; tests that deliberately
mutate the real OS credential store are opt-in and are not routine validation.
The existing Linux development script is an explicit Docker/extbuild lane; only
claim platform acceptance when its actual command ran successfully.

Golden PNGs are expected test data; their manifest hashes describe bytes, not a
claim that a host check has run. Regeneration requires reviewing the actual images.

Distribution, signing and notarization have separate prerequisites and candidate
checks. Building the development app does not establish release qualification.
Do not alter identities, bypass clean-source or compatibility checks, or infer
permission to publish from a build command. Keep exact source/lock agreement and
current ABI/API checks when changing producer contracts.
