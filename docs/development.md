# Development and verification

Run commands from the repository root or use `make -C /path/to/harvestcircle`.
The root Makefile owns command configuration and target dependencies;
`scripts/commands.sh` implements their leaf actions. Lifecycle shell scripts live
under `scripts/`; Rust tooling and the design catalog live under `tools/`.
Use Make as the public entry point, including from a consuming monorepo; its parent
Makefile does not manage HarvestCircle's application runtime. No local Make include
or second command dependency graph is supported. `make doctor` diagnoses
selected JDK, Rust components and native compiler prerequisites before a workload.
It reports installation commands for missing tools. The Gradle wrapper may
download its pinned distribution; prerequisite checks never install host tools.

| Command | Purpose |
| --- | --- |
| `make dev` / `make run` | Hot-reload development / normal desktop launch |
| `make dev-check` | Finite launch-classpath and isolated native startup/shutdown checks |
| `make build` | Build product Rust and the desktop application |
| `make format` / `make format-fix` | Check / apply formatting |
| `make lint` / `make test` | Static analysis / Rust and Kotlin tests |
| `make check` | Aggregate routine source verification |
| `make repo-check` | Repository source and dependency boundaries |
| `make build-logic-check` | Gradle contract and convention-plugin tests |
| `make bindings` | Native library and generated binding consistency |
| `make licenses` / `make audit` | Licence/source policy / network advisory checks |
| `make integration-check` | Native application integration and isolation |

Advanced checks require their corresponding tools: `cargo-deny` for licence and
source policy and `cargo-audit` for Rust advisories. Advisory
checks may need external services. Select these lanes explicitly and report
unavailable tooling rather than interpreting it as a pass.

Shared Lib identity comes from `core/Cargo.toml` and `core/Cargo.lock`. Native
and Gradle producers validate exact public Git source, revision and version
agreement; there is no separate source-lock file or implicit sibling override.

## External build storage

Standalone is the default and uses standard ignored build directories, including
`.gradle/user-home` for its Gradle user home. This isolates builds from ambient
workstation caches and Gradle init scripts. Set `GRADLE_USER_HOME` explicitly to
use a different cache; explicit settings are validated and never silently replaced.
`make doctor` reports the selected home and diagnoses broken directory links,
non-directory paths and inaccessible cache ancestors before starting Gradle.
It does not repair global caches or create replacement symlink targets. To select
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
Run `make BUILD_MODE=governed linux-check` for the Docker/extbuild Linux lane; only
claim platform acceptance when its actual command ran successfully.

Golden PNGs are expected test data; their manifest hashes describe bytes, not a
claim that a host check has run. Regeneration requires reviewing the actual images.

Distribution, signing and notarization have separate prerequisites and candidate
checks. Building the development app does not establish release qualification.
Do not alter identities, bypass clean-source or compatibility checks, or infer
permission to publish from a build command. Keep exact source/lock agreement and
current FFI and storage checks when changing producer contracts.

Internal Rust APIs evolve with their workspace consumers during MVP development;
there is no exact public-API snapshot or nightly API-tool requirement. Compiler,
behavioral, storage/migration and generated FFI compatibility checks remain active.
