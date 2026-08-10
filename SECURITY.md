# Security policy

## Reporting

Use GitHub private vulnerability reporting when it is available.

Do not publish secret keys, nsec values, NIP-46 secrets, decrypted private
contracts, or exploitable operational details in a public issue.

## Scope

Security-sensitive areas include:

- key generation and recovery;
- operating-system keyring custody;
- FFI compatibility;
- native library loading;
- database migrations and recovery;
- operation replay;
- relay transport;
- private Nostr events;
- package provenance;
- dependency policy.

## Expectations

Reports should include:

- affected commit;
- affected platform;
- reproduction steps that do not expose real secrets;
- expected and observed behaviour;
- impact.

The project makes no production-readiness claim during the alpha phase.
