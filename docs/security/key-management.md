# Nostr key management

## Status

Initial security contract. Implementation-specific recovery phases and
platform results will be added with their checkpoints.

## Storage boundary

Production Nostr secret keys are stored only through Rust `SecretStore` in the
OS credential store. The service is `org.radroots.studio.nostr`; the credential
account key is canonical public-key hex. There is no ordinary-file fallback.

The production adapter uses keyring 4.1.6 with the native macOS Keychain,
Windows Credential Manager, or freedesktop Secret Service backend selected by
the crate. Unsupported, locked, inaccessible, malformed, and platform-failure
outcomes become a stable `KeyringUnavailable` error. No alternative storage is
attempted. The real keyring smoke test is ignored by default because it mutates
the invoking user's credential store and must be run explicitly on each target.

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

The journal contains only an operation identifier, operation kind, canonical
public key, phase, safe timestamp, and optional safe diagnostic code. It cannot
store credential text or arbitrary payloads, and it survives account metadata
removal until cleanup has been finalized.

Workspace redaction tests scan public snapshot and safe-error debug output plus
the SQLite schema and representative durable records for known secret-hex,
nsec, and secret-prefix fixtures. These guards run before account commands are
allowed to carry production credentials.
