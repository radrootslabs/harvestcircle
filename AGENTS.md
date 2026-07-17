# Rad Roots Application - Agent Specification

## 1. Scope and hierarchy

- This file applies to the full repository.
- Keep this file concise, durable, and repository-specific.
- If a closer directory-level `AGENTS.md` is added later, it overrides this file for that subtree.

## 2. Repository identity

- This repository is the standalone Rad Roots application repository.
- Treat it as a public open-source repository with a direct GPUI-native application focus.
- Preserve the repository's top-level identity files and keep the root easy to understand without mount-path context.

## 3. Change discipline

- Prefer the smallest coherent change that fully addresses the request.
- Do not mix unrelated cleanup or speculative refactors into the same change.
- Remove obsolete code and scaffolding when they are clearly replaced.

## 4. Before editing

Before making substantial changes:

- Read this file, `README.md`, and `CONTRIBUTING.md`.
- Inspect `git status --short` before broad edits, refactors, or file removals.
- Read the current implementation before changing behavior.
- Use checked-in commands and docs as the source of truth.

## 5. Validation and command surface

- Run validation from the repository root.
- Prefer the narrowest relevant validation first.
- Use documented commands before inventing new ones.
- Current canonical commands are:
  - `cargo metadata --format-version 1 --no-deps`
  - `cargo check -p radroots_studio_app`
  - `cargo test`
  - `cargo run -p radroots_studio_app`
  - `./scripts/check.sh`
  - `./scripts/run.sh`

## 6. Repository structure

- Keep the repository root as the package root.
- Keep the structure minimal until a durable new boundary is required.
- Do not reintroduce deprecated egui-era scaffolding.

## 7. Rust engineering rules

- Use Rust `1.97.0`, edition `2024`, and safe Rust only.
- Keep state, data flow, and side effects explicit.
- Avoid hidden panics in non-test code.
- Keep code readable and direct.

## 8. Commit and handoff rules

- Format commits as `<scope>: <imperative summary>`.
- Use lowercase scopes.
- Keep handoff summaries clear and standalone.
- In handoff, state what changed, what validation ran, and any remaining risks or assumptions.

## 9. Definition of done

- The requested change is implemented.
- Obsolete scaffolding is removed when clearly replaced.
- Relevant validation ran, or a concrete blocker is reported.
