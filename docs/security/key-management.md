# Nostr key management

## Status

Initial security contract. Implementation-specific recovery phases and
platform results will be added with their checkpoints.

## Storage boundary

Production Nostr secret keys are stored only through Rust `SecretStore` in the
OS credential store. The service is `org.radroots.studio.nostr`; the credential
account key is canonical public-key hex. There is no ordinary-file fallback.

Secrets are forbidden in SQLite, profile cache, operation journals, public
snapshots, normal DTOs, logs, errors, filenames, preferences, fixtures, and
golden files. Rust application secret wrappers are non-cloneable, redacted,
non-serializable values.

Generated nsec is returned once in a direct operation receipt, displayed in
non-saveable Kotlin state, and cleared after acknowledgement or disposal.
Explicit clipboard copy uses conditional delayed clearing. Imported key input
crosses an unavoidable JVM String boundary once and is cleared immediately.

Keyring unavailability is a safe recoverable error. Cross-resource operations
publish no partial success and use a non-secret journal for restart recovery.
