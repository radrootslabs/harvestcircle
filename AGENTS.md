# HarvestCircle development

HarvestCircle is a standalone public KMP/Compose desktop application with a
product Rust core and generated UniFFI bindings. Read README.md and the public
docs before changing its architecture or behavior.

## Ownership

- `app/`: Kotlin presentation, application coordination and desktop lifecycle.
- `core/`: product domain, application, runtime, persistence, Nostr and FFI.
- `build-logic/`: Gradle conventions and native integration.
- `scripts/`: lifecycle command dispatch, prerequisites and shell checks.
- `tools/`: Rust engineering tools and the design catalog.
- `config/` and `contracts/`: current machine-readable build inputs.
- `docs/`: public architecture and contributor guidance.

Keep shared Radroots policy in its independently published libraries. Use exact
public Git revisions; never add implicit sibling dependencies or private inputs.
This repository must build independently of a consuming monorepo. Public docs
and `.github` community files are allowed; private plans, agent/session state,
credentials and orchestration records do not belong here. CI is not implemented.

## Implementation boundaries

Keep domain rules and persistent state in Rust; Kotlin is presentation and client
orchestration. SQLx is the SQLite authority. Keep private keys/signing behind the
native boundary, with opaque identifiers and bounded public DTOs in Kotlin.
Preserve runtime paths, application/keyring identities, migration checksums and
stored data. See [architecture](docs/architecture.md) for current contracts.
Native libraries must match generated bindings, ABI, product and storage inputs;
never accept an ambient or stale binary. Generated bindings and native binaries
are build output and must remain untracked. Change the Rust producer first.

Keep lifecycle work bounded, cancelable and explicitly owned. Do not log secret
material or infer external effects from environment state. Preserve unknown
outcomes, idempotent recovery and current permission checks.

## Commands and changes

The root Makefile owns lifecycle targets, dependencies and configuration;
`scripts/commands.sh` owns command execution. Public submodule runtimes are managed
through this Makefile, independently of a parent repository. No local include files
or workstation paths. Standalone is the default. Select governed execution with
`BUILD_MODE=governed` and an optional `EXTBUILD_MACHINE_CONFIG`; a successful
extbuild doctor must precede governed workloads. Build output stays external in
that mode. Tool installation and machine policy are separate host concerns.

Inspect instructions, relevant contracts/tests and Git status before edits.
Preserve unrelated work and independent Git owners. Make small coherent changes,
verify through owning Make commands, and review the final diff/status. Report
actual checks, skipped checks and platform limits. Never weaken a check merely
to obtain a passing result. Keep dependency locks and generated input evidence
aligned. Do not hand-edit wrapper binaries or generated bindings.

Use `<scope>: <imperative summary>` commits. Commit, push, publish, deploy, sign,
change credentials or mutate external settings only within the user's authority.
Security reports go to [security@radroots.org](mailto:security@radroots.org).
