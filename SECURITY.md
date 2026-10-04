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

The planned browser runtime uses explicit extension identity/signing without
raw private-key custody. Its IndexedDB and sessions are separate from the
native database and keyring. Browser-specific threats include XSS, extension
interaction, private-session teardown, inbox ownership/readiness, and private
message authentication. Applesauce adapters and pure Radroots contract adapters
must follow `web/AGENTS.md`; the browser implementation and its security
qualification remain pending.

Keep private AUTH, decrypted messages, and private unsent plaintext out of
public caches, metadata, logs, analytics, static output, and long-lived UI
state. Browser private unsent plaintext stays in memory. Identity, signing,
and publication require explicit user action; a displayed name or message body
is not identity evidence. Reports and tests must distinguish controlled test
providers from real extension, relay, and independent-client qualification.

Source archives bind the complete exact clean Git tree under native artifact
contract v3, including tracked browser source. Compiled native packages and
future static website output have separate payload boundaries; neither may
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
