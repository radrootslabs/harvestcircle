# Security policy

Report all security-related issues privately to
[security@radroots.org](mailto:security@radroots.org).

Include the affected commit/version, platform, impact and reproduction steps.
Describe expected and observed behavior without exposing real keys, signer
secrets, decrypted private events, credentials or exploitable operational details
in public issues or logs.

Sensitive boundaries include native key custody, FFI/library loading, database
migrations and recovery, replay, relay transport and package provenance. Kotlin
handles opaque identifiers and bounded public data; signing and private keys
remain behind the native boundary.

The application is in early development and makes no production-readiness claim.
