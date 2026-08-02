# Rust and UniFFI boundary

## Status

The proc-macro UniFFI namespace is `radroots_studio_ffi`. Kotlin bindings use
the package `org.radroots.studio.ffi`; generated sources remain build output.

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

The public snapshot DTO carries revisioned lifecycle, account, session, relay,
profile, and safe-error values. It contains no credential field. A generated
account's nsec is confined to the explicit one-time receipt added with the
command boundary.

## Native artifact

The FFI crate builds as both an `rlib` for Rust tests and a `cdylib` for the JVM
binding. Build the current-host development artifact from the capsule root:

```sh
cargo build --manifest-path core/Cargo.toml -p radroots-studio-ffi
```

On macOS this produces
`core/target/debug/libradroots_studio_ffi.dylib`. Other desktop hosts use the
platform-equivalent `radroots_studio_ffi` dynamic-library filename under the
same Cargo profile directory.
