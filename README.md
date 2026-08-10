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

## Development branch

Active implementation currently proceeds on `dev`.

## Specifications

The durable product contract is under:

```text
spec/harvestcircle_mvp_v1/
```

Accepted architecture and tooling decisions are under `docs/decisions/`,
including the [direct rust-nostr transport decision](docs/decisions/ADR-0011-direct-rust-nostr-transport.md)
and the [Detekt compatibility exception](docs/decisions/ADR-0012-detekt-tooling-exception.md).

## Security

Do not submit secret keys, nsec values, signer secrets, or decrypted private
contracts in issues or logs.

See `SECURITY.md`.

## Licence

HarvestCircle is licensed under GPL-3.0-only. See `LICENSE` and `LICENSES/`.
