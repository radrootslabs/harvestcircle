# ADR-0010: Snapshot streams are conflated and gap-aware

Status: Accepted

Application changes contain complete snapshots.

The transport may conflate intermediate values, but must preserve the latest
state, check delivery failure, validate predecessor revisions, and resnapshot
on gaps.
