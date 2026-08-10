# Contributing

## Start with the specification

Read:

```text
spec/harvestcircle_mvp_v1/
AGENTS.md
```

before changing product behaviour or architecture.

## Development flow

1. Use the current development branch policy.
2. Make one coherent change at a time.
3. Add or update tests.
4. Run focused checks.
5. Run:

```sh
make format
make lint
make test
make check
```

6. Do not hand-edit generated UniFFI code.
7. Do not include secrets or private event plaintext.
8. Document intentional architecture deviations.

## Commit style

```text
<scope>: <imperative summary>
```

## Public contracts

Changes to FFI, product coordinates, storage migrations, or future Radroots
event contracts require explicit compatibility review.
