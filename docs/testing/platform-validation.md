# Platform validation ledger

## Command authority

The capsule root `Makefile` is the complete human-facing lifecycle. It invokes
Cargo with `core/Cargo.toml` explicitly and invokes the checked-in Gradle
wrapper. This standalone `oss/**` repository does not use extbuild, validation
scripts, or `.github/**` workflows.

| Target | Contract |
| --- | --- |
| `make doctor` | report Java, Cargo, and Gradle toolchains |
| `make format` | check Rust formatting |
| `make lint` | deny Rust workspace and all-target Clippy warnings |
| `make test` | run the full Rust and Kotlin/Compose/native-loader suites |
| `make check` | run format, lint, test, and Gradle check |
| `make build` | build the Rust workspace and desktop application |
| `make bindings` | generate Kotlin bindings and stage the host native library |
| `make dev` | launch Compose hot reload in development relay mode |
| `make run` | launch Compose Desktop in development relay mode |
| `make package` | build the current-host desktop distribution |
| `make clean` | remove Gradle and Cargo build outputs |

## Current-host result

Validation date: 2026-08-02.

- Host: macOS 26.5 build 25F71, arm64.
- Java: Eclipse Temurin 21.0.11 LTS.
- Cargo: 1.97.1 with the workspace Rust 1.97 toolchain contract.
- Gradle: wrapper 9.5.0.
- Native resource: `darwin-aarch64/libradroots_studio_ffi.dylib`.
- Package format: macOS DMG with application name `Radroots`, bundle ID
  `org.radroots.studio`, installer-compatible version `1.0.0`, and the
  generated squircle application icon. macOS `jpackage` rejects a leading-zero
  app version, so this field cannot encode `0.1.0`; application and runtime
  artifacts keep the full `0.1.0-alpha` prerelease version.

The final validation checkpoint records the exact green command results and
artifact inspection. Interactive `make dev` and `make run` are launchers and
are reviewed but not held open during automated validation. `make clean` is
destructive to recoverable build outputs and is not required for acceptance.

Checkpoint 62 results:

| Command | Result |
| --- | --- |
| `make check` | passed Rust format, all-target Clippy, Rust workspace tests, Kotlin/Compose/native-loader tests, and Gradle check |
| `make build` | passed Rust workspace and desktop builds |
| `make bindings` | passed UniFFI generation and current-host native staging |
| `make package` | produced `Radroots-1.0.0.dmg` successfully |
| packaged app inspection | bundle ID and installer version matched; code signature verified; application jar contained the arm64 dylib and both icon resources |

## Remaining platform matrix

Linux x86-64/aarch64 and Windows x86-64/aarch64 resource-prefix selection is
implemented but not validated on this macOS host. Each platform must run
`make check`, `make build`, `make bindings`, the real keyring smoke in an
isolated credential account, and its native packaging smoke. A package must
contain only its matching JNA resource prefix and native filename. Cross-built
artifacts are not accepted as substitutes for host loader and credential-store
tests.
