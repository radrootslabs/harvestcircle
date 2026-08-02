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

Gradle exposes the same operation as `:app:desktop:buildRustCore`. The task
tracks Rust manifests, lockfile, sources, and migrations as inputs and the
current-host debug dynamic library as its output. It does not redirect Cargo's
target directory.

`:app:desktop:generateUniFfiKotlin` runs the repository-pinned UniFFI 0.32
generator against that dynamic library and writes Kotlin to
`app/desktop/build/generated/uniffi/kotlin`. The main Kotlin source set reads
that generated directory, and `compileKotlin` depends on generation. Generated
bindings are ignored build output and are never committed.

Development and tests point JNA at `core/target/debug`. Packaging stages the
current-host library under JNA's platform resource prefix inside the desktop
resources, allowing the packaged JVM runtime to extract and load it without a
machine-specific absolute path. `NativeLoaderTest` crosses the generated ABI
and verifies the native crate version without opening storage or credentials.
