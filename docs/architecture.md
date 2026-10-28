# Architecture

HarvestCircle has one native desktop runtime. Kotlin Multiplatform and Compose
own presentation and client coordination. The Rust workspace owns domain rules,
application state, Nostr adapters, persistence, native lifecycle and UniFFI.
Shared Radroots packages are immutable public Git dependencies, not sibling paths.

The [food-hub MVP contract](mvp.md) defines the intended candidate operational
surface. Demo-context setup and reads use native owner/session authorization.
Native receipt commands and the shared receiving/Stock workspace implement the
first physical workflow; later stock workflows remain unfinished. No server, shared network database file or cloud
evidence service is required for the selected demo.

## Source map

| Path | Responsibility |
| --- | --- |
| `app/shared` | Shared UI, presenter, navigation and application contracts |
| `app/design_system` | Product design components and bundled font resources |
| `app/desktop` | Desktop host, native loading and generated binding consumers |
| `core/crates` | Product Rust implementation, FFI and test-only adapters |
| `build-logic` | Gradle conventions, generation and packaging checks |
| `tools/design_catalog` | Design development application |

Domain rules keep I/O out. Effects belong in adapters and owned runtime work.
Presentation code does not establish identity, permissions or persistent truth.
Production graphs must exclude test bridges and fixture controls.

## Compatibility and data

The current storage schema is 7, FFI is 5.5 and snapshot schema is 1. Rust and
Kotlin compatibility inputs are checked before opening application state.
Generated bindings and native libraries must come from the same producer.
There is no ambient-library fallback.

The runtime context uses service `harvestcircle`, instance `desktop`, and exact
`state.sqlite`/`state.lock` names. SQLx is the high-level SQLite authority. The
explicit development state root must already exist. For the normal interactive
profile, the native host prepares the resolved user data anchor before the sealed
runtime directory plan provisions its service suffix. Preparation uses held
directory descriptors, rejects symlinks and unsafe ownership/permissions, creates
missing directories with mode 0700, syncs parent entries and revalidates identities.
It never creates a missing home directory, repairs existing permissions or removes
data. Compatibility and relay validation precede preparation. The SQLite host owns creation/opening, migration,
locking, integrity, backup, restore and close. Legacy `harvestcircle.sqlite3`
files remain untouched and unsupported.

Migrations are immutable. Public evidence retains original signed wire and first
provenance, independently of installed identities. Its 4,096-version/120 MiB
ordinary-growth bounds preserve an 8 MiB reserve within 128 MiB. Durable commands
preserve terminal receipts for seven days, with 1,024 unfinished/4,096 total rows
and bounded expiry cleanup. These are identity-operation bounds, not a stock
idempotency policy. Inventory operations retain separate durable
uniqueness for the lifetime of their history. Unknown outcomes require
original-operation recovery.

Reference data uses a separate append-only version table, independent of removable
accounts and seven-day identity receipts. A protected transaction checks the
current native owner, exact canonical v1 command intent, catalog/record revisions
and finite capacity before recording the version and original operation receipt.
Original receipts retain actor, time and before/after values through later edits
and reopen. Changed intent under a committed operation ID conflicts. Operation
lookup reads the original result; absence after restore is unresolved and never
causes replay. The actor also requires the current credential for writes. The immutable sample context records
Whole-pack demo v1 with an explicit UTC site timezone; reads validate this stored
basis before projecting reference data. It does not assert reviewed site compliance.

Reference pages are limited to 64 records and bind continuation to the catalog
revision. History pages are also limited to 64; damaged values are bounded before
hydration and revalidated in Rust. Typed input/conflict/capacity/permission errors
are distinct from an uncertain command outcome. The latter retains the original
operation ID for explicit result lookup. The shared Compose reference workspace uses these native queries and commands.
Its UI-scope presenter retains drafts across navigation, freezes submitted input,
and rejects late responses after an identity change. A stale form requires
explicit current-version review; uncertain outcomes expose only original-result
lookup. These reference interfaces do not create physical stock. Durable
admission and recovery of pre-commit submissions across restart/restore remain
part of the unfinished recovery surface.

Whole-pack receipts commit original trace/reference versions, a Receiving position,
pending put-away work, a physical event and an immutable operation result in one
protected transaction. Planned locations do not create positions or completed work.
Source identity includes issuer, kind, original code and provenance; equal codes
never merge sources. New trace/group/source IDs are deterministically namespaced
from the native original operation. Existing-source selection must already resolve
and retain its complete original identity. Current owner checks precede credential
access and repeat inside SQL. Original-result replay is checked before fresh
catalog versions and capacity; changed intent conflicts. Current Stock pages show
current product/location names while retaining original receipt labels and versions.
Pages are bounded to 64 rows and pin both stock and reference revisions. Original
rows are bounded before decoding.
The decoder reuses pure receipt planning and validates the stored intent hash.
The bounds are 4,096 traces and 32,768 events; stock history has no identity-receipt expiry.
Native submitted-operation admission across restart/restore is still the unfinished
cross-feature recovery gate. The shared receiving workspace freezes original
submission intent, retains unsubmitted input during navigation and uses read-only
original-result lookup for uncertain outcomes. Current stock and catalog versions
are reviewed explicitly before resubmitting known rejected input. Kotlin renders
native stock/work projections; it does not calculate authoritative balances.

Identified handling uses the same lifetime operation journal and physical event
history. A forward migration preserves schema 6 receipts, counts and original
work targets, then replaces its single work row with requirements and scoped
allocations. Each physical group has one active location and a uniform status
for each attributed requirement. Moving part requires an observed distinguishable
portion; generated IDs do not prove physical identification. Native transactions
conserve quantity, complete work only on first qualifying arrival and retain
original movement results independently of later positions or work progress.

Replanning retires only remaining work and creates a separate requirement. Explicit
new handling cannot overwrite pending work. Discrepancy resolution preserves the
original observation and changes neither stock nor work. Current native group,
work and discrepancy queries are bounded and versioned. Work pages contain one
row per requirement, rather than one copy per stock position. The native limits
are 16,384 groups/requirements, 256 groups per requirement, 64 requirements per
group and 4,096 discrepancies. A derived causal time floor follows each physical
portion; an unknown occurrence does not erase earlier constraints or invent an
actual time. Shared Kotlin transport and the desktop native adapter expose these
closed commands and projections without calculating authoritative stock or work.
Generated/native consumer tests exercise original results, restart, owner access,
replanning, explicit new handling and discrepancy follow-up. The shared handling
workspace exposes current groups, requirement queues, actual movement, replanning,
explicit new work and discrepancy follow-up. It retains unsubmitted observations
across navigation and freezes submitted input until a known result or explicit
known-rejection review. Stale scope requires a separate current-data review;
uncertain outcomes permit original-result lookup without another mutation. Loading
removes stale queue rows and disables unavailable actions. These in-memory draft
controls do not replace the unfinished native admission/session/restore gate.

Pure portion policy shares conserved partition and occurrence-bound mechanics
with movement, while a separate split plan changes neither location nor work.
Physical lineage uses immutable group creation revisions, including empty
retained ancestors, to resolve captured population scope. A branch detached
before capture is excluded unless selected; only later descendants of included
groups inherit scope. Overlapping selections deduplicate current group IDs.
The bounds are 256 groups per lineage and 256 scopes per union. The native
adapter must load complete validated lineage and current authority; these pure
values never grant permission. Independent split persistence/FFI/UI and actual
acceptance/hold policy are subsequent implementation work.

## Secrets, transport and lifecycle

Private keys and signing stay in Rust behind opaque handles. OS custody uses
create-only operations and same-operation envelope verification. The native host
owns one Tokio runtime and a bounded eight-entry keyring worker per core lifetime;
shutdown is explicit, resumable and bounded to 30 seconds. Caller cancellation
does not invent proof that an already-started external mutation did not happen.

Desktop opening has one owner through Opening, Ready, Failed and Closing states.
Closing during a synchronous native open never abandons its eventual handle;
shutdown must be confirmed before exit or another open. A failed cleanup retains
ownership and offers explicit force exit. Native constructors join their workers
before returning an ordinary retryable failure. Metadata is checked before core
acquisition. Expected failures become safe application problems; cancellation and
VM fatal failures retain their semantics. Diagnostic lines contain only stage,
code, a host-generated random attempt ID and the compiled product version.

An absent relay list opens the existing offline configuration. No public relay
or ambient endpoint is inferred. Explicit endpoints retain strict classification,
count, URL and transport validation; network operations still require configured
relays. Development keeps its explicit localhost default.

Relay destination/DNS/connection policy comes from the pinned shared transport.
Product code selects explicit profiles and validates product observations. Keep
private material out of logs, filenames and long-lived UI state. Sensitive
clipboard writes are explicit, bounded and ownership-aware. Backup/restore use
sealed identity/digest/member-bound capabilities, never arbitrary repair paths.

These are existing implementation boundaries, not permission to perform live
credential, relay, recovery or publication operations during ordinary checks.

## Local demo hub context

The explicit native demo constructor selects a named, private data root under
`demos/<run>/data` below the resolved interactive anchor. Ordinary startup keeps
its existing root and cannot initialize demo hubs. Demo opening defaults offline.
Run names are bounded single path components; provisioning retains the normal
ownership, no-symlink and no-repair checks. A database profile is separate from
the OS keyring: use a dedicated demo identity rather than an existing credential.

Storage migration 4 adds the single hub context and immutable actor history with
no foreign keys to removable account rows. The serialized native actor derives
current ownership from the active session, verifies custody before initialization,
and rejects stale generations or other identities. Setup uses an original stable
operation ID and atomically writes its context/history; retry returns the same
result. Hub metadata remains after identity removal. UI and stock operations are
still being implemented; the native policy is not a complete food-hub workflow.
