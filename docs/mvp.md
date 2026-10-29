# Food-hub demonstration MVP

This is the accepted candidate product contract. Isolated demo launch, native
hub-owner/reference setup, whole-pack receiving, identified movement, quantity work,
sample acceptance/independent holds, discrepancy follow-up, immutable identification
and Work/Stock/Dispatch navigation are implemented alongside
existing Nostr identity, custody, persistence and startup. Later operational
workflows remain intended behavior. Existing
functionality remains supported while those workflows are developed.

## Release scope

The candidate demonstrates a single site's original whole-pack receipt-to-dispatch
workflow with fictional data, one operator and one authoritative desktop. Work,
Stock and Dispatch are primary navigation; identity, Network and Settings
remain reachable. Qualification targets a packaged macOS app on the development
Mac and the existing Make development loop. Preserve existing Linux support.

Include receiving, physical locations/movement, identifiable portions, acceptance,
holds/release, put-away work, actual departures, immutable identification, history,
trace and local output. Include count reconciliation, opening stock, restricted
returns, dispositions, conflicting actual reports and linked corrections.

Defer server/shared-device operation, network database files, mobile, Pro, AI,
media processing/cloud objects, commercial integrations/imports/acknowledgements,
orders, reservations/ATP, pricing/accounting, repacking, variable-weight inventory,
multi-site operation and general workflow/team/privacy administration. Retain the
existing Rust/UniFFI/SQLite runtime. No new shared-library implementation or
protocol is required. Existing immutable dependency pins remain unchanged.

This is a candidate demonstration, not an operational cutover or compliance
certification. Actual hub validation, procedures, external distribution, signing
and notarization have separate acceptance. No delivery date is implied.

## Product contracts

### C01: Demo lifecycle and sample data

Provide explicit isolated demo launch through the child root Makefile and
scripts/commands.sh. Persist the demo profile under approved runtime-data routing,
separate from ordinary user state and build caches. Show sample-data status.
Use a newly generated demo identity through the existing backup acknowledgement
flow. A separate database does not isolate OS-keyring entries: the current keyring
account is public-key-based under the shared application service. Never copy/import
ordinary profile credentials as fixture setup or automatically delete keyring data.
Starting another run creates a new owned profile and retains earlier runs. Resume
is deliberate; no automatic reset, lock deletion or destructive fixture cleanup.
Packaged demo selection must be explicit and preserve canonical path/ownership,
compatibility and startup checks. It is not an auth or test-control bypass.

### C02: Identity, authority and privacy

Preserve generation/reveal/backup acknowledgement/cancel/expiry, import, activation,
selection/switching, sign-out, removal, profiles, relay configuration, keyring
custody and missing-credential recovery. Private keys/signing stay native. Relay
connectivity is not required for local operation; demo defaults offline without
changing normal development relay policy or inferring public endpoints.
Explicit setup binds one local owner identity to the hub. Rust derives and checks
current hub/session authority for private reads and writes; public-key possession,
selection, read-only entry, account labels and UI visibility confer no hub grant.
Keep existing Nostr utilities reachable for other identities without hub access.
Reject stale-session commands and prevent late results/drafts crossing accounts.
Removing an identity retains independent historical public-key attribution and
hub records, with a consequence preview. Permit recovery by original-key import
or explicit handover to another verified local identity; never silently reassign
ownership. No multi-user role designer or food-safety certification is implied.
Nostr availability is a separate model, not physical inventory. Never implicitly
publish hub stock, holds, parties or recipients to relays or mint stock from
received availability events. Future protocol adoption is separate scope.

### C03: Model, quantities and source truth

Use stable opaque identities for hub/site context, product/original-pack variant,
party, source reference, receipt trace unit, identifiable stock group, location,
position, task/issue, hold, departure, count, issued version and linked amendment.
A source code includes issuer/type/provenance; supplier, producer and issuer are
independent. Preserve original text, explicit unknowns and possible/established
source distinctions. Do not invent history or conflate populations.
Use bounded whole integers and checked arithmetic, including decoding, sums and
multi-line effects. Counts carry units; unlike variants are not aggregated as
fungible stock. Nominal weight is metadata, not measured mass. Pack-size change
creates another variant rather than rewriting history. Define finite command,
query/page, text and output bounds in the public contract and test limit failures.

The pure reference policy lives in `core/crates/harvestcircle_domain/src/hub/reference.rs`.
It permits at most 512 current references, with 64 rows per reference page and a
16,384-version history bound for the persistence adapter. Labels are at most 120
UTF-8 bytes, source codes 128 and provenance 512. Original source text is preserved;
blank text, controls and bidi overrides are rejected. IDs use 32 lowercase hex
characters for 16 nonzero opaque bytes. Nominal mass is optional metadata bounded
to 1..1,000,000,000 grams. A counted quantity is 0..1,000,000 original packs with
checked arithmetic; quantities from different pack variants cannot be summed.
Pack definitions and location purposes are immutable; a changed definition creates
another identity. Label revisions retain the complete before/after versions.
Reference versions and committed original-operation receipts now persist under
the native owner/session/credential boundary, with atomic replay and finite
history enforcement. Native pages and original-result/history reads are bounded.
The shared reference workspace provides bounded search, original-pack product
selection, versioned label editing and history. A submission freezes its original
intent; uncertain results offer read-only lookup. A stale edit retains input and
requires explicit review of current versions before a new submission. Native
Rust remains the authority for validation, permission and persistence.

### C04: Physical events, portions and eligibility

Persist operational events and authoritative balances/history atomically; no
generic event-sourcing framework or unrestricted set-balance command is needed.
On hand equals opening plus intakes/positive adjustments minus actual departures,
dispositions and negative adjustments. Moves/splits conserve quantity and origin.
Normal commands cannot make positions negative or silently choose an unknown source.
Partial treatment needs physically distinguishable scope. Splits preserve quantity,
lineage and applicable holds; no cross-trace merge. Acceptance, custody, location,
restrictions, eligibility and work are independent. Rust computes eligibility over
affected physical groups, counting overlapping restrictions once. Selected-hold
release does not grant acceptance or release another hold. Explicit sample policy
can permit internal relocation/disposition while blocking normal buyer departure.
Use versioned sample handling basis, not a general rule engine or implicit expiry.

Captured physical selections bind the native stock revision and selected whole
groups. Already-detached descendants stay outside a selection unless explicitly
included; portions subsequently separated from the selected population inherit
its scope. Later descendants of an excluded branch remain excluded. Overlapping
selections count current physical groups once. Unknown or older incomplete
history cannot resolve to an empty success. The pure portion/lineage rules live
in `core/crates/harvestcircle_domain/src/hub/portions.rs`. Native storage/runtime
and FFI5.6 now support explicit same-location separation, immutable original
results and complete validated lineage with pinned pages. The shared independent
separation editor retains observed count/identifier/time input and freezes an
uncertain submission. The lineage inspector shows original creation/parentage,
current on-hand positions and retained empty ancestors, with stock/catalog-pinned
continuations. Kotlin renders these native projections without resolving scope or
calculating authoritative quantities.

Pure sample acceptance/hold policy lives in
`core/crates/harvestcircle_domain/src/hub/treatment.rs`. It defaults to review
required and computes departure eligibility from accepted groups minus their
active hold union, retaining original scope, sample basis and separate clocks.
Acceptance and selected-hold release never clear each other. The existing sample
basis permits internal relocation, observed separation and explicit disposition
under native ownership; normal recipient departure requires acceptance and no
active hold. The bounds are64 selected groups and256 primary decisions/holds per
trace, with one retained release per hold. At primary history capacity, normal
departure fails closed because another precautionary hold cannot be recorded;
physical treatment totals stay distinct from command readiness. These projections
carry no current authority or future mutation grant. Native schema9/FFI5.7 now
persist and recover acceptance, independent holds and selected whole-hold release.
Current owner/credential checks precede mutations; release-capacity reservations
share the mutation transaction. Original results retain captured whole-pack scope,
actors, basis and separate clocks through later handling and restart. Bounded
current snapshots and pinned history expose native counts and action readiness.
Shared editors now record explicit acceptance, independent holds and selected
whole-hold release. Operators choose whole groups by their native physical labels
and locations; partial treatment requires observed separation first. Current
restricted/eligible counts, original scopes, separate clocks and native action
readiness remain inspectable. Drafts retain selected groups, decision, hold,
occurrence and observation through ordinary navigation; uncertain submissions
freeze the original intent. Context review is explicit and history continuation
pins both native revisions. Native durable admission/session/restore recovery
and full MVP qualification remain unfinished.

Receipt planning is implemented as pure Rust in
`core/crates/harvestcircle_domain/src/hub/stock.rs`. It captures original reference
versions and manual observations, preserves independent supplier/producer/issuer
attribution and separates occurrence from recording time. Its initial position is
at Receiving; the planned destination creates no stock or completed work. The
inventory contract bounds traces to 4,096, operational events to 32,768 and read
pages to 64. Native receipt commands now persist the trace, Receiving position, pending work,
physical event and original result atomically. Stock/original-operation reads are
bounded and owner-protected. The shared receiving form and Stock/Work views use
these native commands and projections. Original receipt details retain observed
source certainty, historical labels and separate occurrence/recording clocks.
Durable pre-commit/restart recovery and later physical workflows remain unfinished;
receiving alone does not qualify the complete demo.

### C05: Work and time

For quantity work: required = completed + no longer required + remaining. Count
actual applicable completion once. Repeated/backward moves, replanning and issue
closure cannot fabricate progress; a new handling requirement becomes explicit
new work. Departure/shortage at Receiving can remove remaining put-away without
completing it. Closing a discrepancy does not release an independent hold.
Separate actual occurrence/certainty from native recording time and record order.
Retain unambiguous instants/offsets and site timezone. Uncertain intervals/unknown
times remain explicit. Record order is not proof of historical occurrence; a later
hold does not establish an earlier breach. Sample requirement versions preserve
their basis and original clocks without asserting legal applicability.

The pure movement and work policy lives in
`core/crates/harvestcircle_domain/src/hub/movement.rs`. A partial move requires an
observed physical identifier and basis for the separated portion. Whole-group
moves preserve identity; credited work follows the identified portion through
return and repeated moves. Replanning retires the original remaining quantity
without completing it and creates a distinct pending requirement. A later new
handling requirement is explicit and cannot silently replace pending work.
Discrepancy resolution is follow-up only and has no stock/work/restriction effect.
The native adapter must enforce complete scope and identity uniqueness within
16,384 retained groups, 64 requirements per group and 256 groups per requirement.
Native storage, authorization and FFI implement these handling rules. The shared
workspace records whole-group movement or an observed identifiable partial move,
replans all remaining work on a requirement, creates explicit new handling and
retains discrepancy observations after resolution. Work uses one native row per
requirement. Unsubmitted input survives ordinary navigation; submissions freeze
their original input, stale scope requires explicit review and uncertain outcomes
offer read-only original-result lookup. Native durable admission and session/
restore recovery remain unfinished; the demonstration is not release-qualified.

### C06: Commands, concurrency and durable outcomes

Inventory has its own durable operation model, separate from identity kinds and
seven-day receipt retention. Define operation uniqueness scope, schema/canonical
intent, original actor and preconditions. Changed intent under an existing identity
conflicts. Current authority and all stock/hold/work preconditions are checked at
the protected transaction boundary, using the existing sealed SQLx host. Atomically
commit the physical/work/history effects and operation result. No external I/O
belongs inside that transaction. Competing stale forms cannot overwrite facts.
Return typed recorded/rejected/conflict/pending/uncertain outcomes and resolve the
original operation after timeout. Retain stock operation uniqueness throughout the
demo history; never silently prune it with identity receipts. Reopening/restore
changes runtime/session context. Preserve known receipts; do not automatically
replay stale submissions or infer an action never happened because an older
restored history lacks its receipt. Unknown prior outcomes require explicit review.
Do not create a new operation identity merely to escape uncertainty.

### C07: Counts, opening, actual reports and correction

Counts are observations with population/location, cutoff and observer. Posting is
separate and considers intervening activity; unresolved comparison blocks an
unjustified adjustment. Original receipt counts survive linked adjustments.
Opening stock is an idempotent counted opening with known/unknown history, never a
fabricated historical receipt. Returns are new restricted intakes, optionally
linked to the original departure; financial reversal is outside scope. Disposition
records actual removal. Preserve already-taken conflicting reports before review.
Normal planned-action rejection must not discard them. Reconciliation distinguishes
established breach, earlier departure and uncertain timing. Unknown sources remain
unresolved with visible affected scope; no invented stock or arbitrary attribution.
Corrections preserve originals, before/after, actor/basis, balanced effects and
downstream recipient/document follow-up. Known departures cannot become generic
loss. Source/recipient/quantity amendments use bounded specialized paths; no free
historical overwrite or unrestricted Undo after dependent activity.

Pure count policy lives in `harvestcircle_domain::hub::counts`. Observation plans
capture the original population, pack/location versions, recorded count, cutoff
and observer without changing stock or work. A fresh comparison rejects changed
scope, newer reconciliation/observation, uncertain or pre-activity cutoffs,
unresolved actual reports and uncertain prior outcomes. Reviewed shortages retire
only pending work; completed work and original intake remain. A surplus requires
separate physical identification and plans new counted stock with unknown source
and review-required acceptance, preserving the earlier population's treatment.
This policy foundation is not yet persisted or exposed through native commands
or count/opening forms. Native persistence, opening stock and complete lifecycle
qualification remain unfinished.

### C08: Preparation, departure, documents and trace

Draft preparation reserves nothing. One actual departure has one recipient and
explicit product/source-position lines. Revalidate every line, acceptance/holds,
versions and required identification; failure leaves no partial shipment. Issued
identification is an immutable snapshot. Changed material identity/recipient/quantity
needs a linked replacement under the sample procedure; retain unused/superseded
versions. Issue/print/reprint/export never posts physical departure. Reprint uses
the issued version, not current reference data. Local output success/cancel/failure
is independent of the physical event; no external acceptance/delivery claim.
The pure preparation/identification policy lives in
`core/crates/harvestcircle_domain/src/hub/dispatch.rs`. Preparation selects whole
current physical groups; a smaller population first uses observed separation.
Drafts reserve no stock, and two drafts may select the same source. Issuance checks
current command versions and reviewed material; changed physical groups or material
require review, while unrelated journal activity does not change the document.
Issued recipient/source/reference snapshots remain immutable. Material replacement
links a new version and supersedes the prior issue; marking unused is explicit and
retains the as-issued content. Limits are 64 lines and 32 identification versions
per preparation. These plans do not authorize departure. Native schema 11 and
FFI 5.11 persist preparation, revision, issuance, replacement and unused closure.
Original recovery retains exact immutable material/reference/source history through later edits and
restart without substituting current positions. The journal reserves one closure
slot per active issue alongside outstanding hold releases. Native pages carry
summaries and paired revision pins; selected records carry complete bounded version
history. The shared Dispatch workspace now prepares/revises whole-group selections,
issues identification, creates linked replacements and explicitly marks unused
versions. Preview shows captured recipient/source facts and clocks. Drafts retain
input through navigation and explicit context review; uncertain submission freezes
the original operation for read-only recovery. Reviewing current physical facts
never silently rewrites the saved preparation. Native durable recovery, remaining
stock lifecycle and full candidate qualification remain unfinished.

The pure normal-departure planner now checks the current issued version, exact
material, current acceptance/hold projection and separate occurrence/recording
clocks for every line. It plans one atomic departure, preserves original paper
history and retires only pending work. The 20/8/5 branch retains eight completed,
retires five and leaves seven remaining. Native schema 11 retains immutable actual
outflow/usage and original work scope in one transaction, with one journal revision
and a physical event for each selected line. On-hand plus retained outflow conserves
each original trace independently; unlike products keep their own accounting.
Current views distinguish Departed from unchanged as-issued identification history.
Used identification cannot be replaced or marked unused. FFI 5.11 exposes bounded
actual-departure submission and read-only original recovery under current native
owner/session/credential checks. Dispatch exposes a separate actual-departure
confirmation workspace for the current issued version. It retains observations
across navigation, requires explicit review and renewed confirmation after return,
and freezes the exact operation on uncertain results. Read-only original recovery
does not submit again. Native durable recovery and full release qualification remain.
Read-only native review now reports readiness for the exact issued version and
proposed work effects from the recording planner, including its reserved-capacity
rules. It changes no business rows and grants no future action. The current owner
and credential are checked in Rust; actual occurrence and all fresh conditions are
checked again on recording. Native block reasons remain distinct from storage
corruption. Actual occurrence remains known, interval or explicitly unknown;
native recording time and operator are shown separately.

Trace includes receipts, remaining stock, recipients, adjustments, returns and
dispositions with explicit uncertainty and retrieval limits. Preserve every plausible
source/recipient in supported unresolved-origin retrieval. Bound and safely render
untrusted text in local save/print/export. Defer external integration lifecycle.

The first trace components define pure selection, counted-population conservation
and bounded escaped local HTML. Source-impact selection includes every unresolved
candidate and cannot be silently narrowed by a text filter. Identification rendering
keeps the chosen as-issued facts independent of current labels and document closure.
Trace rendering discloses local snapshot and history limits; output identity binds
the rendered content. Storage queries now retain fully departed populations,
original-backed physical/source/recipient facts and paged before/after document
history. Read continuations bind stock and catalog revisions. Native FFI5.11 and
generated Kotlin transport expose these original-backed reads and local rendering
under current-owner/session authority; schema11 is unchanged. Current trace output
requires both reviewed revision pins, and exact identification output selects its
immutable version. Rendering submits no physical operation. Shared inspection is
reachable within Work/Stock/Dispatch: it shows native per-population counts,
current acceptance/holds, retained recipient departures and paged linked history.
Broad source candidates, original-field filters and recorded-local completeness
limits remain explicit. Opening and closing the inspector preserves the underlying
draft and route. Local save/print controls now retain one native rendering across
cancellation/failure/retry. Saving uses bounded atomic UTF-8 replacement; print
results distinguish submission from uncertain output and require queue review
before an uncertain reprint. Neither adapter submits physical commands. Native UI
checks exercise real saving/cancellation/failure, unchanged stock and restart, plus
exact old-version output after reference changes; printer submission is injected.
Real host dialogs/hardware and full candidate qualification remain separate. Later lifecycle
events extend the same accounting rather than editing original intake history.

### C09: UI, drafts and native recovery

Work/Stock/Dispatch use the existing design system and labelled full-height sidebar,
right main panel and shared action workspace. Keep native chrome clearance; an
inspector becomes a full record when it does not fit. Bind existing controls/tokens
before adding components. No new theme/font/navigation framework is implied.
Test existing 1100x720 minimum and 1280x800 initial window, enlarged text, light/dark,
keyboard/focus, long/unknown values, loading, empty, stale, permission, conflict,
uncertain and output-failure states. Share forms/inspectors rather than building
forty disconnected routes. Preserve input across ordinary navigation/retry; keep
unsubmitted drafts in memory under the identity and clear on switch/sign-out/close.
Submitted-operation recovery is durable Rust state, not a UI draft or generic toast.
Preserve owned off-UI-thread opening, cancellation/late-result handling and confirmed
shutdown. Extend existing native compatibility and first/default-profile tests.

### C10: Backup, restore and release boundary

Reuse existing native backup verification with trusted capture metadata and exact
profile binding; do not trust a checksum supplied only by an untrusted manifest.
Qualify same-profile capture/restore with old state, interrupted recovery, current
authority and post-restore operation handling. Drain/quiesce runtime work and renew
session/runtime context before accepting restored-state commands. Restored history
cannot establish post-backup actions never occurred. Never automatically replay
unknown old intents. Database backups do not contain OS-keyring secrets; document
credential recovery separately. This is not bare-metal/cross-profile disaster
recovery certification. Retain demo history without a general deletion programme.
Validate the actual packaged macOS app and its bundled runtime/resources/native
library. No external distribution, signing/notarization or live operating readiness
is inferred. Preserve existing Linux checks without claiming a Linux demo release.

## Surface and acceptance coverage

View and scenario IDs below are stable design references. They describe adopted
behavior and explicit deferrals, not application test results.

### Desktop responsibilities


| View IDs | Accepted disposition |
| --- | --- |
| F01 | Adapt to existing shell, Work/Stock/Dispatch and retained utilities |
| F02 | Bounded local search/filter/disambiguation; no separate search platform |
| F03 | Existing Nostr identity/session lifecycle and bounded Help/account surface |
| W01-W02 | Quantity work and task/issue detail; no generic workflow engine |
| W03 | Defer media/AI submission; retain manual source/report annotations inline |
| W04-W05 | Discrepancy and conflicting-actual-event review |
| S01-S03 | Stock, stock record and scoped location view using shared lists/inspectors |
| S04-S13 | Receiving, movement, portion, acceptance, hold/release, count, disposition, return and linked correction |
| D01-D04 | Dispatch list/preparation, identification/handover and recorded departure |
| R01 | Defer original-media gallery/processing; preserve manual original observations |
| R02-R03 | History and trace within selected records |
| R04 | Bounded local trace/document output; no external delivery/privacy-request system |
| M01-M03 | Minimal demo setup, opening balances and site/sample-policy context |
| M04-M05 | Minimal product/pack and party editors |
| M06 | Locations and manual physical identifiers; defer scanner/device administration |
| M07 | Bounded local ownership handover in Settings; defer team/role administration |
| M08 | Sample requirement/policy versions and basis; defer general rules/legal-clock administration |
| M09 | Defer commercial imports, exchange and acknowledgements; local output is R04 |
| M10 | Defer privacy/retention request administration; backup/recovery is separate Settings |
| M11 | Defer Pro/entitlements/mobile recovery administration |

### All 25 invariants

Adopt INV-01 through INV-22 for the selected workflow under C01-C10, including
whole counts, unknown provenance, source identity, conservation, actual/planned
separation, physical portions, independent decisions, overlap, native authority,
work arithmetic, count review, time certainty, current departure checks, actual
reports, issued snapshots, recipient links, output separation, original-operation
recovery, returns/dispositions, linked corrections, uncertainty and opening stock.
INV-17's external acknowledgement mechanisms are deferred; local output truth is
mandatory. INV-23 applies to retained manual originals/confirmation; media/AI
processing is deferred. INV-24 applies to sample version/basis and truthful clocks;
legal policy/deletion systems are deferred. INV-25 requires independent manual
Core operation and identity isolation; Pro/mobile entitlement mechanics are deferred.

### All 40 acceptance scenarios

| IDs | Disposition and retained meaning |
| --- | --- |
| AC-001-004 | Adopt manual intake, units, source collisions and unknown provenance |
| AC-005 | Adapt imported expectation to manually recorded discrepancy; closure cannot change stock/work/holds |
| AC-006-014 | Adopt movement, work, counts, cutoff, portions, acceptance and hold union |
| AC-015 | Adapt role matrix to current single-owner native authorization and direct-call denials |
| AC-016 | Adopt action-specific handling of held stock under sample policy |
| AC-017 | Adopt non-reserving drafts; defer commercial import preview/version application |
| AC-018-024 | Adopt actual multi-line departure, stale checks and immutable identification lifecycle |
| AC-025 | Adopt local output failure/retry without another physical effect; defer external acknowledgements |
| AC-026-034 | Adopt original-operation recovery/replay, historical conflicts, returns/dispositions and corrections |
| AC-035 | Retain manual original observations and confirmed facts; defer upload/analysis/AI stages |
| AC-036 | Adopt uncertain-source trace, recipient accounting and explicit local-output limits |
| AC-037 | Adopt idempotent manual opening balance; defer file-import workflow |
| AC-038 | Retain sample version/basis/clock history; defer legal deadlines and broad reviewed-configuration UI |
| AC-039 | Defer privacy/retention request and optional-media deletion administration |
| AC-040 | Retain identity/history/recovery isolation with in-memory draft policy; defer Pro/shared-staff/mobile queues |

An adapted case is accepted only for its named retained subset. Never report the
unmodified design scenario passed in full. Keep remaining variants visibly deferred.
Preserve all twelve fictional fixture branches FX-01 through FX-12 independently.
Their schemas are test data, not database/API definitions. Public tests/fixtures
must run from an independent public checkout without external design documents.

### All 12 UI/platform gates

| Gate | Disposition |
| --- | --- |
| UI-01 | Full-height labelled shell and main-panel-owned navbars across selected views |
| UI-02 | List/inspector/full-record state and focus restoration |
| UI-03 | Existing token/component binding and representative rendered review |
| UI-04 | Supported sizes, density, units, unknowns and enlarged text |
| UI-05 | Scoped consequences, validation and one clear commit action |
| UI-06 | Honest loading/stale/conflict/uncertain/output-failure states |
| UI-07 | Packaged macOS keyboard/accessibility/focus; record actual platform limits |
| UI-08 | Actual file/save/print and identity success/cancel/failure; defer media and scanner hardware |
| UI-09 | Selected in-memory draft/session policy and durable submitted-operation recovery |
| UI-10 | Real packaged runtime/resources/native library on the supported Mac |
| UI-11 | Selected Work/Stock/forms/holds/handover/trace/session set; defer media/import views |
| UI-12 | Complete selected responsibilities and explicit deferred dispositions, not all-forty implementation |


## Repository bindings and implementation status

The command inventory below contains implemented and planned responsibilities.
Native demo-context initialization, reference edits, receiving, identified movement,
quantity work and discrepancy follow-up persist through the serialized Rust actor
and FFI. Explicit `make demo
DEMO_RUN=hub-preview` opens a named offline profile. The Work/Stock/Dispatch shell
uses native hub setup and owner checks. Shared receiving and handling forms consume
native Stock, requirement and discrepancy projections; later lifecycle commands
and Dispatch remain unfinished.
The table identifies extension points, not completed inventory features.

Pure hub identity, revision and ownership policy is implemented in
core/crates/harvestcircle_domain/src/hub.rs. This policy does not establish
credential possession or replace native session, persistence and FFI checks.

| Responsibility | Existing implementation / intended binding |
| --- | --- |
| Pure rules and types | core/crates/harvestcircle_domain/src, new cohesive hub module |
| Commands, ports and native session | core/crates/harvestcircle_application/src/actor.rs, ports.rs and app_core.rs |
| Persistence and transactions | core/crates/harvestcircle_storage/src/db.rs and contract.rs; add private hub adapters |
| Runtime composition | core/crates/harvestcircle_runtime/src/persistence.rs and runtime_actor.rs |
| FFI producer | core/crates/harvestcircle_ffi/src; extend native contracts before consumers |
| Kotlin transport | app/shared/src/commonMain/kotlin/org/harvestcircle/application/RuntimeContracts.kt |
| Native consumer | app/desktop/src/main/kotlin/org/harvestcircle/application/NativeHarvestCircleRuntime.kt |
| Primary navigation | app/shared/src/commonMain/kotlin/org/harvestcircle/navigation/Navigation.kt and product/SurfaceRegistry.kt |
| Shell / workspace | app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/DashboardScaffold.kt |
| Frame sizing / chrome | app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/layout/HarvestCircleAppFrame.kt |
| Controls, typography and semantics | app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem |
| Entry and window sizes | app/desktop/src/main/kotlin/org/harvestcircle/desktop/Main.kt |

The frame keeps labelled navigation at compact widths and uses a full-record
inspector fallback when it cannot fit beside the list. It retains the existing
metrics and native chrome exclusion. Existing identity tests
include IdentityBootstrapAcceptanceTest, NativeRuntimeIntegrationTest,
IdentityLifecycleHostUiTest, NativeGeneratedRecoveryTest and
DefaultProfileStartupTest. Extend actual native/SQLite tests alongside features.

## Command and query inventory

These are semantic responsibilities, not exported API names or implemented
endpoints. Bind concrete typed inputs/results as each vertical feature lands.

| Family | Required operations / reads |
| --- | --- |
| Context | Initialize/resume demo hub; current owner/session; verified local ownership handover |
| Reference data | Create/revise product variants, parties and locations; scoped search/disambiguation |
| Intake | Record receipt and explicit opening balance; stock/location/population reads |
| Handling | Record movement, identify portion, record acceptance, place/release selected hold |
| Work | Inspect required/completed/no-longer-required/remaining; replan; resolve discrepancy without stock effects |
| Departure | Prepare, issue/replace/mark-unused identification, record actual multi-line departure |
| Exceptions | Observe count, post reviewed adjustment, record return/disposition, preserve/reconcile actual report |
| Correction | Append specialized source/recipient/quantity correction and inspect affected records |
| History/output | History, trace, exact issued version, bounded local save/print/export |
| Recovery | Resolve original operation, capture/verify/restore same-profile backup |

Every consequential command needs bounded inputs, stable operation identity,
current native authorization, frozen semantic intent and appropriate versions.
Query responses carry authoritative units/basis/version/freshness; pages and
exports are bounded. Concrete limits must be named constants and tested before
an API ships. Never expose unrestricted SQL, set-balance or override-hold APIs.
Preview/consequence calculation belongs in Rust; Kotlin formats its read models.

## Verification and availability

Use the owning commands in [development](development.md). Existing commands are
make doctor, check, bindings, dev-check, ui-check, integration-check,
host-package-check, host-ui-lifecycle-check, demo, hub-check and governed linux-check.
The focused hub-check executes real Rust hub/SQLite/runtime/FFI tests, shared UI
checks and native desktop integration. Current UI acceptance executes receipt,
identified movement, replanning, explicit new handling, discrepancy resolution and
same-database restart at the minimum window size. This does not qualify the
complete MVP, durable pre-commit recovery or a packaged release. Existing checks
and generated/native compatibility remain active.

Behavioral acceptance requires real SQLite transactions and FFI/UI integration,
including failed/stale/replayed requests and lost responses. Rendered/macOS
interaction checks supplement those tests. The twelve isolated fictional fixture
branches described by the scenarios must become public executable fixtures; they
are not a production schema or a claimed passing application suite.
