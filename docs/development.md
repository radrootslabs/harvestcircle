# Development and verification

The [food-hub MVP contract](mvp.md) records implemented behavior and unfinished
workflows. make demo opens an isolated named profile, and make hub-check exercises
implemented native hub components and UI regressions. Receiving, handling and
physical-portion and sample acceptance/hold/release editors are implemented;
later MVP workflows and complete qualification remain unfinished. The commands
below are the public lifecycle entry points. Do not use private planning material,
machine state or a sibling checkout to run the project.

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
| `make dev-check` | Finite launch-classpath, fresh default-profile UI and isolated native lifecycle checks |
| `make build` | Build product Rust and the desktop application |
| `make format` / `make format-fix` | Check / apply formatting |
| `make lint` / `make test` | Static analysis / Rust and Kotlin tests |
| `make check` | Aggregate routine source verification |
| `make repo-check` | Repository source and dependency boundaries |
| `make build-logic-check` | Gradle contract and convention-plugin tests |
| `make bindings` | Native library and generated binding consistency |
| `make licenses` / `make audit` | Licence/source policy / Rust advisory checks |
| `make hub-check` | Focused Rust hub rules/SQLite/runtime/FFI plus shared UI and native integration |
| `make integration-check` | Native application integration and isolation |

Advanced checks require their corresponding tools: `cargo-deny` for licence and
source policy and `cargo-audit` for Rust advisories. Advisory
checks may need external services. Select these lanes explicitly and report
unavailable tooling rather than interpreting it as a pass.

Shared Lib identity comes from `core/Cargo.toml` and `core/Cargo.lock`. Native
and Gradle producers validate exact public Git source, revision and version
agreement; there is no separate source-lock file or implicit sibling override.

## First launch and recovery

Normal startup prepares the canonical user data location automatically. On macOS
it is `$HOME/Library/Application Support/Radroots/data`; on Linux it is the
Radroots directory under the resolved XDG data home. The app preserves existing
data and permissions. Explicit development data roots must still exist and be
canonical; they are not a substitute for testing the default user profile.

Opening runs in the background. For unavailable local data, check disk/mount
availability and directory permissions, close other HarvestCircle instances, then
choose **Try again**. Compatibility or native-loading failures require a rebuild
or reinstall. Startup never resets the database, deletes a lock or automatically
retries. Closing waits for owned startup/shutdown; an exceeded safe timeout offers
an explicit **Force exit** choice.

**Copy diagnostics** copies a safe stage/code/attempt/version record. The same
record is written to standard error without raw exceptions, filesystem paths,
relay input or secret material. Clipboard failure does not block closing. Security
reports go to [security@radroots.org](mailto:security@radroots.org).

A normal launch without `HARVESTCIRCLE_NOSTR_RELAYS` opens offline. Network
operations require explicitly configured relays. `make dev` retains the localhost
development default; no connection is inferred for an ordinary packaged launch.
`make dev-check` runs a separate JVM with a fresh HOME and no development-data
override, checks first opening and reopening through the real application factory,
and verifies a missing-library failure can still close safely. The ordinary
`make check` includes these regressions and startup ownership/cancellation tests.

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

Visual tests compare rendered output with checked-in golden PNGs on macOS aarch64.
Use `make design-goldens-update` for intentional visual changes and review the
resulting images. No second hash or source-path manifest needs updating.

Basic macOS/Linux packages are available for development. Production signing,
notarization and release certification are deferred until distribution requirements
are established; a development package is not a production release.
Do not alter identities, bypass clean-source or compatibility checks, or infer
permission to publish from a build command. Keep exact source/lock agreement and
current FFI and storage checks when changing producer contracts.

Internal Rust APIs evolve with their workspace consumers during MVP development;
there is no exact public-API snapshot or nightly API-tool requirement. Compiler,
behavioral, storage/migration and generated FFI compatibility checks remain active.

The Rust advisory lane remains available explicitly. Gradle conventions do not
load an advisory database/scanner during ordinary development. Review Kotlin/JVM
dependency changes and their advisories deliberately; automated JVM advisory
scanning is deferred until a suitable distribution workflow is established.

Internal implementation refactors are checked through compilation and behavioral
tests rather than exact source fragments. Structural audits retain private storage
modules, create-only custody and read-only verification boundaries.

## Isolated demonstration

`make ui-check` runs Kotlin/Compose checks and tests with the native build inputs.
It is a focused frontend lane; `make check` also runs the Rust/repository checks.

Run `make demo DEMO_RUN=hub-preview` to deliberately open a persistent, named
demo profile. Run names contain 1–48 lowercase ASCII letters, digits or hyphens,
beginning with a letter. Reusing a name resumes its state; another name creates
a separate profile. This is runtime data under the ordinary application anchor,
not a build cache. There is no automatic reset or keyring deletion.

The demo defaults offline and never imports ordinary profile credentials. Create
a new local Nostr identity and complete the existing recovery-key acknowledgement
before initializing the hub. Work, Stock and Dispatch are the primary surfaces;
Network, Settings and the Nostr reference utility remain available. Native Rust
authorizes hub setup and reference reads/writes. After setup, open Reference data
to add fictional products, original packs, parties and locations. Pack definitions
and location purposes are immutable; label changes retain history. Unknown command
outcomes offer original-result lookup, while stale edits retain their input for
explicit review. Receive original packs records an actual count at a Receiving
location; a planned put-away destination does not move stock. Work shows native
required/completed/no-longer-required/remaining counts, and Stock provides bounded
location/search pages and original receipt details. Unsubmitted receipt input
survives navigation; unresolved submissions retain their original operation for
read-only result lookup. Unimplemented stock actions are labelled as such.
An unresolved setup result retains its original operation ID; use Check local hub
to read the durable result rather than submitting a new setup.

Dispatch provides preparation and immutable identification. Choose a recipient and
whole current source groups, then record your reviewed observation. For part of a
group, first record observed physical separation in Stock. Issue records the saved
material; changed material must first be revised. Replacement links a new issued
version and retains the superseded original. Mark unused closes the active issue
without changing stock, work or holds. Preview exposes original source certainty,
issuer/provenance, receipt clocks and recipient versions. Unknown outcomes keep the
original operation for lookup. Paperwork never records an actual departure; actual
departure and the rest of the candidate lifecycle are still being implemented.

The same explicit selection is accepted by the desktop executable as
`--demo hub-preview`. The packaged demonstration is qualified separately before
release; adding this launch path alone does not qualify a complete demo workflow.
