# Contributing

Target pull requests at `dev`. Explain the concrete problem, resulting behavior
and checks performed. Keep changes focused; coordinate changes to storage, FFI,
product identities or dependency boundaries with maintainers before implementation.

Start with [README.md](README.md), [architecture](docs/architecture.md) and the
[development guide](docs/development.md). All public build and contribution inputs
live in this repository. A private monorepo is not a prerequisite.

Use the root Makefile for development. Run focused owning checks while iterating
and `make check` before submitting changes. Include actual commands/results and
any skipped checks or unsupported platform assumptions in the pull request.

Update affected producer contracts, consumers, tests and docs together. Derive
UniFFI bindings from Rust, preserve migration checksums and existing user data,
and keep generated output and secrets out of Git. Preserve exact dependency
pins and lockfiles; make dependency updates deliberate and separately reviewable.

Use `<scope>: <imperative summary>` commit messages. Keep unrelated edits separate.
See [SECURITY.md](SECURITY.md) for private vulnerability reporting.
