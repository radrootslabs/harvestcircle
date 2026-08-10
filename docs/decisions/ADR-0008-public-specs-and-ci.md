# ADR-0008: Public specs and local verification are repository requirements

Status: Accepted

HarvestCircle is an open-source, spec-anchored project intended for public
review and an OpenSats application.

Durable specs, decisions, and qualification evidence belong in the repository.
The standalone capsule owns portable Make, Gradle, and Cargo verification
commands, but it does not own workflow definitions under `.github/**` or
`.act/**`.

The consuming monorepo may invoke those commands through governed local-only
workflows under its root `.act/**` surface. Those workflows add orchestration,
not build behavior, and are not a substitute for running the standalone
commands directly.
