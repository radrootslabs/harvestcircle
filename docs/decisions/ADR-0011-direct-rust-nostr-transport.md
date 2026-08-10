# ADR-0011: Use direct rust-nostr profile transport

- Status: Accepted
- Date: 2026-08-10

## Context

HarvestCircle previously reached Nostr through both the shared Radroots
transport crates and the rust-nostr SDK. Commit
`1f8e5728f4815961d7dac0545c2b03464991b592` removed the redundant shared
transport dependency chain and its duplicate registry-sourced rust-nostr
graph. No specific vulnerability identifier is claimed by this decision.

The profile adapter now depends directly on `nostr`, `nostr-sdk`, and the
test-only `nostr-relay-builder`, all pinned to rust-nostr revision
`5bba5163eb77107f82c4a8262cf29d7f33a73219`.

## Decision

HarvestCircle owns its current profile-fetch transport policy locally in
`radroots_harvestcircle_nostr`. The adapter uses the pinned rust-nostr SDK
directly and preserves these application-visible behaviors:

- only relay endpoints with read capability are queried;
- each readable endpoint receives a bounded kind-0 profile filter;
- the overall operation has a deadline;
- returned events are checked for signature, author, kind, and valid profile
  metadata;
- the newest valid event is selected deterministically; and
- partial and complete outcomes remain distinct, including per-relay failure
  evidence.

Local mock-relay tests exercise these semantics without making external relay
availability part of the test contract.

This is a profile-transport decision only. Domain relay classification and
capability policy remain owned by `radroots_harvestcircle_domain`, and
application orchestration remains owned by
`radroots_harvestcircle_application`.

## Consequences

The direct dependency reduces duplicate transport and dependency surfaces,
but HarvestCircle must maintain the adapter, its retry/deadline behavior, and
its tests. Future collective-market transport is not required to use this
profile adapter; it must be selected against the collective protocol and
privacy requirements when those contracts are implemented.

## Re-adoption criteria

A reusable transport abstraction may replace this adapter only when it:

1. uses one revision-pinned and policy-compliant rust-nostr graph;
2. preserves the typed endpoint capability and destination boundary;
3. preserves bounded querying, validation, deterministic selection, and
   partial-success evidence;
4. does not introduce private or product-external dependencies into this
   public capsule; and
5. passes the capsule's source, dependency, Rust, binding, and integration
   verification lanes.
