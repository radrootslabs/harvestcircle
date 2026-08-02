# ADR 0003: Account metadata and secret storage

## Status

Accepted.

## Decision

Store migration-managed, non-secret account metadata and profile cache in
bundled SQLite at an injectable OS application-data location. Store local
secret keys only in the OS credential store through Rust `SecretStore`, using
service `org.radroots.studio.nostr` and canonical public-key hex as the account
key.

Use a non-secret operation journal because SQLite and the credential store
cannot share an atomic transaction. Publish snapshots only after durable
success. There is no ordinary-file secret fallback.

Use `refinery 0.9.2` with bundled `rusqlite 0.39.0`. Refinery's supported
Rusqlite range ends at 0.39, so this compatibility pin replaces the initially
reviewed 0.40.1 candidate.

## Consequences

Startup recovery is mandatory. Add/import and removal need failure injection at
every cross-resource boundary. Generated nsec is a one-time receipt exception;
imported key text has a documented JVM String limitation.
