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

## Runtime boundaries

The implemented native runtime keeps private keys and signing behind the
UniFFI boundary with operating-system keyring custody. Kotlin handles opaque
identifiers and bounded public data. Native SQLx state, migrations, recovery,
installation identities, and FFI compatibility retain their existing contracts.

Source archives bind the complete exact clean Git tree under native artifact
contract v3, including all tracked source. Compiled native packages have
a separate payload boundary; neither may
include secrets, local databases, private transcripts, or parent documents.
Production archive and release qualification are deferred and unclaimed.

## Expectations

Reports should include:

- affected commit;
- affected platform;
- reproduction steps that do not expose real secrets;
- expected and observed behaviour;
- impact.

The project makes no production-readiness claim during the alpha phase.
