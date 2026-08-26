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

## Build

Prerequisites include JDK 21, Rust 1.97.1, and platform packaging tools.

```sh
make doctor
make check
make build
make governed-development-check
```

These commands always use the standalone contributor lane. Use
`make governed-check` or `make governed-integration-check` when a narrower
extbuild-governed lane is required. The full active development milestone uses
`make governed-development-check` on macOS aarch64 and
`make governed-linux-x86_64-development-check` for Linux x86_64. It verifies
source, runtime, generated bindings, the public storage API, the exact Radroots
source lock, the single SQLx-selected SQLite linkage, and offline license/source
policy. Network advisory services, package assembly, release evidence, signing,
notarization, Nix, and OCI qualification remain deferred and unclaimed until a
release candidate is declared with fresh authority.

## Development branch

Active implementation proceeds on `master`.

## Local state

HarvestCircle derives one canonical `harvestcircle`/`desktop` runtime context
and stores application state only in its governed `state.sqlite` service
database. SQLx is the sole high-level SQLite library, while
`radroots_service_sqlite` owns connection, authority, migration, integrity,
close, backup, and restore mechanics. The historical `harvestcircle.sqlite3`
file is legacy evidence only and is never imported, repaired, deleted, or
treated as current state.

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
through a bounded supervised worker rather than directly on an async runtime
worker.

## Project documentation

The consuming Radroots monorepo owns normative HarvestCircle specifications,
decisions, handoffs, reviews, and qualification evidence under
`docs/oss/harvestcircle/`. This standalone source tree remains independently
buildable and testable without that documentation tree.

## Security

Do not submit secret keys, nsec values, signer secrets, or decrypted private
contracts in issues or logs.

See `SECURITY.md`.

## Licence

HarvestCircle is licensed under GPL-3.0-only. See `LICENSE` and `LICENSES/`.
