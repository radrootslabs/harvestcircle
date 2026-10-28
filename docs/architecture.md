# Architecture

HarvestCircle has one native desktop runtime. Kotlin Multiplatform and Compose
own presentation and client coordination. The Rust workspace owns domain rules,
application state, Nostr adapters, persistence, native lifecycle and UniFFI.
Shared Radroots packages are immutable public Git dependencies, not sibling paths.

The [food-hub MVP contract](mvp.md) defines the intended candidate operational
surface. Hub commands, permissions and inventory rules will extend this same
native architecture; those features are not implemented yet. No server, shared
network database file or cloud evidence service is required for the selected demo.

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

The current storage schema is 3, FFI is 5.0 and snapshot schema is 1. Rust and Kotlin compatibility inputs are checked before opening application
state. Generated bindings and native libraries must come from the same producer.
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
idempotency policy. Planned inventory operations require separate durable
uniqueness for the lifetime of their history. Unknown outcomes require
original-operation recovery.

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
