# Rust and UniFFI boundary

## Status

Initial boundary contract. Generated names and paths will be recorded when the
binding pipeline exists.

## Contract

One Rust `AppCore` instance owns canonical state. Kotlin receives immutable,
revisioned DTO snapshots, invokes explicit commands, and subscribes through a
closeable observer handle. Mutations are serialized, callbacks occur outside
locks, and stale asynchronous results cannot replace newer state.

Normal public DTOs contain no secret. The only exception is the direct,
one-time generated-key receipt. Generated bindings belong under
`build/generated` and are not committed.

Potentially blocking storage, credential, and relay operations must not run on
the Compose thread. Kotlin closes its observer and AppCore during application
disposal. Tests cover native loading, callback ordering, re-entry,
deregistration, cancellation, stale completion, and close races.
