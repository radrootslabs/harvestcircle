# Nostr key management

## Status

Implemented MVP security contract.

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
Explicit copy places the nsec in the operating-system clipboard, which is an
additional user-authorized exposure. A lifecycle-owned timer clears it after
60 seconds only if the clipboard still contains the copied value; user-replaced
content is preserved. Imported key input crosses an unavoidable JVM `String` boundary once;
the masked Compose draft is cleared immediately when the command is accepted,
before the native coroutine executes. The in-flight JVM argument cannot be
guaranteed zeroized and is never logged or added to public state.

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

Startup reads the non-secret operation journal before restoring public state.
An empty journal performs no keyring operation. Removal recovery retries an
intent, continues honestly from `CredentialDeleted`, completes metadata and
account-namespace cleanup, persists deterministic fallback selection, and then
finalizes the entry. Keyring failure leaves the phase unchanged for retry.

Add/import records intent before credential creation. After credential write it
records `CredentialWritten`, writes public metadata and selection, records
`MetadataWritten`, then finalizes. A metadata failure attempts credential
compensation; a failed compensation leaves enough non-secret journal state for
startup recovery. Removal records intent, signs out if necessary, deletes the
credential, records `CredentialDeleted`, deletes public and account-owned data,
records `MetadataDeleted`, persists fallback selection, and finalizes.

Corrupt or inaccessible SQLite fails safely and is not silently recreated.
Locked or unavailable credentials do not become watch-only accounts. Platform
credential behavior must be checked on macOS, Windows, and Linux; the real
keyring smoke remains ignored by default because it mutates user credential
state.
