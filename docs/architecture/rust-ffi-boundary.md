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

Potentially blocking storage and credential operations use Rust blocking tasks;
relay operations use the supervised Tokio runtime. UniFFI exports them as
suspending Kotlin calls. `StudioAppStore` accepts at most one command at a
time, keeps Compose state read-only to consumers, rejects stale observer
revisions, and closes its removal ticket, subscription, gateway, and native
core during application disposal.

The public snapshot DTO carries revisioned lifecycle, account, session, relay,
profile, and safe-error values. It contains no credential field. A generated
account's nsec is confined to the explicit one-time receipt added with the
command boundary.

The exported command surface is bootstrap, snapshot, generate, import, select,
activate, sign out, profile refresh, removal request/confirmation, subscribe,
unsubscribe, and shutdown. `ObserverSubscription.unsubscribe` is idempotent;
`StudioAppCore.shutdown` deregisters remaining observers and signs out before
the generated object handle is closed. Callback tests re-enter `snapshot`,
observe asynchronous profile refresh, and prove no callback arrives after
unsubscribe.

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

Current platform resource prefixes are `darwin-aarch64`, `darwin-x86-64`,
`linux-aarch64`, `linux-x86-64`, `win32-aarch64`, and `win32-x86-64`. The build
stages only the current host artifact; each release platform must build and
smoke its own package rather than reusing another platform's dynamic library.
