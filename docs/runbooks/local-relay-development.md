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
cargo test --manifest-path core/Cargo.toml -p radroots-studio-storage local_relay_e2e
cargo test --manifest-path core/Cargo.toml -p radroots-studio-ffi ffi_callback_receives_async_profile_refresh
```

Development and tests use local WebSocket relays only. The default development
endpoint is `ws://localhost:8080`; tests bind ephemeral loopback ports. Override
the ordered relay list with comma-separated `RADROOTS_NOSTR_RELAYS` values.

Plain `ws://` is accepted only for `localhost`, `127.0.0.0/8`, and `::1`.
Production remote endpoints require `wss://`. Tests must fail closed rather
than contact public relays when a local fixture is unavailable.

For interactive development, either start a relay at
`ws://localhost:8080` or set `RADROOTS_NOSTR_RELAYS` before `make dev` or
`make run`. Use a comma-separated ordered list when testing more than one
relay. Do not put credentials in this variable. The development launcher sets
the native core to development mode; packaged applications use packaged mode
and therefore require explicit valid relay configuration.

If refresh fails, inspect the safe relay/profile state in the active home
screen. Cached metadata remains visible. Invalid configuration is corrected by
restarting with a valid environment value; there is no in-app relay editor in
this MVP. Local test failures should be reproduced with the focused commands
above before broad workspace validation.
