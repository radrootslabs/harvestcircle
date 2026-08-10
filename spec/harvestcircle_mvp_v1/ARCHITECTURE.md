# Architecture

```text
app:shared
  KMP common application models, presenters, shared Compose
        ↓ platform-neutral runtime interface
app:desktop
  desktop host, generated UniFFI adapter, JNA/AWT, packaging
        ↓
Rust HarvestCircle application/runtime/storage/Nostr/FFI crates
        ↓
canonical public Radroots libraries
```

Rust owns canonical identity, persistence, operation, Nostr, compatibility,
and future commercial protocol state.

Kotlin shared code owns presentation state and platform-neutral use cases.

Generated FFI types remain in the desktop adapter.

No silent fallback, duplicated commercial model, or UI-thread blocking.

## Relay bootstrap

Every relay endpoint carries an explicit destination (`Local`,
`PrivateNetwork`, or `Public`) and independent read/write capabilities. Rust
owns URL, destination, uniqueness, and capability validation; the desktop host
only adapts environment input into the typed UniFFI record.

Development input uses deterministic comma-separated entries:

```text
HARVESTCIRCLE_NOSTR_RELAYS=local|ws://127.0.0.1:8080,public|wss://relay.example
```

The `private|` prefix selects `PrivateNetwork`. Current desktop entries enable
both reading and writing. Missing packaged configuration remains a visible
degraded-network state and never invents or reclassifies a relay.
