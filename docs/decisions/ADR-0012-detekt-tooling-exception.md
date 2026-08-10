# ADR-0012: Retain the Detekt alpha compatibility exception

- Status: Accepted with expiry
- Date: 2026-08-10
- Owner: HarvestCircle maintainers
- Review date: 2026-11-10

## Context

The build pins Kotlin `2.4.10`, Gradle `9.5.0`, and Detekt
`2.0.0-alpha.5`. Detekt's official compatibility table reports alpha.5 with
Gradle `9.5.1` and Kotlin `2.4.0`, while stable Detekt `1.23.8` is reported
with Gradle `8.12.1` and Kotlin `2.0.21`.

Sources:

- <https://detekt.dev/docs/introduction/compatibility/>
- <https://detekt.dev/changelog-2.0.0/>

Moving to the stable Detekt line would therefore require unrelated Kotlin and
Gradle downgrades rather than a tooling-only replacement.

## Decision

Retain Detekt `2.0.0-alpha.5` as a narrowly scoped build-tooling exception.
It is used for static analysis only and is not linked into the HarvestCircle
runtime or packaged application.

The reviewed risks are alpha API and rule behavior changes, possible plugin
incompatibility, and non-final defaults. They are contained by exact version
pinning, checked-in configuration, deterministic Gradle verification, and the
repository's lint and full-check lanes.

## Expiry and upgrade trigger

Re-evaluate this exception no later than 2026-11-10, or earlier when a stable
Detekt release officially supports Kotlin 2.4.x and Gradle 9.x. Replace the
alpha when that stable release:

1. runs with the pinned Kotlin and Gradle toolchain without unrelated
   downgrades;
2. preserves or intentionally migrates the checked-in rule configuration;
3. passes `make lint`, `make check`, and both governed and standalone
   verification lanes; and
4. produces no new runtime or packaging dependency.

If no compatible stable release exists at review time, maintainers must
record a new dated review rather than silently extending this exception.
