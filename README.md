# HarvestCircle

HarvestCircle is an open-source Nostr application for coordinating local-food
buying circles.

The project is in early desktop development. It is not ready for real
commercial use.

## Current foundation

- Kotlin Multiplatform shared application code;
- Compose Desktop host;
- product-specific Rust core through UniFFI;
- local Nostr identity creation and import;
- operating-system keyring custody;
- canonical service-instance persistence through the governed SQLx host;
- configurable Nostr relay bootstrap;
- compatibility-gated native startup;
- reproducible source and development qualification.

## Sovereign direction

The MVP is designed to work without a managed HarvestCircle account or API.

Future work adds canonical Radroots collective-market contracts, private buyer
commitments, a selectable open reference authority, pickup, and proof.

## Contributor entry points

Run commands from the standalone HarvestCircle repository root. The KMP app
uses Kotlin/Compose in `app/`, the product Rust core in `core/`, and generated
UniFFI bindings. Install Git, Make, JDK 21 (selected through `JAVA_HOME`),
and Rust 1.99.0 through rustup. Supported hosts are macOS aarch64 and Linux
x86_64. macOS needs Xcode Command Line Tools; Linux needs a C/C++ toolchain,
OpenSSL development headers, and desktop libraries. The Debian package list is
maintained in [the Linux verification script](tools/run-linux-x86_64-development-check.sh).
Running the UI requires a graphical session; identity custody uses macOS
Keychain or a Linux Secret Service provider.

The checked-in Gradle wrapper downloads its pinned distribution. Make builds
the Rust library and generates UniFFI bindings automatically. The first build
needs network access to the public dependency sources.

```sh
git clone --branch dev https://github.com/radrootslabs/harvestcircle.git
cd harvestcircle
make doctor
make dev       # Compose hot reload
make run       # desktop launch
make build
make check
```

The `native-doctor`, `native-dev`, `native-check`, `native-build`, and
`native-package-check` aliases remain available. Public clones default to
`BUILD_MODE=standalone`, without extbuild or a private parent checkout.

To use external build storage, configure an approved extbuild machine profile
and select governed mode:

```sh
make BUILD_MODE=governed doctor
make BUILD_MODE=governed dev
```

The root Makefile owns all Make configuration. Select a non-default approved
machine profile explicitly when needed:

```sh
make BUILD_MODE=governed EXTBUILD_MACHINE_CONFIG=/absolute/path/to/profile.toml dev
```

Each governed workload runs extbuild doctor first. Cargo and Gradle build output
use the external project directory; Gradle user and build caches use extbuild
locations. Local `.gradle`, `build-logic/.gradle`, and `.kotlin` metadata directories
may be linked into that external project cache by workstation setup. Host paths
and machine policy stay outside portable source.

`make governed-check` and `make governed-integration-check` also select governed
mode for their existing native checks. The active development milestone uses
`make governed-development-check` on macOS aarch64 and
`make governed-linux-x86_64-development-check` for Linux x86_64. These targets
route their build/check commands through extbuild and verify source, runtime,
generated bindings, the public storage API, the exact Radroots source lock,
the single SQLx-selected SQLite linkage, and offline license/source policy.

`native-package-check` retains the full existing package-check prerequisites
and clean-candidate evidence rules; it is not a routine development check.
Package assembly, production source archives, release evidence, network
advisory services, signing, notarization, Nix, and OCI qualification remain
deferred and unclaimed until a release candidate is declared with fresh
authority. Listing a target does not authorize its deferred effects.

## Source checks and provenance

`make repo-check` runs repository source policy. `make check-all` runs both
repository and KMP/native checks with their existing toolchain prerequisites.
`make affected-check BASE=<commit>` reports changed inputs, the native input
fingerprint, and current source policy; it executes no product checks and
never authorizes artifact reuse. Missing tools and history remain explicit.

The native artifact contract v3 requires an exact clean Git-revision tree
source archive. A filtered bundle cannot satisfy that contract. Source
archives and compiled desktop packages have separate payload boundaries.
Production archive and release qualification remain deferred and unclaimed.

## Development branch

Active implementation proceeds on `dev`.

## Local state

HarvestCircle derives one canonical `harvestcircle`/`desktop` runtime context
and stores application state only in its governed `state.sqlite` service
database. SQLx is the sole high-level SQLite library, while
`radroots_service_sqlite` owns connection, authority, migration, integrity,
close, backup, and restore mechanics. The historical `harvestcircle.sqlite3`
file is legacy evidence only and is never imported, repaired, deleted, or
treated as current state.

The platform or development harness must supply the existing canonical state
root. HarvestCircle then uses the runtime context's sealed provisioning plan to
create or validate only `services/harvestcircle/desktop`. The SQLite host makes
the create-versus-existing decision atomically and returns the actual verified
metadata; product storage never probes the database path, recursively creates
roots, repairs existing permissions, or opens a raw SQLx connection.

Online backup capture returns the canonical manifest in memory and writes only
the governed `state.sqlite` member into a caller-selected new directory.
Restore accepts only a digest-bound, identity-bound, size-bounded verified
backup capability, closes the live host, uses the governed marker protocol,
and reopens recovered state before returning. There is no arbitrary database
repair or pathname-only restore authority.

Relay endpoints are explicit inputs validated by the pinned Radroots Nostr
transport policy before any socket work. HarvestCircle owns profile selection
and signature-verified kind-0 interpretation, while the shared transport owns
relay URL, destination, DNS, connection, and bounded-fetch behavior. The
native FFI host owns one runtime per application core and closes it
idempotently. Cancelling a close never reopens command admission, and a later
close call resumes the same shutdown. Operating-system keyring calls run
through an object-safe asynchronous application port and a bounded supervised
worker rather than directly on an async runtime worker. Callers await one-shot
responses; the dedicated operating-system thread alone drives the blocking
platform adapter. Its request queue is fixed at eight entries and credential
mutations carry the caller's canonical UUIDv7 durable request identity. Work
cancelled while still queued has no credential effect; caller loss after work
starts is an unknown outcome reconciled from the durable operation journal.
Shutdown has a fixed 30-second wait and reports success only after the worker
thread is joined; a timeout remains recovery-required and a later close resumes
the same drain.

Credential creation is native and atomic: macOS uses create-only Keychain
insertion, while Linux uses Secret Service creation with replacement disabled.
The stored zeroizing envelope binds the creating durable operation to the
secret. Exact same-operation replay is idempotent only when the complete
envelope matches; another operation conflicts and never overwrites the existing
credential. No compatibility path reads the former plaintext credential shape.

The state database initializes at schema v1 and applies the pinned schema-v2
operation-journal and schema-v3 public evidence migrations before host exposure.
Terminal receipts carry an explicit completion time and remain replayable for
exactly seven days.
Admission caps unfinished operations at 1,024 and all journal rows at 4,096,
deletes at most 256 expired terminal receipts in one transaction, reserves each
accepted operation's terminal row in place, and never evicts an in-window
receipt. The migrations, resulting tables and guards, and all three schema
snapshots are checksum-pinned. Invalid legacy state is refused without replacement;
failed migration transactions roll back atomically.

Public listing versions retain their exact signed wire, full unsigned event
timestamps, tolerant admission result, and first named provenance without
installing an account or local signer. Loads re-verify bounded original wire.
One durable public payload meter admits at most 4,096 versions and 120 MiB of
ordinary logical payload within a 128 MiB total, preserving an 8 MiB recovery
reserve. Exact-ID duplicates consume no additional capacity; no automatic
eviction or recovery bypass is exposed.

## Project documentation

The consuming Radroots monorepo owns normative HarvestCircle specifications,
decisions, handoffs, reviews, and qualification evidence under
`docs/oss/harvestcircle/`. This standalone source tree remains independently
cloneable, buildable, testable, and packageable without private parent code,
contracts, documentation, unpublished artifacts, sibling checkouts, or absolute
host paths. Parent orchestration is optional integration evidence and must
invoke the capsule's standalone commands; it does not replace them. Do not add
`docs/`, `spec/`, `.github/`, or `.act/` roots inside this repository.

## Security

Do not submit secret keys, nsec values, signer secrets, or decrypted private
contracts in issues or logs.

See `SECURITY.md`.

## Licence

HarvestCircle is licensed under GPL-3.0-only. See `LICENSE` and `LICENSES/`.
