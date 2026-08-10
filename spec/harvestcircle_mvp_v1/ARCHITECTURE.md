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
