# ADR 0001: Rust core and UniFFI

## Status

Accepted.

## Decision

Implement the canonical Radroots Studio account and application runtime in a
Rust workspace under `core/**`. Expose immutable snapshots, explicit commands,
safe errors, and closeable observer subscriptions to Kotlin through UniFFI.
Compose Desktop remains a thin native JVM 21 lifecycle and presentation shell.

## Consequences

Account behavior is reusable by future shells and cannot diverge in Kotlin.
The build must generate Kotlin bindings, stage a platform native library, test
callback and object lifetime behavior, and package that library with the app.
