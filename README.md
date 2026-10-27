# HarvestCircle

HarvestCircle is an open-source Nostr application for coordinating local-food
buying circles. It is in early desktop development and is not ready for real
commercial use. The current application supports local Nostr identities, OS
keyring custody, a SQLx-backed state database and configurable relay bootstrap.

## Get started

Install Git, Make, JDK 21 (selected through `JAVA_HOME`) and Rust 1.99.0 through
rustup, including its `clippy` and `rustfmt` components:

```sh
rustup toolchain install 1.99.0 --component clippy --component rustfmt
```

Supported hosts are macOS aarch64 and Linux x86_64. macOS needs Xcode
Command Line Tools. Linux needs a C/C++ toolchain, OpenSSL development headers,
desktop libraries and a graphical session; see the package list in the
[Linux verification script](tools/run-linux-x86_64-development-check.sh).
Identity custody uses macOS Keychain or a Linux Secret Service provider.

```sh
git clone --branch dev https://github.com/radrootslabs/harvestcircle.git
cd harvestcircle
make doctor
make dev       # Compose hot reload
# Or: make run
```

The checked-in Gradle wrapper downloads its pinned distribution. Make builds
the Rust library and generates Kotlin bindings automatically; the first build
needs network access to the public dependency sources. A standalone checkout
needs no private parent repository, sibling library checkout or extbuild tool.
Standalone Make commands use `.gradle/user-home` unless `GRADLE_USER_HOME` is
explicitly selected, keeping workstation Gradle state separate.

## Development

```sh
make build
make check
```

Use [CONTRIBUTING.md](CONTRIBUTING.md) and the
[development guide](docs/development.md) for focused verification commands.
The [architecture guide](docs/architecture.md) explains source ownership,
native compatibility, local data and security boundaries. Work proceeds on `dev`.

The root Makefile owns all Make configuration. For external build storage:

```sh
make BUILD_MODE=governed EXTBUILD_MACHINE_CONFIG=/absolute/path/to/profile.toml dev
```

The selected host profile must pass extbuild doctor. Host paths and machine
policy stay outside portable source; no local Make include is supported.

## Security and licence

Report security issues privately to [security@radroots.org](mailto:security@radroots.org).
Do not post secret keys or private event contents in public issues.

HarvestCircle is GPL-3.0-only; see [LICENSE](LICENSE). Bundled Inter fonts use
OFL-1.1; their [copyright and licence](app/design_system/src/commonMain/composeResources/files/licenses/inter-OFL-1.1.txt)
are included in the application resources.
