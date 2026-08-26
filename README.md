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
- reproducible source and package verification.

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
make package
```

These commands always use the standalone contributor lane. Use
`make governed-check`, `make governed-integration-check`, or
`make governed-package-check` when extbuild-governed output routing is
required. Release, signing, and notarization checks are governed-only.

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
