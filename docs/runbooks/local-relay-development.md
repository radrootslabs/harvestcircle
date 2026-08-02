# Local relay development

Rust reads the ordered comma-separated relay list from
`RADROOTS_NOSTR_RELAYS`, trims entries, validates the WebSocket policy, and
deduplicates while preserving first-seen order. Development and tests may use
`ws://localhost:8080` when the variable is absent or empty. Packaged builds
have no fallback and report `InvalidRelayConfiguration` until at least one
valid relay is configured.

The Rust integration lane starts an in-process relay on an ephemeral loopback
port, publishes a signed kind-0 event, fetches it through the production SDK
adapter, and shuts both clients and the relay down before returning. Run it
without any public-network dependency:

```sh
cargo test --manifest-path core/Cargo.toml -p radroots-studio-application sdk_client
```

Development and tests use local WebSocket relays only. The default development
endpoint is `ws://localhost:8080`; tests bind ephemeral loopback ports. Override
the ordered relay list with comma-separated `RADROOTS_NOSTR_RELAYS` values.

Plain `ws://` is accepted only for `localhost`, `127.0.0.0/8`, and `::1`.
Production remote endpoints require `wss://`. Tests must fail closed rather
than contact public relays when a local fixture is unavailable.
