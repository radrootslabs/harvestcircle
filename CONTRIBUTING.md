# Contributing

Rad Roots is an open-source application repository.
Contributions are welcome, including bug fixes, UI improvements, tests, and documentation updates.

## Scope

This repository currently ships a single-package GPUI bootstrap application named `radroots_studio_app`.
Keep the filetree small and direct until a larger application boundary is justified.

## Prerequisites

Install the pinned Rust toolchain:

```bash
rustup toolchain install 1.97.0
rustup override set 1.97.0
```

Confirm your environment:

```bash
cargo --version
rustc --version
```

## Development Commands

Run these commands from the repository root.

```bash
cargo metadata --format-version 1 --no-deps
cargo check -p radroots_studio_app
cargo test
cargo run -p radroots_studio_app
./scripts/check.sh
./scripts/build.sh
./scripts/build.sh --release
./scripts/run.sh
```

## Contribution Guidelines

- Keep changes scoped to a single coherent change.
- Prefer small, reviewable commits.
- Update tests when behavior changes.
- Update documentation when commands or structure change.
- Use repository-relative paths in contributor-facing text.
- Remove obsolete code and dependencies when they are clearly replaced.

## Reporting Issues

When reporting a bug, include:

- your operating system and version
- Rust toolchain version
- the command you ran
- the observed behavior
- the expected behavior

## Submitting Changes

1. Create a branch for your change.
2. Make the smallest coherent update that solves the issue.
3. Run the relevant validation commands from this document.
4. Open a pull request with a clear summary of what changed and how it was verified.
