import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  cp,
  link,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  realpath,
  rm,
  symlink,
  writeFile
} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { generateBuildInfo } from '../../tools/build-info.mjs';
import { auditOutput } from '../../tools/check-output.mjs';

// Slow-host verification allows1800s for each setup/copy/compiler/audit test.
// Vite children and admission checks keep1800s within their1800s group.
// Serial top-level tests keep compatibility work out of the mutation budget.
{
  const source = fileURLToPath(new URL('../../../', import.meta.url));
  // Exact HCP043 product shell bytes, immutable compatibility evidence only.
  const historicalProduct =
    '<svelte:head><title>Food — HarvestCircle</title></svelte:head>\n<div class="page page--reading stack">\n  <h1>Food</h1>\n  <p class="notice">Food details are unavailable during development.</p>\n</div>\n';
  // Exact unchanged118 producer closure for historical compatibility variants.
  // Restore before the immutable older overlays; no ancestor Git runtime dependency.
  /** @type {Record<string, {source:string, sha256:string}>} */
  const historicalPublicProducerClosure = {
    'src/lib/publishing/prepare-listing.ts': {
      source:
        "import { buildFoodTemplate } from '../contracts/food-availability-v1/write.ts';\nimport type {\n  PublicDraftRecord,\n  PublicDraftForm\n} from '../contracts/local-records.ts';\nimport { canonicalLocalId, newLocalId } from '../private-handles.ts';\nimport { readRelayPolicy, publicRelayTargets } from '../config/relays.ts';\nimport { inspectLocalTemplate } from '../nostr/local-template.ts';\nimport {\n  decodePublicRecord,\n  type PublicRecordHandle\n} from '../persistence/records.ts';\nimport {\n  publicPublicationOwner,\n  publicPublicationDraftRepository,\n  savePublicListingIntent,\n  type PublicPublicationRepository,\n  type PublicListingSave\n} from '../persistence/public-publications.ts';\nimport {\n  publicDraftControllerOwnership,\n  publicDraftControllerSnapshot,\n  flushPublicDraft,\n  type PublicDraftController\n} from './draft-controller.ts';\nimport {\n  newListingReadinessOwnership,\n  type NewListingReadiness\n} from './new-listing-readiness.ts';\n\ndeclare const reviewBrand: unique symbol;\nexport type ListingReview = Readonly<{ [reviewBrand]: true }>;\nexport type ListingPreview = Readonly<{\n  owner: string;\n  command: string;\n  draftId: string;\n  draftRevision: number;\n  identifier: string;\n  createdAtSeconds: number;\n  form: PublicDraftForm;\n  summary: string;\n  templateWire: string;\n  expectedHash: string;\n  targets: readonly string[];\n  policyFingerprint: string;\n}>;\ntype Review = Readonly<{\n  repository: PublicPublicationRepository;\n  controller: PublicDraftController;\n  expected: PublicDraftRecord;\n  editVersion: number;\n  command: string;\n  readiness: NewListingReadiness;\n  record: PublicRecordHandle;\n  previewWire: string;\n  current(this: void): boolean;\n}>;\nconst reviews = new WeakMap<ListingReview, Review>();\nconst commands = new WeakMap<\n  PublicPublicationRepository,\n  Map<string, ListingReview>\n>();\nconst activeReservations = new WeakMap<\n  ListingReview,\n  Promise<ListingReservation>\n>();\nexport type ListingReviewResult =\n  | Readonly<{ status: 'review'; review: ListingReview }>\n  | Readonly<{\n      status: 'blocked';\n      reason:\n        | 'invalid_scope'\n        | 'invalid_command'\n        | 'unavailable'\n        | 'invalid_form'\n        | 'conflict';\n    }>;\nexport type ListingReservation =\n  PublicListingSave | Readonly<{ status: 'invalid' }>;\n// Review flushes local draft state and prepares an exact immutable template;\n// it neither reserves a publication nor asks the extension to sign.\nexport async function reviewNewListing(\n  repository: PublicPublicationRepository,\n  controller: PublicDraftController,\n  readiness: NewListingReadiness,\n  command: unknown,\n  consent: unknown\n): Promise<ListingReviewResult> {\n  try {\n    const custody = publicDraftControllerOwnership(controller),\n      owner = publicPublicationOwner(repository);\n    if (\n      !custody ||\n      !owner ||\n      custody.repository !== publicPublicationDraftRepository(repository)\n    )\n      return { status: 'blocked', reason: 'invalid_scope' };\n    const id = canonicalLocalId(command);\n    if (!id) return { status: 'blocked', reason: 'invalid_command' };\n    const ready = newListingReadinessOwnership(readiness);\n    if (\n      !ready ||\n      ready.owner !== owner ||\n      !ready.current() ||\n      consent !== 'reviewed_new_listing'\n    )\n      return { status: 'blocked', reason: 'unavailable' };\n    if (!(await flushPublicDraft(controller)))\n      return { status: 'blocked', reason: 'unavailable' };\n    const snapshot = publicDraftControllerSnapshot(controller),\n      expected = snapshot?.draft;\n    if (\n      !snapshot ||\n      !expected ||\n      expected.owner !== owner ||\n      !custody.current(JSON.stringify(expected), snapshot.editVersion)\n    )\n      return { status: 'blocked', reason: 'conflict' };\n    const cached = commands.get(repository)?.get(id);\n    if (cached) {\n      const original = reviews.get(cached);\n      if (\n        original &&\n        original.controller === controller &&\n        original.readiness === readiness &&\n        JSON.stringify(original.expected) === JSON.stringify(expected) &&\n        original.current() &&\n        custody.current(JSON.stringify(expected), snapshot.editVersion)\n      ) {\n        return { status: 'review', review: cached };\n      }\n      return { status: 'blocked', reason: 'conflict' };\n    }\n    const identifier = newLocalId(),\n      createdAtSeconds = Math.floor(Date.now() / 1000);\n    if (!identifier) return { status: 'blocked', reason: 'unavailable' };\n    const form = expected.form;\n    if (form.contactType === '' && form.contactValue !== '')\n      return { status: 'blocked', reason: 'invalid_form' };\n    const contact =\n      form.contactType === ''\n        ? undefined\n        : {\n            type: form.contactType,\n            value: form.contactValue,\n            public: true as const\n          };\n    const built = buildFoodTemplate({\n      identifier,\n      title: form.title,\n      description: form.description,\n      location: form.location,\n      amount: form.amount,\n      currency: form.currency,\n      unit: form.unit,\n      ...(form.quantity === '' ? {} : { quantity: form.quantity }),\n      created_at: createdAtSeconds,\n      published_at: createdAtSeconds,\n      status: 'active',\n      ...(contact ? { contact } : {})\n    });\n    if (!built.ok) return { status: 'blocked', reason: 'invalid_form' };\n    const manifest = readRelayPolicy(ready.policy),\n      targets = publicRelayTargets(ready.policy, 'write');\n    const templateWire = JSON.stringify({\n      pubkey: owner,\n      kind: 30402,\n      created_at: createdAtSeconds,\n      tags: built.wire_parts.tags,\n      content: built.wire_parts.content\n    });\n    const template = inspectLocalTemplate(templateWire, owner, 30402);\n    if (!template || !manifest.postingEnabled || targets.length === 0)\n      return { status: 'blocked', reason: 'unavailable' };\n    const digest = await crypto.subtle.digest(\n      'SHA-256',\n      new TextEncoder().encode(JSON.stringify(manifest))\n    );\n    const policyFingerprint = Array.from(new Uint8Array(digest), (value) =>\n      value.toString(16).padStart(2, '0')\n    ).join('');\n    // Hashing is outside any IDB transaction; recheck original mutable owners.\n    if (\n      !ready.current() ||\n      publicPublicationOwner(repository) !== owner ||\n      !custody.current(JSON.stringify(expected), snapshot.editVersion)\n    )\n      return { status: 'blocked', reason: 'conflict' };\n    // Another review may have completed while this one awaited hashing. The\n    // first retained command owns its ID/time/template; never replace it.\n    const racing = commands.get(repository)?.get(id);\n    if (racing) {\n      const original = reviews.get(racing);\n      if (\n        original &&\n        original.controller === controller &&\n        original.readiness === readiness &&\n        JSON.stringify(original.expected) === JSON.stringify(expected) &&\n        original.current() &&\n        custody.current(JSON.stringify(expected), snapshot.editVersion)\n      )\n        return { status: 'review', review: racing };\n      return { status: 'blocked', reason: 'conflict' };\n    }\n    const admitted = decodePublicRecord(\n      JSON.stringify({\n        schema: 1,\n        family: 'public_operation',\n        owner,\n        id,\n        revision: 0,\n        source: { type: 'draft', id: expected.id, revision: expected.revision },\n        capture: {\n          kind: 30402,\n          wire: templateWire,\n          hash: template.hash,\n          targets,\n          policyFingerprint\n        },\n        artifact: null,\n        receipts: []\n      }),\n      owner,\n      id\n    );\n    if (!admitted.ok) return { status: 'blocked', reason: 'invalid_form' };\n    const review = Object.freeze({}) as ListingReview;\n    reviews.set(review, {\n      repository,\n      controller,\n      expected,\n      editVersion: snapshot.editVersion,\n      command: id,\n      readiness,\n      record: admitted.value,\n      previewWire: JSON.stringify({\n        owner,\n        command: id,\n        draftId: expected.id,\n        draftRevision: expected.revision,\n        identifier,\n        createdAtSeconds,\n        form,\n        summary: built.summary,\n        templateWire,\n        expectedHash: template.hash,\n        targets,\n        policyFingerprint\n      }),\n      current: () =>\n        ready.current() && publicPublicationOwner(repository) === owner\n    });\n    const cache = commands.get(repository) ?? new Map<string, ListingReview>();\n    cache.set(id, review);\n    commands.set(repository, cache);\n    return { status: 'review', review };\n  } catch {\n    return { status: 'blocked', reason: 'unavailable' };\n  }\n}\nexport function listingReviewSnapshot(\n  review: ListingReview\n): ListingPreview | undefined {\n  const value = reviews.get(review);\n  return value ? (JSON.parse(value.previewWire) as ListingPreview) : undefined;\n}\n// Internal original custody, not a signing or transport permission. The later\n// controller must reread the persisted intent and obtain its own effect lease.\nexport function listingReviewOwnership(review: ListingReview) {\n  const original = reviews.get(review);\n  const ready = original && newListingReadinessOwnership(original.readiness);\n  return original && ready && original.current()\n    ? {\n        repository: original.repository,\n        record: original.record,\n        command: original.command,\n        readiness: ready,\n        current: original.current\n      }\n    : undefined;\n}\n// One original review deduplicates simultaneous clicks. Failed/uncertain work\n// remains the same capture for later explicit retry; no fresh ID/time/template.\nexport function reserveNewListing(\n  review: ListingReview,\n  consent: unknown\n): Promise<ListingReservation> {\n  const value = reviews.get(review);\n  if (!value || consent !== 'publish_reviewed_listing')\n    return Promise.resolve({ status: 'invalid' });\n  const active = activeReservations.get(review);\n  if (active) return active;\n  const work = savePublicListingIntent(\n    value.repository,\n    value.controller,\n    value.record,\n    value.expected,\n    value.editVersion,\n    value.command,\n    value.current\n  ).finally(() => {\n    if (activeReservations.get(review) === work)\n      activeReservations.delete(review);\n  });\n  activeReservations.set(review, work);\n  return work;\n}\n",
      sha256: 'a362b18fce125b16d9f488b702cecdda1fc683f8feab25b9dca368712a40fd4e'
    },
    'src/lib/publishing/new-listing-readiness.ts': {
      source:
        "import {\n  identityMessagingOwnership,\n  type IdentitySession\n} from '../runtime/identity-session.ts';\nimport { privateSessionOwnership } from '../runtime/private-session.ts';\nimport {\n  inboxSetupViewOwnership,\n  inboxSetupViewSnapshot,\n  type InboxSetupView\n} from '../messaging/inbox-setup-view.ts';\nimport {\n  inboxResolutionSnapshot,\n  inboxResolverCurrentAfterSample\n} from '../messaging/resolve-inbox.ts';\nimport { existingInboxSetupSnapshot } from '../messaging/inbox-setup-verification.ts';\nimport type { InboxAccessObservation } from '../messaging/readiness.ts';\nimport type { RelayPolicy } from '../config/relays.ts';\n\ndeclare const readinessBrand: unique symbol;\nexport type NewListingReadiness = Readonly<{ [readinessBrand]: true }>;\ntype Ownership = Readonly<{\n  owner: string;\n  identity: IdentitySession;\n  policy: RelayPolicy;\n  current(this: void): boolean;\n  signingCurrent(this: void): boolean;\n}>;\nconst reviews = new WeakMap<\n  NewListingReadiness,\n  {\n    ownership: Ownership;\n    close(): void;\n  }\n>();\n\n// Readiness is a prerequisite for a later new-listing command, not permission\n// to sign, write a preference, reserve a listing or publish an event. Its owners\n// must still validate the exact draft/author/template and final explicit action.\nfunction sample(identity: IdentitySession, setup: InboxSetupView | undefined) {\n  try {\n    if (typeof window === 'undefined') return;\n    const actor = identityMessagingOwnership(identity);\n    const own = inboxSetupViewOwnership(setup);\n    const state = inboxSetupViewSnapshot(setup);\n    if (\n      !actor?.current() ||\n      !own?.current() ||\n      own.identity !== identity ||\n      !own.session ||\n      !own.resolver ||\n      state.busy ||\n      !state.setupComplete\n    )\n      return;\n    const privateOwner = privateSessionOwnership(own.session);\n    if (\n      !privateOwner?.current() ||\n      privateOwner.owner !== actor.owner ||\n      privateOwner.session !== actor.session\n    )\n      return;\n    const initial = inboxResolutionSnapshot(own.resolver);\n    if (!initial.knownBase || initial.author !== actor.owner) return;\n    let access: InboxAccessObservation | undefined;\n    const ready = existingInboxSetupSnapshot(\n      identity,\n      own.session,\n      own.policy,\n      own.resolver,\n      own.observeAccess\n        ? (context) => {\n            access = own.observeAccess!(context);\n            return access;\n          }\n        : undefined\n    );\n    const final = inboxResolutionSnapshot(own.resolver);\n    const accessCurrent = access?.current() ?? false;\n    const actual = inboxSetupViewOwnership(setup);\n    const finalState = inboxSetupViewSnapshot(setup);\n    if (\n      !ready.setupComplete ||\n      !ready.newListingReady ||\n      !accessCurrent ||\n      final.knownBase?.id !== initial.knownBase.id ||\n      final.status !== 'ready' ||\n      !actor.current() ||\n      !privateOwner.current() ||\n      !own.current() ||\n      !actual?.current() ||\n      actual.identity !== identity ||\n      actual.policy !== own.policy ||\n      actual.session !== own.session ||\n      actual.resolver !== own.resolver ||\n      finalState.busy ||\n      !finalState.setupComplete ||\n      !inboxResolverCurrentAfterSample(own.resolver)\n    )\n      return;\n    return {\n      actor,\n      own,\n      privateOwner,\n      preferenceId: final.knownBase.id,\n      accessCurrent: () => access?.current() ?? false\n    };\n  } catch {\n    return;\n  }\n}\n\n// Detached display information never mints or replaces the original review.\n// Draft saving has no dependency on this module and remains available here.\nexport function newListingReadinessSnapshot(\n  identity: IdentitySession,\n  setup: InboxSetupView | undefined\n) {\n  const ready = !!sample(identity, setup);\n  return {\n    draft: true as const,\n    readyToReview: ready,\n    preferenceWrite: 'not_requested' as const,\n    copy: ready\n      ? 'Messages: ready to receive replies'\n      : 'Enable messages before publishing so buyers can contact you.'\n  };\n}\nexport function captureNewListingReadiness(\n  identity: IdentitySession,\n  setup: InboxSetupView | undefined,\n  review: unknown\n): NewListingReadiness | undefined {\n  if (review !== 'reviewed_new_listing_inbox') return;\n  const captured = sample(identity, setup);\n  if (!captured) return;\n  const capturedResolver = captured.own.resolver;\n  if (!capturedResolver) return;\n  let active = true;\n  // Continuity of an already admitted original signing action. Reacquiring\n  // messaging capability during that action would reject its own busy prompt.\n  // This checks retained original generations/access, never admits a new action.\n  const signingCurrent = () => {\n    const actual = inboxSetupViewOwnership(setup);\n    const state = inboxSetupViewSnapshot(setup);\n    const resolved = inboxResolutionSnapshot(capturedResolver);\n    return (\n      active &&\n      captured.actor.current() &&\n      captured.own.current() &&\n      captured.privateOwner.current() &&\n      captured.accessCurrent() &&\n      !!actual?.current() &&\n      actual.identity === captured.own.identity &&\n      actual.policy === captured.own.policy &&\n      actual.session === captured.own.session &&\n      actual.resolver === captured.own.resolver &&\n      !state.busy &&\n      state.setupComplete &&\n      resolved.status === 'ready' &&\n      resolved.author === captured.actor.owner &&\n      resolved.knownBase?.id === captured.preferenceId &&\n      inboxResolverCurrentAfterSample(capturedResolver) &&\n      active &&\n      captured.actor.current() &&\n      captured.privateOwner.current()\n    );\n  };\n  const current = () => {\n    if (\n      !active ||\n      !captured.actor.current() ||\n      !captured.own.current() ||\n      !captured.privateOwner.current()\n    )\n      return false;\n    const actual = sample(identity, setup);\n    return (\n      !!actual &&\n      active &&\n      actual.actor.owner === captured.actor.owner &&\n      actual.actor.session === captured.actor.session &&\n      actual.own.identity === captured.own.identity &&\n      actual.own.policy === captured.own.policy &&\n      actual.own.session === captured.own.session &&\n      actual.own.resolver === captured.own.resolver &&\n      actual.preferenceId === captured.preferenceId &&\n      captured.actor.current() &&\n      captured.own.current() &&\n      captured.privateOwner.current()\n    );\n  };\n  const token = Object.freeze({}) as NewListingReadiness;\n  reviews.set(token, {\n    ownership: {\n      owner: captured.actor.owner,\n      identity,\n      policy: captured.own.policy,\n      current,\n      signingCurrent\n    },\n    close: () => {\n      active = false;\n    }\n  });\n  return token;\n}\nexport function newListingReadinessOwnership(\n  token: NewListingReadiness | undefined\n): Ownership | undefined {\n  const original = token && reviews.get(token);\n  return original?.ownership.current() ? { ...original.ownership } : undefined;\n}\nexport function closeNewListingReadiness(token: NewListingReadiness): void {\n  reviews.get(token)?.close();\n}\n",
      sha256: '0f88210b837998dbc459986e2648e0a11f3a688451a9a60f1da58d3b9d1d6e1d'
    },
    'src/lib/publishing/listing-editor.ts': {
      source:
        "import {\n  exactLocalFields,\n  type PublicDraftForm\n} from '../contracts/local-records.ts';\nimport { canonicalLocalId, newLocalId } from '../private-handles.ts';\nimport type { InboxSetupView } from '../messaging/inbox-setup-view.ts';\nimport {\n  captureNewListingReadiness,\n  closeNewListingReadiness,\n  type NewListingReadiness\n} from './new-listing-readiness.ts';\nimport {\n  reviewNewListing,\n  listingReviewSnapshot,\n  listingReviewOwnership,\n  type ListingReview,\n  type ListingReviewResult\n} from './prepare-listing.ts';\nimport { decodeProductReference } from '../nostr/references.ts';\nimport {\n  openOwnedEditSource,\n  ownedEditSourceSnapshot,\n  authorizeOwnedEditSource,\n  closeOwnedEditSource,\n  type OwnedEditSource\n} from './edit-source.ts';\nimport type { PublicRuntime } from '../runtime/public-runtime.ts';\nimport {\n  openBrowserDatabase,\n  closeBrowserDatabase,\n  type BrowserDatabase\n} from '../persistence/database.ts';\nimport {\n  createPublicPublicationRepository,\n  publicPublicationDraftRepository,\n  publicPublicationQuotaRepository,\n  readPublicListingIntent\n} from '../persistence/public-publications.ts';\nimport {\n  inspectPublicStorage,\n  publicInventorySnapshot\n} from '../persistence/quota.ts';\nimport { publicRecordSnapshot } from '../persistence/records.ts';\nimport {\n  publicDraftRepositoryOwner,\n  type PublicDraftRepository\n} from '../persistence/drafts.ts';\nimport {\n  createPublicDraftController,\n  publicDraftControllerSnapshot,\n  editPublicDraft,\n  flushPublicDraft,\n  retryPublicDraft,\n  saveAndClosePublicDraft,\n  closePublicDraftController,\n  type PublicDraftController\n} from './draft-controller.ts';\nimport {\n  identityViewSession,\n  identityViewMatchesOwner,\n  type IdentityViewContext\n} from '../runtime/view-context.ts';\nimport {\n  identitySessionSnapshot,\n  subscribeIdentityInvalidation\n} from '../runtime/identity-session.ts';\nimport {\n  readListingOperationView,\n  closeListingOperationView,\n  type ListingOperationView\n} from './listing-operation-view.ts';\n\ndeclare const editorBrand: unique symbol;\nexport type ListingEditor = Readonly<{ [editorBrand]: true }>;\nexport type ListingEditorRoute =\n  | Readonly<{ mode: 'new' }>\n  | Readonly<{ mode: 'draft'; draftId: string }>\n  | Readonly<{ mode: 'edit'; naddr: string }>;\nexport type ListingEditorSnapshot = Readonly<{\n  mode: ListingEditorRoute['mode'];\n  owner: string;\n  form: PublicDraftForm;\n  editable: boolean;\n  draftId: string | null;\n  state: 'editing' | 'saving' | 'saved' | 'failed' | 'read-only';\n  failure: string | null;\n  coordinate: string | null;\n  sourceEventId: string | null;\n  sourcePublishedAtSeconds?: number;\n  operations: readonly ListingOperationView[];\n}>;\ntype Editor = Readonly<{\n  current(): boolean;\n  retire(): void;\n  snapshot(): ListingEditorSnapshot | undefined;\n  edit(form: unknown): Promise<boolean>;\n  flush(): Promise<boolean>;\n  saveClose(): Promise<boolean>;\n  review(\n    setup: InboxSetupView | undefined,\n    form: PublicDraftForm\n  ): Promise<ListingReviewResult>;\n  reviewCurrent(review: ListingReview): boolean;\n  source(): OwnedEditSource | undefined;\n  observe(): void;\n  subscribe(listener: () => void): () => void;\n}>;\nconst editors = new WeakMap<ListingEditor, Editor>();\nconst active = new WeakMap<\n  IdentityViewContext,\n  Readonly<{ stamp: symbol; retire(): void }>\n>();\nconst emptyForm: PublicDraftForm = {\n  title: '',\n  description: '',\n  location: '',\n  amount: '',\n  currency: '',\n  unit: '',\n  quantity: '',\n  contactType: '',\n  contactValue: ''\n};\nfunction routeSnapshot(value: unknown): ListingEditorRoute | undefined {\n  try {\n    if (exactLocalFields(value, ['mode']) && value.mode === 'new')\n      return { mode: 'new' };\n    if (\n      exactLocalFields(value, ['mode', 'draftId']) &&\n      value.mode === 'draft'\n    ) {\n      const draftId = canonicalLocalId(value.draftId);\n      return draftId ? { mode: 'draft', draftId } : undefined;\n    }\n    if (exactLocalFields(value, ['mode', 'naddr']) && value.mode === 'edit') {\n      const reference = decodeProductReference(value.naddr);\n      return reference ? { mode: 'edit', naddr: reference.naddr } : undefined;\n    }\n  } catch {\n    return;\n  }\n}\n// Owner admission precedes database opening and all protected hydration. Edit\n// mode presents validated local values; independent head authorization is later.\nexport async function openListingEditor(\n  context: IdentityViewContext,\n  input: unknown,\n  signal?: AbortSignal,\n  runtime?: PublicRuntime\n): Promise<\n  | Readonly<{ status: 'ready'; editor: ListingEditor }>\n  | Readonly<{\n      status: 'unavailable';\n      reason: 'owner' | 'resource' | 'storage' | 'source';\n    }>\n> {\n  const session = identityViewSession(context),\n    route = routeSnapshot(input);\n  if (!session || signal?.aborted)\n    return { status: 'unavailable', reason: 'owner' };\n  const identity = identitySessionSnapshot(session),\n    owner = 'publicKey' in identity ? identity.publicKey : undefined;\n  if (!owner || !identityViewMatchesOwner(context, owner))\n    return { status: 'unavailable', reason: 'owner' };\n  if (\n    !route ||\n    (route.mode === 'edit' &&\n      decodeProductReference(route.naddr)?.pubkey !== owner)\n  )\n    return { status: 'unavailable', reason: 'resource' };\n  if (typeof window === 'undefined' || !navigator.locks?.request)\n    return { status: 'unavailable', reason: 'storage' };\n  active.get(context)?.retire();\n  const stamp = Symbol(),\n    listeners = new Map<symbol, () => void>(),\n    cancellation = new AbortController();\n  let alive = true,\n    closing = false,\n    unsubscribe = () => {},\n    database: BrowserDatabase | undefined,\n    repository: PublicDraftRepository | undefined,\n    controller: PublicDraftController | undefined;\n  const operations = new Map<string, ListingOperationView>();\n  let editSource: OwnedEditSource | undefined;\n  let acquiringEditDraft:\n    Promise<PublicDraftController | undefined> | undefined;\n  let reviewed:\n    | {\n        setup: InboxSetupView | undefined;\n        proof: NewListingReadiness;\n        review: ListingReview;\n      }\n    | undefined;\n  let reviewing: Promise<ListingReviewResult> | undefined;\n  let reviewingForm: string | undefined;\n  let reviewingSetup: InboxSetupView | undefined;\n  function clearReview() {\n    if (reviewed) closeNewListingReadiness(reviewed.proof);\n    reviewed = undefined;\n  }\n  function current() {\n    return (\n      alive &&\n      active.get(context)?.stamp === stamp &&\n      identityViewSession(context) === session &&\n      identityViewMatchesOwner(context, owner) &&\n      (!repository || publicDraftRepositoryOwner(repository) === owner)\n    );\n  }\n  let notifying = false;\n  function notify() {\n    if (notifying) return;\n    notifying = true;\n    try {\n      for (const [key, listener] of Array.from(listeners.entries()))\n        if (listeners.get(key) === listener) {\n          try {\n            listener();\n          } catch {\n            /*Presentation observers cannot authorize or retain ownership.*/\n          }\n        }\n    } finally {\n      notifying = false;\n    }\n  }\n  function retire() {\n    if (!alive) return;\n    alive = false;\n    unsubscribe();\n    signal?.removeEventListener('abort', retire);\n    cancellation.abort();\n    clearReview();\n    if (editSource) closeOwnedEditSource(editSource);\n    if (controller) closePublicDraftController(controller);\n    for (const operation of operations.values())\n      closeListingOperationView(operation);\n    if (database) closeBrowserDatabase(database);\n    if (active.get(context)?.stamp === stamp) active.delete(context);\n    notify();\n    listeners.clear();\n  }\n  active.set(context, { stamp, retire });\n  unsubscribe = subscribeIdentityInvalidation(session, retire);\n  signal?.addEventListener('abort', retire, { once: true });\n  if (signal?.aborted) retire();\n  function unavailable(reason: 'resource' | 'storage' | 'source') {\n    retire();\n    return { status: 'unavailable' as const, reason };\n  }\n  try {\n    const opened = await openBrowserDatabase(cancellation.signal);\n    if (opened.state !== 'ready') return unavailable('storage');\n    if (!current()) {\n      closeBrowserDatabase(opened.owner);\n      return unavailable('storage');\n    }\n    database = opened.owner;\n    const publications = createPublicPublicationRepository(database, owner);\n    repository = publications && publicPublicationDraftRepository(publications);\n    if (!publications || !repository || !current())\n      return unavailable('storage');\n    const drafts = repository;\n    let sourceForm: PublicDraftForm | undefined,\n      coordinate: string | null = null,\n      sourceEventId: string | null = null;\n    if (route.mode === 'edit') {\n      editSource = await openOwnedEditSource(\n        publications,\n        context,\n        route.naddr,\n        runtime,\n        cancellation.signal\n      );\n      if (!current()) return unavailable('source');\n      const retained = editSource && ownedEditSourceSnapshot(editSource);\n      if (!retained) return unavailable('source');\n      sourceForm = retained.form;\n      coordinate = route.naddr;\n      sourceEventId = retained.sourceEventId;\n    } else {\n      const acquired = await createPublicDraftController(\n        drafts,\n        emptyForm,\n        route.mode === 'draft' ? route.draftId : undefined\n      );\n      if (!current()) {\n        if (acquired) closePublicDraftController(acquired);\n        return unavailable('resource');\n      }\n      controller = acquired;\n      if (!controller) return unavailable('resource');\n      if (route.mode === 'draft') {\n        const quota = publicPublicationQuotaRepository(publications);\n        if (!quota) return unavailable('source');\n        const inspected = await inspectPublicStorage(quota);\n        if (!current() || !inspected.ok) return unavailable('source');\n        const inventory = publicInventorySnapshot(quota, inspected.value);\n        if (!inventory) return unavailable('source');\n        for (const item of inventory.rows) {\n          if (item.family !== 'public_operation') continue;\n          const read = await readPublicListingIntent(publications, item.id);\n          if (!current()) return unavailable('source');\n          const row =\n            read.status === 'found' &&\n            publicRecordSnapshot(read.record, owner, item.id);\n          if (!row || row.family !== 'public_operation')\n            return unavailable('source');\n          if (row.source.type !== 'draft' || row.source.id !== route.draftId)\n            continue;\n          if (operations.size >= 100) return unavailable('source');\n          const operation = await readListingOperationView(\n            publications,\n            session,\n            item.id\n          );\n          if (!current()) {\n            if (operation.status === 'found')\n              closeListingOperationView(operation.view);\n            return unavailable('source');\n          }\n          if (operation.status !== 'found') return unavailable('source');\n          operations.set(item.id, operation.view);\n        }\n      }\n    }\n    if (!current()) return unavailable('source');\n    function reviewCurrent(review: ListingReview) {\n      const preview = listingReviewSnapshot(review),\n        local = controller && publicDraftControllerSnapshot(controller);\n      return !!(\n        current() &&\n        !closing &&\n        !operations.size &&\n        route &&\n        route.mode !== 'edit' &&\n        reviewed?.review === review &&\n        listingReviewOwnership(review)?.current() &&\n        preview &&\n        local?.state === 'saved' &&\n        local.draft &&\n        preview.owner === owner &&\n        preview.draftId === local.draft.id &&\n        preview.draftRevision === local.draft.revision &&\n        JSON.stringify(preview.form) === JSON.stringify(local.form)\n      );\n    }\n    const value: Editor = {\n      current,\n      retire,\n      source: () => (current() ? editSource : undefined),\n      observe: () => {\n        if (current()) notify();\n      },\n      snapshot() {\n        if (!current()) {\n          retire();\n          return;\n        }\n        const local = controller && publicDraftControllerSnapshot(controller);\n        return {\n          mode: route.mode,\n          owner,\n          form: local ? local.form : { ...(sourceForm ?? emptyForm) },\n          editable:\n            operations.size === 0 &&\n            (route.mode !== 'edit' ||\n              !!(editSource && authorizeOwnedEditSource(editSource))),\n          draftId: local?.draft?.id ?? null,\n          state:\n            local && operations.size === 0\n              ? local.state === 'closed'\n                ? 'failed'\n                : local.state\n              : 'read-only',\n          failure: local?.failure ?? null,\n          coordinate,\n          sourceEventId,\n          ...(editSource\n            ? {\n                sourcePublishedAtSeconds:\n                  ownedEditSourceSnapshot(editSource)?.publishedAtSeconds\n              }\n            : {}),\n          operations: Array.from(operations.values())\n        };\n      },\n      async edit(input) {\n        if (!current()) {\n          retire();\n          return false;\n        }\n        if (route.mode === 'edit') {\n          if (\n            !editSource ||\n            !authorizeOwnedEditSource(editSource) ||\n            !sourceForm\n          )\n            return false;\n          if (!controller) {\n            acquiringEditDraft ??= createPublicDraftController(\n              drafts,\n              sourceForm\n            );\n            const acquired = await acquiringEditDraft;\n            if (!current() || !authorizeOwnedEditSource(editSource)) {\n              if (acquired) closePublicDraftController(acquired);\n              return false;\n            }\n            controller = acquired;\n          }\n        }\n        if (!controller || operations.size) return false;\n        if (!editPublicDraft(controller, input)) return false;\n        clearReview();\n        notify();\n        const pending = flushPublicDraft(controller);\n        await Promise.resolve();\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        const saved = await pending;\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        return saved;\n      },\n      async flush() {\n        if (!current()) {\n          retire();\n          return false;\n        }\n        if (!controller || operations.size) return false;\n        const pending = flushPublicDraft(controller);\n        await Promise.resolve();\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        const saved = await pending;\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        return saved;\n      },\n      async saveClose() {\n        if (!current() || !controller || operations.size || closing)\n          return false;\n        closing = true;\n        try {\n          if (publicDraftControllerSnapshot(controller)?.state === 'failed') {\n            const retried = await retryPublicDraft(controller);\n            if (!current()) {\n              retire();\n              return false;\n            }\n            notify();\n            if (!retried) return false;\n          }\n          const saved = await saveAndClosePublicDraft(controller);\n          if (!current()) {\n            retire();\n            return false;\n          }\n          if (!saved) {\n            notify();\n            return false;\n          }\n          retire();\n          return true;\n        } finally {\n          closing = false;\n        }\n      },\n      review(setup, input) {\n        const local = controller && publicDraftControllerSnapshot(controller);\n        if (\n          !current() ||\n          closing ||\n          !controller ||\n          !local ||\n          route.mode === 'edit' ||\n          operations.size ||\n          !exactLocalFields(input, Object.keys(emptyForm)) ||\n          JSON.stringify(input) !== JSON.stringify(local.form)\n        )\n          return Promise.resolve({\n            status: 'blocked',\n            reason: 'invalid_scope'\n          });\n        if (\n          reviewed &&\n          reviewed.setup === setup &&\n          reviewCurrent(reviewed.review)\n        )\n          return Promise.resolve({ status: 'review', review: reviewed.review });\n        const formWire = JSON.stringify(local.form);\n        if (reviewing)\n          return reviewingForm === formWire && reviewingSetup === setup\n            ? reviewing\n            : Promise.resolve({ status: 'blocked', reason: 'conflict' });\n        clearReview();\n        const proof = captureNewListingReadiness(\n            session,\n            setup,\n            'reviewed_new_listing_inbox'\n          ),\n          command = newLocalId();\n        if (!proof || !command) {\n          if (proof) closeNewListingReadiness(proof);\n          return Promise.resolve({ status: 'blocked', reason: 'unavailable' });\n        }\n        const original = controller;\n        reviewingForm = formWire;\n        reviewingSetup = setup;\n        const pending = reviewNewListing(\n          publications,\n          original,\n          proof,\n          command,\n          'reviewed_new_listing'\n        )\n          .then((result): ListingReviewResult => {\n            const actual = publicDraftControllerSnapshot(original);\n            if (\n              !current() ||\n              closing ||\n              !actual ||\n              JSON.stringify(actual.form) !== formWire ||\n              result.status !== 'review'\n            ) {\n              closeNewListingReadiness(proof);\n              return result.status === 'blocked'\n                ? result\n                : { status: 'blocked', reason: 'conflict' };\n            }\n            reviewed = { setup, proof, review: result.review };\n            if (!reviewCurrent(result.review)) {\n              clearReview();\n              return { status: 'blocked', reason: 'conflict' };\n            }\n            return result;\n          })\n          .finally(() => {\n            if (reviewing === pending) {\n              reviewing = undefined;\n              reviewingForm = undefined;\n              reviewingSetup = undefined;\n            }\n          });\n        reviewing = pending;\n        return pending;\n      },\n      reviewCurrent,\n      subscribe(listener) {\n        if (!current()) return () => {};\n        const key = Symbol();\n        listeners.set(key, listener);\n        return () => {\n          listeners.delete(key);\n        };\n      }\n    };\n    const editor = Object.freeze({}) as ListingEditor;\n    editors.set(editor, value);\n    return { status: 'ready', editor };\n  } catch {\n    return unavailable('storage');\n  }\n}\nexport function listingEditorSnapshot(\n  editor: ListingEditor\n): ListingEditorSnapshot | undefined {\n  return editors.get(editor)?.snapshot();\n}\nexport function editListingEditor(\n  editor: ListingEditor,\n  form: unknown\n): Promise<boolean> {\n  return editors.get(editor)?.edit(form) ?? Promise.resolve(false);\n}\nexport function flushListingEditor(editor: ListingEditor): Promise<boolean> {\n  return editors.get(editor)?.flush() ?? Promise.resolve(false);\n}\nexport function closeListingEditor(editor: ListingEditor): void {\n  editors.get(editor)?.retire();\n}\nexport function listingEditorEditSource(\n  editor: ListingEditor\n): OwnedEditSource | undefined {\n  return editors.get(editor)?.source();\n}\nexport function observeListingEditor(editor: ListingEditor): void {\n  editors.get(editor)?.observe();\n}\nexport function saveAndCloseListingEditor(\n  editor: ListingEditor\n): Promise<boolean> {\n  return editors.get(editor)?.saveClose() ?? Promise.resolve(false);\n}\nexport function subscribeListingEditor(\n  editor: ListingEditor,\n  listener: () => void\n): () => void {\n  return editors.get(editor)?.subscribe(listener) ?? (() => {});\n}\n// Original editor custody remains private. Review never reserves or signs.\nexport function reviewListingEditor(\n  editor: ListingEditor,\n  setup: InboxSetupView | undefined,\n  form: PublicDraftForm\n): Promise<ListingReviewResult> {\n  return (\n    editors.get(editor)?.review(setup, form) ??\n    Promise.resolve({ status: 'blocked', reason: 'invalid_scope' })\n  );\n}\nexport function listingEditorReviewCurrent(\n  editor: ListingEditor,\n  review: ListingReview\n): boolean {\n  return editors.get(editor)?.reviewCurrent(review) ?? false;\n}\n",
      sha256: 'fa2c698892b820f10adc0c816641da6a2c40b5cfc3df8db2880f61659d81b4ec'
    },
    'src/lib/publishing/edit-source.ts': {
      source:
        "import { buildFoodTemplate } from '../contracts/food-availability-v1/write.ts';\nimport {\n  projectFoodParts,\n  type FoodProjection\n} from '../contracts/food-availability-v1/read.ts';\nimport type { PublicDraftForm } from '../contracts/local-records.ts';\nimport { decodeProductReference } from '../nostr/references.ts';\nimport { inspectLocalTemplate } from '../nostr/local-template.ts';\nimport {\n  verifyEnvelope,\n  verifiedEnvelopeSnapshot\n} from '../nostr/verified-envelope.ts';\nimport {\n  publicPublicationOwner,\n  publicPublicationDraftRepository,\n  publicPublicationQuotaRepository,\n  readPublicListingIntent,\n  type PublicPublicationRepository\n} from '../persistence/public-publications.ts';\nimport {\n  inspectPublicStorage,\n  publicInventorySnapshot\n} from '../persistence/quota.ts';\nimport { publicRecordSnapshot } from '../persistence/records.ts';\nimport { readPublicDraft } from '../persistence/drafts.ts';\nimport {\n  identityViewSession,\n  identityViewMatchesOwner,\n  type IdentityViewContext\n} from '../runtime/view-context.ts';\nimport { subscribeIdentityInvalidation } from '../runtime/identity-session.ts';\nimport {\n  createPublicView,\n  beginPublicViewRun,\n  disposePublicView,\n  publicViewOwnsRun,\n  publicViewRunCurrent,\n  publicViewProjectionAvailable,\n  type PublicRuntime,\n  type PublicView\n} from '../runtime/public-runtime.ts';\nimport {\n  createPublicViewHeadResolver,\n  resolveHeads,\n  headResolutionSnapshot,\n  closeHeadResolver,\n  type HeadResolver\n} from '../catalog/resolve-head.ts';\nimport { createPublicHeadCandidate, publicHeadKey } from '../catalog/heads.ts';\n\ndeclare const sourceBrand: unique symbol;\nexport type OwnedEditSource = Readonly<{ [sourceBrand]: true }>;\nexport type OwnedEditSourceSnapshot = Readonly<{\n  owner: string;\n  coordinate: string;\n  sourceEventId: string;\n  sourceCommand: string;\n  publishedAtSeconds: number;\n  form: PublicDraftForm;\n  status:\n    'read-only' | 'checking' | 'ready' | 'partial' | 'conflict' | 'withdrawn';\n  current?: FoodProjection;\n  currentEventId?: string;\n  definitiveAbsence: false;\n}>;\ntype Source = {\n  snapshot(): OwnedEditSourceSnapshot | undefined;\n  authorize():\n    | Readonly<{\n        repository: PublicPublicationRepository;\n        owner: string;\n        coordinate: string;\n        baseWire: string;\n        sourceCommand: string;\n        publishedAtSeconds: number;\n        current(): boolean;\n      }>\n    | undefined;\n  close(): void;\n};\nconst sources = new WeakMap<OwnedEditSource, Source>();\n// Retained prototype custody precedes network work. A public/Tera event, a\n// snapshot, or an arbitrary same-author signature is not adopted as local source.\nexport async function openOwnedEditSource(\n  repository: PublicPublicationRepository,\n  context: IdentityViewContext,\n  value: unknown,\n  runtime?: PublicRuntime,\n  signal?: AbortSignal\n): Promise<OwnedEditSource | undefined> {\n  let view: PublicView | undefined, resolver: HeadResolver | undefined;\n  let closed = false,\n    off = () => {};\n  function close() {\n    if (closed) return;\n    closed = true;\n    off();\n    signal?.removeEventListener('abort', close);\n    try {\n      if (resolver) closeHeadResolver(resolver);\n    } finally {\n      if (view) disposePublicView(view);\n    }\n  }\n  try {\n    const owner = publicPublicationOwner(repository),\n      session = identityViewSession(context),\n      reference = decodeProductReference(value),\n      drafts = publicPublicationDraftRepository(repository),\n      quota = publicPublicationQuotaRepository(repository);\n    if (\n      !owner ||\n      !session ||\n      !reference ||\n      reference.pubkey !== owner ||\n      !drafts ||\n      !quota ||\n      signal?.aborted ||\n      !identityViewMatchesOwner(context, owner)\n    )\n      return;\n    const current = () =>\n      !closed &&\n      !signal?.aborted &&\n      identityViewSession(context) === session &&\n      identityViewMatchesOwner(context, owner) &&\n      publicPublicationOwner(repository) === owner;\n    off = subscribeIdentityInvalidation(session, close);\n    signal?.addEventListener('abort', close, { once: true });\n    const inspected = await inspectPublicStorage(quota);\n    if (!current() || !inspected.ok) {\n      close();\n      return;\n    }\n    const inventory = publicInventorySnapshot(quota, inspected.value);\n    if (!inventory) {\n      close();\n      return;\n    }\n    let retained:\n      | Readonly<{\n          form: PublicDraftForm;\n          wire: string;\n          command: string;\n          publishedAt: number;\n          eventId: string;\n        }>\n      | undefined;\n    for (const item of inventory.rows) {\n      if (item.family !== 'public_operation') continue;\n      const read = await readPublicListingIntent(repository, item.id);\n      if (!current()) {\n        close();\n        return;\n      }\n      const row =\n        read.status === 'found' &&\n        publicRecordSnapshot(read.record, owner, item.id);\n      // Incomplete critical storage is not an absence that permits adoption.\n      if (!row || row.family !== 'public_operation') {\n        close();\n        return;\n      }\n      if (\n        row.capture.kind !== 30402 ||\n        !row.artifact ||\n        row.source.type !== 'draft'\n      )\n        continue;\n      const template = inspectLocalTemplate(row.capture.wire, owner, 30402),\n        projected = template && projectFoodParts(template);\n      if (\n        !template ||\n        !projected ||\n        projected.outcome !== 'focused' ||\n        projected.projection.identifier !== reference.identifier\n      )\n        continue;\n      const source = await readPublicDraft(drafts, row.source.id);\n      if (!current()) {\n        close();\n        return;\n      }\n      if (!source.ok || source.value.revision !== row.source.revision) continue;\n      const form = source.value.form,\n        food = projected.projection;\n      if (form.contactType === '' && form.contactValue !== '') continue;\n      const built = buildFoodTemplate({\n        identifier: food.identifier,\n        title: form.title,\n        description: form.description,\n        location: form.location,\n        amount: form.amount,\n        currency: form.currency,\n        unit: form.unit,\n        ...(form.quantity === '' ? {} : { quantity: form.quantity }),\n        published_at: food.published_at,\n        created_at: template.created_at,\n        status: food.status,\n        ...(form.contactType === ''\n          ? {}\n          : {\n              contact: {\n                type: form.contactType,\n                value: form.contactValue,\n                public: true as const\n              }\n            })\n      });\n      if (\n        !built.ok ||\n        JSON.stringify({\n          pubkey: owner,\n          kind: 30402,\n          created_at: template.created_at,\n          tags: built.wire_parts.tags,\n          content: built.wire_parts.content\n        }) !== row.capture.wire\n      )\n        continue;\n      const verified = verifyEnvelope(row.artifact.wire),\n        signed = verified.ok && verifiedEnvelopeSnapshot(verified.value);\n      if (\n        !signed ||\n        signed.id !== row.artifact.eventId ||\n        signed.id !== row.capture.hash ||\n        JSON.stringify({\n          pubkey: signed.pubkey,\n          kind: signed.kind,\n          created_at: signed.created_at,\n          tags: signed.tags,\n          content: signed.content\n        }) !== row.capture.wire\n      )\n        continue;\n      if (retained) {\n        close();\n        return;\n      }\n      retained = {\n        form,\n        wire: row.artifact.wire,\n        command: row.id,\n        publishedAt: food.published_at,\n        eventId: signed.id\n      };\n    }\n    if (!retained || !current()) {\n      close();\n      return;\n    }\n    const base = retained,\n      proof = verifyEnvelope(base.wire);\n    const head = proof.ok && createPublicHeadCandidate(proof.value);\n    if (!head) {\n      close();\n      return;\n    }\n    const key = publicHeadKey(head);\n    let run: ReturnType<typeof beginPublicViewRun> | undefined;\n    if (runtime) {\n      // A separate view/run and exact coordinate+deletion requests are independent\n      // of keyword search. Genuine retained proof seeds the ordinary selector.\n      view = createPublicView(runtime);\n      run = beginPublicViewRun(view);\n      resolver = createPublicViewHeadResolver(view, run, {\n        nowSeconds: () => Math.floor(Date.now() / 1000)\n      });\n      resolveHeads(resolver, [head]);\n    }\n    const networkOwned = () =>\n      !!(\n        view &&\n        run &&\n        publicViewOwnsRun(view, run) &&\n        publicViewProjectionAvailable(view)\n      );\n    function snapshot(): OwnedEditSourceSnapshot | undefined {\n      if (!current()) {\n        close();\n        return;\n      }\n      const row =\n        resolver && networkOwned()\n          ? headResolutionSnapshot(resolver).find((row) => row.key === key)\n          : undefined;\n      const scopes = row && [row.headSources, row.deletionSources];\n      const complete = !!(\n        row &&\n        view &&\n        run &&\n        publicViewRunCurrent(view, run) &&\n        row.coverage === 'bounded-eose' &&\n        !row.lastKnown &&\n        scopes?.every(\n          (scope) =>\n            scope?.state === 'eose' && scope.result.coverage === 'bounded-eose'\n        )\n      );\n      const competing = row && row.state.head.id !== base.eventId;\n      const status: OwnedEditSourceSnapshot['status'] = !runtime\n        ? 'read-only'\n        : row?.deletion.outcome === 'suppressed'\n          ? 'withdrawn'\n          : competing\n            ? 'conflict'\n            : complete &&\n                row?.state.food &&\n                row.state.display === 'focused_known'\n              ? 'ready'\n              : scopes?.some((scope) => scope?.state === 'active')\n                ? 'checking'\n                : 'partial';\n      return {\n        owner: owner!,\n        coordinate: reference!.naddr,\n        sourceEventId: base.eventId,\n        sourceCommand: base.command,\n        publishedAtSeconds: base.publishedAt,\n        form: { ...base.form },\n        status,\n        ...(row?.state.food ? { current: row.state.food } : {}),\n        ...(row ? { currentEventId: row.state.head.id } : {}),\n        definitiveAbsence: false\n      };\n    }\n    const token = Object.freeze({}) as OwnedEditSource;\n    sources.set(token, {\n      snapshot,\n      close,\n      authorize() {\n        if (snapshot()?.status !== 'ready') return;\n        return {\n          repository,\n          owner,\n          coordinate: reference.naddr,\n          baseWire: base.wire,\n          sourceCommand: base.command,\n          publishedAtSeconds: base.publishedAt,\n          current: () => snapshot()?.status === 'ready'\n        };\n      }\n    });\n    return token;\n  } catch {\n    close();\n    return;\n  }\n}\nexport function ownedEditSourceSnapshot(\n  source: OwnedEditSource\n): OwnedEditSourceSnapshot | undefined {\n  return sources.get(source)?.snapshot();\n}\n// Internal original edit custody only, never a signing/delivery lease.\nexport function authorizeOwnedEditSource(source: OwnedEditSource) {\n  return sources.get(source)?.authorize();\n}\nexport function closeOwnedEditSource(source: OwnedEditSource): void {\n  sources.get(source)?.close();\n}\n",
      sha256: '985634e4c4eb2cd12839409552b1c56cd518021d4f031166a3c1ab8c4a0e7eff'
    },
    'src/lib/runtime/public-runtime.ts': {
      source:
        "import { deploymentRelayPolicy } from '../config/deployment-relays.ts';\nimport type { RelayPolicy } from '../config/relays.ts';\nimport { qualifiedNip50Sources } from '../nostr/search-sources.ts';\nimport {\n  getPublicPool,\n  subscribePublicPool,\n  closePublicPool,\n  type PublicPool\n} from '../nostr/public-pool.ts';\nimport {\n  getPublicStore,\n  insertPublicEnvelope,\n  closePublicStore,\n  publicStoreRetention,\n  publicStoreKnownEvidence,\n  publicStoreEnvelope,\n  type PublicStore\n} from '../nostr/public-store.ts';\nimport {\n  createPublicScheduler,\n  createPublicRun,\n  openPublicRequest,\n  openInboxRequest,\n  disposePublicRun,\n  closePublicScheduler,\n  publicRunSnapshot,\n  type PublicScheduler,\n  type PublicRun,\n  type PublicRequest,\n  type RequestKind,\n  type RequestClock\n} from '../nostr/request-scope.ts';\nimport type { PublicFilter } from '../nostr/exports.ts';\nimport { inboxPreferenceQueries } from '../nostr/inbox-queries.ts';\nimport {\n  publicHeadEnvelope,\n  publicHeadSnapshot,\n  type PublicHead\n} from '../catalog/heads.ts';\nimport type { KnownPublicEvidence } from '../catalog/retention.ts';\nimport type { VerifiedEnvelope } from '../nostr/verified-envelope.ts';\ndeclare const contextBrand: unique symbol;\ndeclare const runtimeBrand: unique symbol;\ndeclare const viewBrand: unique symbol;\nexport type PublicRuntimeContext = Readonly<{ readonly [contextBrand]: true }>;\nexport type PublicRuntime = Readonly<{ readonly [runtimeBrand]: true }>;\nexport type PublicView = Readonly<{ readonly [viewBrand]: true }>;\nexport const PUBLIC_RUNTIME_CONTEXT = 'harvestcircle_public_runtime';\ninterface RuntimeOwner {\n  readonly pool: PublicPool;\n  readonly store: PublicStore;\n  readonly scheduler: PublicScheduler;\n  readonly policy: RelayPolicy;\n  readonly views: Map<PublicView, () => boolean>;\n  readonly closed: () => boolean;\n  readonly projectionValid: (retry?: boolean) => boolean;\n  readonly invalidateProjection: () => void;\n  readonly close: () => void;\n}\ninterface ContextOwner {\n  readonly value: () => PublicRuntime | undefined;\n  readonly mount: (\n    policy: RelayPolicy,\n    clock?: RequestClock\n  ) => PublicRuntime | undefined;\n  readonly ready: Promise<PublicRuntime | undefined>;\n  readonly close: () => void;\n}\ninterface ViewOwner {\n  readonly runtime: RuntimeOwner;\n  readonly begin: () => PublicRun;\n  readonly current: (run: PublicRun) => boolean;\n  readonly settling: () => boolean;\n  readonly settleWith: (refresh: () => void) => void;\n  readonly dispose: () => boolean;\n}\nconst contexts = new WeakMap<PublicRuntimeContext, ContextOwner>(),\n  runtimes = new WeakMap<PublicRuntime, RuntimeOwner>(),\n  views = new WeakMap<PublicView, ViewOwner>();\nlet browserContext: PublicRuntimeContext | undefined;\nfunction contextOf(token: PublicRuntimeContext): ContextOwner {\n  const owner = contexts.get(token);\n  if (!owner) throw new Error('public_runtime_context_invalid');\n  return owner;\n}\nfunction runtimeOf(token: PublicRuntime): RuntimeOwner {\n  const owner = runtimes.get(token);\n  if (!owner) throw new Error('public_runtime_invalid');\n  if (owner.closed()) throw new Error('public_runtime_closed');\n  return owner;\n}\nfunction viewOf(token: PublicView): ViewOwner {\n  const owner = views.get(token);\n  if (!owner) throw new Error('public_view_invalid');\n  return owner;\n}\n// Context construction is SSR-pure. Root onMount acquires the browser lifetime;\n// children can await readiness even when their onMount runs before the root's.\nexport function createPublicRuntimeContext(): PublicRuntimeContext {\n  let value: PublicRuntime | undefined,\n    closed = false;\n  let ready: (value: PublicRuntime | undefined) => void = () => {};\n  let selectedClock: RequestClock | undefined;\n  const promise = new Promise<PublicRuntime | undefined>((resolve) => {\n    ready = resolve;\n  });\n  const owner: ContextOwner = {\n    value: () => (closed ? undefined : value),\n    ready: promise,\n    mount(policy, clock) {\n      if (closed) throw new Error('public_runtime_closed');\n      if (typeof window === 'undefined') return undefined;\n      if (value) {\n        const prior = runtimes.get(value);\n        if (!prior || prior.policy !== policy)\n          throw new Error('public_runtime_policy_changed');\n        if (clock !== selectedClock)\n          throw new Error('public_runtime_clock_changed');\n        return value;\n      }\n      if (browserContext && browserContext !== contextToken)\n        throw new Error('public_runtime_owner_changed');\n      const pool = getPublicPool(policy),\n        store = getPublicStore();\n      if (!pool || !store) return undefined;\n      const scheduler = createPublicScheduler(clock),\n        registered = new Map<PublicView, () => boolean>();\n      let terminal = false,\n        invalidated = false,\n        invalidating = false;\n      const token = Object.freeze({}) as PublicRuntime;\n      const runtime: RuntimeOwner = {\n        pool,\n        store,\n        scheduler,\n        policy,\n        views: registered,\n        closed: () => terminal,\n        projectionValid(retry = true) {\n          const retained = publicStoreRetention(store);\n          if (retained.stopped || (!terminal && retained.closed))\n            invalidated = true;\n          // A settler reads the terminal flag without recursively retrying its\n          // own disposal. The outer cleanup loop keeps failed controls live.\n          if (invalidated && retry) runtime.invalidateProjection();\n          return !invalidated;\n        },\n        invalidateProjection() {\n          invalidated = true;\n          if (invalidating) return;\n          invalidating = true;\n          let failed = false;\n          try {\n            for (const dispose of registered.values())\n              if (!dispose()) failed = true;\n          } finally {\n            invalidating = false;\n          }\n          if (failed) throw new Error('public_projection_close_failed');\n        },\n        close() {\n          terminal = true;\n          let failed = false;\n          for (const dispose of registered.values())\n            if (!dispose()) failed = true;\n          // Reentrant or failed settlement cannot preserve a coherent projection\n          // across store teardown. Terminal uncertainty clears every view.\n          if (failed) invalidated = true;\n          try {\n            closePublicScheduler(scheduler);\n          } catch {\n            failed = true;\n          }\n          try {\n            closePublicPool(pool);\n          } catch {\n            failed = true;\n          }\n          try {\n            closePublicStore(store);\n          } catch {\n            failed = true;\n          }\n          if (failed) throw new Error('public_runtime_close_failed');\n        }\n      };\n      runtimes.set(token, runtime);\n      value = token;\n      browserContext = contextToken;\n      selectedClock = clock;\n      ready(token);\n      return token;\n    },\n    close() {\n      closed = true;\n      ready(undefined);\n      if (value) {\n        const runtime = runtimes.get(value);\n        if (runtime) runtime.close();\n      }\n    }\n  };\n  const contextToken = Object.freeze({}) as PublicRuntimeContext;\n  contexts.set(contextToken, owner);\n  return contextToken;\n}\nexport function publicRuntime(\n  context: PublicRuntimeContext\n): PublicRuntime | undefined {\n  return contextOf(context).value();\n}\nexport function publicRuntimeReady(\n  context: PublicRuntimeContext\n): Promise<PublicRuntime | undefined> {\n  return contextOf(context).ready;\n}\nexport function mountPublicRuntime(\n  context: PublicRuntimeContext,\n  policy: RelayPolicy = deploymentRelayPolicy,\n  clock?: RequestClock\n): PublicRuntime | undefined {\n  return contextOf(context).mount(policy, clock);\n}\nexport function closePublicRuntime(context: PublicRuntimeContext): void {\n  contextOf(context).close();\n}\nexport function createPublicView(runtime: PublicRuntime): PublicView {\n  const shared = runtimeOf(runtime);\n  if (!shared.projectionValid())\n    throw new Error('public_projection_unavailable');\n  const token = Object.freeze({}) as PublicView;\n  let run: PublicRun | undefined,\n    disposed = false;\n  let generation = 0,\n    settler: (() => void) | undefined,\n    settling = false;\n  function settle() {\n    const refresh = settler;\n    settler = undefined; // Detach before any caller-controlled clock reentry.\n    if (!refresh) return;\n    settling = true;\n    try {\n      refresh?.();\n    } catch (error) {\n      if (!settler) settler = refresh;\n      throw error;\n    } finally {\n      settling = false;\n    }\n  }\n  const owner: ViewOwner = {\n    runtime: shared,\n    begin() {\n      if (disposed || shared.closed() || !shared.projectionValid())\n        throw new Error('public_view_closed');\n      const operation = ++generation,\n        previous = run;\n      // Generation reserves the transition; keep failed old controls retryable.\n      try {\n        settle();\n      } finally {\n        if (previous) disposePublicRun(previous);\n      }\n      if (\n        operation !== generation ||\n        disposed ||\n        shared.closed() ||\n        !shared.projectionValid()\n      )\n        throw new Error('public_view_run_superseded');\n      const candidate = createPublicRun(shared.scheduler, shared.policy);\n      // Run creation samples the injected clock. It may synchronously start a\n      // newer view generation before returning; never overwrite that owner.\n      if (\n        operation !== generation ||\n        disposed ||\n        shared.closed() ||\n        !shared.projectionValid()\n      ) {\n        disposePublicRun(candidate);\n        throw new Error('public_view_run_superseded');\n      }\n      run = candidate;\n      return run;\n    },\n    settling: () => settling,\n    settleWith(refresh) {\n      const previous = settler;\n      settler = refresh;\n      previous?.();\n    },\n    current(candidate) {\n      return !disposed && !shared.closed() && run === candidate;\n    },\n    dispose() {\n      disposed = true;\n      generation++;\n      if (settling) return false;\n      try {\n        settle();\n        if (run) disposePublicRun(run);\n        shared.views.delete(token);\n        return true;\n      } catch {\n        return false;\n      }\n    }\n  };\n  views.set(token, owner);\n  shared.views.set(token, () => owner.dispose());\n  return token;\n}\nexport function beginPublicViewRun(view: PublicView): PublicRun {\n  return viewOf(view).begin();\n}\nexport function publicViewRunCurrent(\n  view: PublicView,\n  run: PublicRun\n): boolean {\n  return viewOf(view).current(run) && publicRunSnapshot(run).active;\n}\n// Ownership survives an ordinary deadline so a foreground owner can publish\n// its terminal result once. Supersession/disposal/shared closure revoke it.\nexport function publicViewOwnsRun(view: PublicView, run: PublicRun): boolean {\n  return viewOf(view).current(run);\n}\nexport function disposePublicView(view: PublicView): void {\n  if (!viewOf(view).dispose()) throw new Error('public_view_close_failed');\n}\nexport function subscribePublicView(\n  view: PublicView,\n  run: PublicRun,\n  kind: RequestKind,\n  filters: readonly PublicFilter[],\n  onVerified: (event: VerifiedEnvelope) => void,\n  sampleSource?: string\n): PublicRequest {\n  const owner = viewOf(view);\n  if (!owner.current(run)) throw new Error('public_view_run_inactive');\n  // Copy the request template once before SDK effects; caller mutations cannot\n  // retarget this request. Product filter builders remain HCP038 and later.\n  const frozen = JSON.stringify(filters);\n  return openPublicRequest(\n    run,\n    kind,\n    (next) =>\n      subscribePublicPool(\n        owner.runtime.pool,\n        JSON.parse(frozen) as PublicFilter[],\n        next,\n        sampleSource\n      ),\n    (event) => {\n      if (!owner.current(run)) return;\n      const result = insertPublicEnvelope(owner.runtime.store, event);\n      if (result === 'limit' || result === 'rejected' || result === 'closed') {\n        owner.runtime.invalidateProjection();\n        return;\n      }\n      if (\n        (result === 'accepted' || result === 'duplicate') &&\n        owner.current(run)\n      )\n        onVerified(event);\n    },\n    sampleSource\n  );\n}\n\nexport function publicViewNip50Sources(view: PublicView): readonly string[] {\n  return qualifiedNip50Sources(viewOf(view).runtime.policy);\n}\n\n// Public metadata, dedicated ownership: no preference enters the food store.\n// Fixed anonymous origins, clocks, scheduler and aggregate ingress stay shared.\nexport function subscribeInboxPreference(\n  view: PublicView,\n  run: PublicRun,\n  author: unknown,\n  onVerified: (event: VerifiedEnvelope, inboxSource?: string) => void\n): PublicRequest {\n  const owner = viewOf(view),\n    filters = inboxPreferenceQueries(author);\n  if (!owner.current(run)) throw new Error('public_view_run_inactive');\n  return openInboxRequest(\n    run,\n    author,\n    (next) => subscribePublicPool(owner.runtime.pool, filters, next),\n    (event, inboxSource) => {\n      if (owner.current(run)) onVerified(event, inboxSource);\n    }\n  );\n}\n\n// Capacity invalidation differs from ordinary teardown: last-known snapshots\n// remain available on normal close, while a breached working set clears all views.\nexport function publicViewProjectionAvailable(view: PublicView): boolean {\n  const owner = viewOf(view);\n  return owner.runtime.projectionValid(!owner.settling());\n}\nexport function publicViewKnownEvidence(\n  view: PublicView,\n  head: PublicHead\n): KnownPublicEvidence {\n  return publicStoreKnownEvidence(viewOf(view).runtime.store, head);\n}\nexport function retainPublicViewHeads(\n  view: PublicView,\n  run: PublicRun,\n  heads: readonly PublicHead[]\n): boolean {\n  const owner = viewOf(view);\n  if (!owner.current(run) || !owner.runtime.projectionValid()) return false;\n  const active = publicRunSnapshot(run).active;\n  if (!owner.current(run) || !owner.runtime.projectionValid()) return false;\n  if (!active) {\n    // Inactive materialization cannot install or charge another event. Require\n    // every genuine head to have already passed the shared retained cache.\n    for (const head of heads) {\n      if (\n        !publicStoreEnvelope(owner.runtime.store, publicHeadSnapshot(head).id)\n      )\n        throw new Error('public_view_uncached_head');\n    }\n    return owner.current(run) && owner.runtime.projectionValid();\n  }\n  for (const head of heads) {\n    const result = insertPublicEnvelope(\n      owner.runtime.store,\n      publicHeadEnvelope(head)\n    );\n    if (result !== 'accepted' && result !== 'duplicate') {\n      owner.runtime.invalidateProjection();\n      return false;\n    }\n  }\n  return owner.runtime.projectionValid();\n}\n\n// Trusted model binding holds only the current projection for this view.\n// Replacement/disposal settles it while retained shared lifecycle proof is live.\nexport function settlePublicViewProjection(\n  view: PublicView,\n  refresh: () => void\n): void {\n  viewOf(view).settleWith(refresh);\n}\n",
      sha256: 'fa2e27d72b82b6bef73693cf93f19614e61ea51ebd11eb40d8ca4ec723f0c14e'
    },
    'src/lib/publishing/editor-workflow.ts': {
      source:
        "import {\n  exactLocalFields,\n  type PublicDraftForm\n} from '../contracts/local-records.ts';\nimport {\n  buildFoodTemplate,\n  type FoodDraftErrorCode\n} from '../contracts/food-availability-v1/write.ts';\nimport type { InboxSetupView } from '../messaging/inbox-setup-view.ts';\nimport {\n  reviewListingEditor,\n  listingEditorReviewCurrent,\n  type ListingEditor\n} from './listing-editor.ts';\nimport {\n  listingReviewSnapshot,\n  type ListingReview,\n  type ListingPreview\n} from './prepare-listing.ts';\n\nexport type EditorFormError = Readonly<{\n  field: keyof PublicDraftForm;\n  message: string;\n}>;\nconst fields = [\n  'title',\n  'description',\n  'location',\n  'amount',\n  'currency',\n  'unit',\n  'quantity',\n  'contactType',\n  'contactValue'\n] as const;\nexport function validateEditorForm(\n  input: PublicDraftForm\n): readonly EditorFormError[] {\n  try {\n    if (\n      !exactLocalFields(input, fields) ||\n      fields.some((field) => typeof input[field] !== 'string')\n    )\n      return [{ field: 'title', message: 'Keep all listing fields as text.' }];\n    if (input.contactType === '' && input.contactValue !== '')\n      return [\n        {\n          field: 'contactValue',\n          message: 'Choose a public contact type or remove the contact details.'\n        }\n      ];\n    const built = buildFoodTemplate({\n      identifier: '00000000-0000-4000-8000-000000000001',\n      created_at: 1,\n      published_at: 1,\n      status: 'active',\n      title: input.title,\n      description: input.description,\n      location: input.location,\n      amount: input.amount,\n      currency: input.currency,\n      unit: input.unit,\n      ...(input.quantity === '' ? {} : { quantity: input.quantity }),\n      ...(input.contactType === ''\n        ? {}\n        : {\n            contact: {\n              type: input.contactType,\n              value: input.contactValue,\n              public: true as const\n            }\n          })\n    });\n    if (built.ok) return [];\n    const messages: Partial<Record<FoodDraftErrorCode, EditorFormError>> = {\n      title_invalid: {\n        field: 'title',\n        message: 'Use a product name within the public text limit.'\n      },\n      location_invalid: {\n        field: 'location',\n        message: 'Use a public area within the text limit.'\n      },\n      description_invalid: {\n        field: 'description',\n        message: 'Add a complete description within the public text limit.'\n      },\n      summary_invalid: {\n        field: 'description',\n        message: 'Use supported public text in the description.'\n      },\n      price_invalid: {\n        field: 'amount',\n        message: 'Use a supported price, currency and price unit.'\n      },\n      quantity_invalid: {\n        field: 'quantity',\n        message: 'Use a supported advertised quantity in the selected unit.'\n      },\n      contact_invalid: {\n        field: 'contactValue',\n        message: 'Use supported public contact details for the selected type.'\n      }\n    };\n    return [\n      messages[built.error.code] ?? {\n        field: 'description',\n        message: 'The public listing exceeds its supported limits.'\n      }\n    ];\n  } catch {\n    return [\n      {\n        field: 'title',\n        message:\n          'The listing fields could not be read. Keep your current input.'\n      }\n    ];\n  }\n}\ndeclare const reviewBrand: unique symbol;\nexport type EditorReview = Readonly<{ [reviewBrand]: true }>;\ntype Owned = Readonly<{ editor: ListingEditor; original: ListingReview }>;\nconst reviews = new WeakMap<EditorReview, Owned>();\nexport type EditorReviewSnapshot = ListingPreview &\n  Readonly<{ content: string; tags: readonly (readonly string[])[] }>;\nexport async function openEditorReview(\n  editor: ListingEditor,\n  setup: InboxSetupView | undefined,\n  form: PublicDraftForm\n): Promise<\n  | Readonly<{ status: 'review'; review: EditorReview }>\n  | Readonly<{\n      status: 'blocked';\n      errors: readonly EditorFormError[];\n      message: string;\n    }>\n> {\n  const errors = validateEditorForm(form);\n  if (errors.length)\n    return {\n      status: 'blocked',\n      errors,\n      message: 'Check the linked fields. Your input remains here.'\n    };\n  const result = await reviewListingEditor(editor, setup, form);\n  if (\n    result.status !== 'review' ||\n    !listingEditorReviewCurrent(editor, result.review)\n  )\n    return {\n      status: 'blocked',\n      errors: [],\n      message:\n        result.status === 'blocked' && result.reason === 'unavailable'\n          ? 'Enable messages before reviewing this listing. Your draft remains in this browser.'\n          : 'The saved listing or account changed. Keep your input and review the current values again.'\n    };\n  const review = Object.freeze({}) as EditorReview;\n  reviews.set(review, { editor, original: result.review });\n  return { status: 'review', review };\n}\nexport function editorReviewSnapshot(\n  review: EditorReview\n): EditorReviewSnapshot | undefined {\n  const owned = reviews.get(review);\n  if (!owned || !listingEditorReviewCurrent(owned.editor, owned.original))\n    return;\n  const preview = listingReviewSnapshot(owned.original);\n  if (!preview) return;\n  const wire = JSON.parse(preview.templateWire) as {\n    content: string;\n    tags: string[][];\n  };\n  return { ...preview, content: wire.content, tags: wire.tags };\n}\nexport function closeEditorReview(review: EditorReview): void {\n  reviews.delete(review);\n}\n",
      sha256: '5bc8827c5ff167e0974f03a87e4e9db350e42f30c511f199b785c30ad365d838'
    },
    'src/lib/components/ListingEditor.svelte': {
      source:
        "<script lang=\"ts\">\n  /* global document, setInterval, clearInterval */\n  import { getContext, onMount, tick } from 'svelte';\n  import ListingReview from './ListingReview.svelte';\n  import ConflictNotice from './ConflictNotice.svelte';\n  import InboxSetup from './InboxSetup.svelte';\n  import {\n    PUBLIC_RUNTIME_CONTEXT,\n    publicRuntimeReady,\n    type PublicRuntimeContext,\n    type PublicRuntime\n  } from '../runtime/public-runtime.ts';\n  import {\n    createRuntimeInboxSetupView,\n    inboxSetupViewSnapshot,\n    subscribeInboxSetupView,\n    closeInboxSetupView,\n    type InboxSetupView\n  } from '../messaging/inbox-setup-view.ts';\n  import {\n    openEditorReview,\n    editorReviewSnapshot,\n    closeEditorReview,\n    type EditorReview,\n    type EditorFormError\n  } from '../publishing/editor-workflow.ts';\n  import CapabilityGate from './CapabilityGate.svelte';\n  import Notice from './Notice.svelte';\n  import OperationStatus from './OperationStatus.svelte';\n  import { foodUnits } from '../contracts/food-availability-v1/values.ts';\n  import type { PublicDraftForm } from '../contracts/local-records.ts';\n  import {\n    IDENTITY_VIEW_CONTEXT,\n    identityViewSnapshot,\n    subscribeIdentityView,\n    connectIdentityView,\n    disconnectIdentityView,\n    type IdentityViewContext\n  } from '../runtime/view-context.ts';\n  import {\n    openListingEditor,\n    listingEditorSnapshot,\n    editListingEditor,\n    saveAndCloseListingEditor,\n    closeListingEditor,\n    subscribeListingEditor,\n    listingEditorEditSource,\n    observeListingEditor,\n    type ListingEditor as Editor,\n    type ListingEditorRoute,\n    type ListingEditorSnapshot\n  } from '../publishing/listing-editor.ts';\n  import { internalHref } from '../navigation-url.ts';\n  let {\n    route,\n    onfirstsave,\n    onsavedclose,\n    inboxController\n  }: {\n    route: ListingEditorRoute;\n    onfirstsave?: (draftId: string, current: () => boolean) => Promise<void>;\n    onsavedclose?: () => Promise<void>;\n    inboxController?: InboxSetupView;\n  } = $props();\n  const context = getContext<IdentityViewContext>(IDENTITY_VIEW_CONTEXT);\n  const publicContext = getContext<PublicRuntimeContext>(\n    PUBLIC_RUNTIME_CONTEXT\n  );\n  let runtime = $state.raw<PublicRuntime | undefined>(),\n    setup = $state.raw<InboxSetupView | undefined>();\n  let setupState = $state(inboxSetupViewSnapshot(undefined));\n  let review = $state.raw<EditorReview | undefined>(),\n    reviewBusy = $state(false);\n  let errors = $state<readonly EditorFormError[]>([]),\n    reviewMessage = $state('');\n  let identity = $state(identityViewSnapshot(context)),\n    mounted = $state(false),\n    editor = $state.raw<Editor | undefined>(),\n    snapshot = $state<ListingEditorSnapshot>(),\n    unavailable = $state(false);\n  let closeBusy = $state(false),\n    navigationFailed = $state(false);\n  let firstNavigation: Promise<void> | undefined;\n  let firstOwner: Editor | undefined;\n  type Form = {\n    -readonly [Key in keyof PublicDraftForm]: PublicDraftForm[Key];\n  };\n  const blankForm = (): Form => ({\n    title: '',\n    description: '',\n    location: '',\n    amount: '',\n    currency: '',\n    unit: '',\n    quantity: '',\n    contactType: '',\n    contactValue: ''\n  });\n  let form = $state<Form>(blankForm());\n  const admittedAuthor = $derived(\n    identity.identity.state !== 'guest' && identity.identity.state !== 'pending'\n      ? identity.identity.publicKey\n      : undefined\n  );\n  const active = $derived(mounted && identity.mounted);\n  const visible = $derived(\n    snapshot &&\n      editor &&\n      identity.mounted &&\n      identity.identity.state !== 'guest' &&\n      identity.identity.state !== 'pending' &&\n      identity.identity.publicKey === snapshot.owner &&\n      listingEditorSnapshot(editor)\n  );\n  onMount(() => {\n    mounted = true;\n    const off = subscribeIdentityView(context, (next) => {\n      identity = next;\n    });\n    if (publicContext)\n      void publicRuntimeReady(publicContext)\n        .then((value) => {\n          if (mounted) runtime = value;\n        })\n        .catch(() => {\n          /* Draft saving does not depend on inbox setup. */\n        });\n    return () => {\n      mounted = false;\n      off();\n    };\n  });\n  $effect(() => {\n    const author = admittedAuthor,\n      available = active,\n      source = runtime,\n      supplied = inboxController;\n    if (!available || !author) {\n      setup = undefined;\n      setupState = inboxSetupViewSnapshot(undefined);\n      return;\n    }\n    const owned =\n      supplied ?? (source && createRuntimeInboxSetupView(context, source));\n    setup = owned;\n    const off = owned\n      ? subscribeInboxSetupView(owned, (next) => {\n          setupState = next;\n        })\n      : () => {};\n    return () => {\n      off();\n      if (owned) closeInboxSetupView(owned);\n    };\n  });\n  const visibleReview = $derived.by(() => {\n    if (!setupState.owner || !snapshot || !visible || !review) return;\n    return editorReviewSnapshot(review) ? review : undefined;\n  });\n  $effect(() => {\n    const resource = route,\n      author = admittedAuthor,\n      editRuntime = route.mode === 'edit' ? runtime : undefined;\n    if (!active || !author) {\n      editor = undefined;\n      snapshot = undefined;\n      form = blankForm();\n      unavailable = false;\n      return;\n    }\n    const cancellation = new AbortController();\n    let alive = true,\n      owned: Editor | undefined,\n      off = () => {};\n    const observer =\n      resource.mode === 'edit'\n        ? setInterval(() => {\n            if (owned) observeListingEditor(owned);\n          }, 250)\n        : undefined;\n    editor = undefined;\n    snapshot = undefined;\n    review = undefined;\n    errors = [];\n    reviewMessage = '';\n    form = blankForm();\n    unavailable = false;\n    void openListingEditor(\n      context,\n      resource,\n      cancellation.signal,\n      editRuntime\n    ).then((result) => {\n      if (!alive) {\n        if (result.status === 'ready') closeListingEditor(result.editor);\n        return;\n      }\n      if (result.status !== 'ready') {\n        unavailable = true;\n        return;\n      }\n      owned = result.editor;\n      editor = owned;\n      const next = listingEditorSnapshot(owned);\n      if (!next) {\n        unavailable = true;\n        return;\n      }\n      snapshot = next;\n      form = { ...next.form };\n      off = subscribeListingEditor(owned, () => {\n        snapshot = owned && listingEditorSnapshot(owned);\n        if (\n          snapshot?.state === 'saved' &&\n          snapshot.draftId &&\n          resource.mode === 'new' &&\n          owned &&\n          firstOwner !== owned &&\n          !closeBusy &&\n          onfirstsave\n        ) {\n          firstOwner = owned;\n          firstNavigation = onfirstsave(\n            snapshot.draftId,\n            () =>\n              alive &&\n              mounted &&\n              editor === owned &&\n              !!owned &&\n              !!listingEditorSnapshot(owned)\n          ).catch(() => {\n            if (alive) navigationFailed = true;\n          });\n        }\n        if (!snapshot) {\n          unavailable = true;\n          form = blankForm();\n        }\n      });\n    });\n    return () => {\n      alive = false;\n      if (observer !== undefined) clearInterval(observer);\n      cancellation.abort();\n      off();\n      if (owned) closeListingEditor(owned);\n      form = blankForm();\n    };\n  });\n  function changed() {\n    const original = editor;\n    if (!original || !listingEditorSnapshot(original)?.editable) return;\n    if (review) closeEditorReview(review);\n    review = undefined;\n    errors = [];\n    void editListingEditor(original, { ...form });\n  }\n  async function showReview() {\n    const original = editor,\n      resource = route,\n      author = admittedAuthor;\n    if (\n      reviewBusy ||\n      closeBusy ||\n      !original ||\n      !listingEditorSnapshot(original)?.editable\n    )\n      return;\n    reviewBusy = true;\n    try {\n      const result = await openEditorReview(original, setup, { ...form });\n      if (\n        !mounted ||\n        editor !== original ||\n        route !== resource ||\n        admittedAuthor !== author ||\n        !listingEditorSnapshot(original)\n      ) {\n        if (result.status === 'review') closeEditorReview(result.review);\n        return;\n      }\n      if (result.status === 'blocked') {\n        errors = result.errors;\n        reviewMessage = result.message;\n        await tick();\n        document.querySelector<HTMLElement>('#listing-review-errors')?.focus();\n        return;\n      }\n      if (review) closeEditorReview(review);\n      review = result.review;\n      errors = [];\n      reviewMessage = '';\n      await tick();\n      if (mounted && editor === original && editorReviewSnapshot(result.review))\n        document.querySelector<HTMLElement>('#listing-review-heading')?.focus();\n    } finally {\n      reviewBusy = false;\n    }\n  }\n  async function editDetails() {\n    if (review) closeEditorReview(review);\n    review = undefined;\n    errors = [];\n    reviewMessage = '';\n    await tick();\n    if (mounted && editor && listingEditorSnapshot(editor)?.editable)\n      document.querySelector<HTMLInputElement>('#listing-title')?.focus();\n  }\n  function fieldError(field: keyof PublicDraftForm) {\n    return errors.find((value) => value.field === field);\n  }\n  function fieldId(field: keyof PublicDraftForm) {\n    return (\n      'listing-' +\n      (field === 'contactType'\n        ? 'contact-type'\n        : field === 'contactValue'\n          ? 'contact-value'\n          : field)\n    );\n  }\n  function connect() {\n    return connectIdentityView(context);\n  }\n  async function saveClose() {\n    const original = editor,\n      resource = route,\n      author = admittedAuthor;\n    if (closeBusy || !original || !listingEditorSnapshot(original)?.editable)\n      return;\n    closeBusy = true;\n    try {\n      const saved = await saveAndCloseListingEditor(original);\n      await firstNavigation;\n      if (\n        !saved ||\n        !mounted ||\n        editor !== original ||\n        route !== resource ||\n        !author ||\n        admittedAuthor !== author\n      )\n        return;\n      await onsavedclose?.();\n    } catch {\n      if (mounted) navigationFailed = true;\n    } finally {\n      closeBusy = false;\n    }\n  }\n  function disconnect() {\n    disconnectIdentityView(context);\n  }\n  const title = $derived(\n    route.mode === 'new'\n      ? 'New listing'\n      : route.mode === 'draft'\n        ? 'Continue draft'\n        : 'Edit listing'\n  );\n</script>\n\n<svelte:head\n  ><title>{title} \u00b7 HarvestCircle</title><meta\n    name=\"robots\"\n    content=\"noindex\"\n  /></svelte:head\n>\n<div class=\"page page--reading stack\">\n  <a\n    href={internalHref(\n      route.mode === 'edit' ? `/products/${route.naddr}` : '/selling'\n    )}>{route.mode === 'edit' ? 'Back to listing' : 'Back to Selling'}</a\n  >\n  <h1>{title}</h1>\n  {#if !visible}\n    <CapabilityGate\n      identity={identity.identity}\n      required=\"connection\"\n      focusTarget=\"protected\"\n      onconnect={mounted ? connect : undefined}\n      ondisconnect={disconnect}\n    />\n    {#if unavailable}<Notice\n        tone=\"warning\"\n        text={route.mode === 'draft'\n          ? 'This draft is not available here.'\n          : route.mode === 'edit'\n            ? 'Supported local listing values are not available for editing. Keep this listing read-only.'\n            : 'Browser storage is unavailable. Your existing local data has not been reset.'}\n      />{:else if identity.identity.state !== 'guest' && identity.identity.state !== 'pending'}<p\n        role=\"status\"\n      >\n        Loading local values.\n      </p>{/if}\n  {:else}\n    {#if route.mode === 'edit' && editor && listingEditorEditSource(editor)}\n      <ConflictNotice\n        source={listingEditorEditSource(editor)!}\n        proposed={form}\n      />\n      {#if visible.editable}<Notice\n          tone=\"info\"\n          text=\"These changes can be saved locally. Publishing changes is not available.\"\n        />{/if}\n    {/if}\n    {#if !visible.editable}<Notice\n        tone=\"info\"\n        text={route.mode === 'draft'\n          ? 'This draft has a retained listing operation. Keep its original outcome for reconciliation.'\n          : 'These retained local values are read-only until the current listing is checked.'}\n      />{:else}<p>\n        Changes are saved in this browser. Browser storage is not a backup.\n      </p>{/if}\n    {#if visibleReview}<ListingReview\n        review={visibleReview}\n        onedit={editDetails}\n      />{:else}\n      {#if reviewMessage}<Notice tone=\"warning\" text={reviewMessage} />{/if}\n      {#if errors.length}<div\n          id=\"listing-review-errors\"\n          tabindex=\"-1\"\n          role=\"alert\"\n        >\n          <p>Check these listing fields. Your input remains here.</p>\n          <ul>\n            {#each errors as error (error.field)}<li>\n                <a href={internalHref('#' + fieldId(error.field))}\n                  >{error.message}</a\n                >\n              </li>{/each}\n          </ul>\n        </div>{/if}\n      <form\n        class=\"stack\"\n        onsubmit={(event) => event.preventDefault()}\n        novalidate\n      >\n        <div class=\"field\">\n          <label class=\"label\" for=\"listing-title\">Product name</label><input\n            id=\"listing-title\"\n            aria-invalid={fieldError('title') ? 'true' : undefined}\n            aria-errormessage={fieldError('title')\n              ? 'listing-title-error'\n              : undefined}\n            type=\"text\"\n            value={form.title}\n            oninput={(event) => {\n              form = { ...form, title: event.currentTarget.value };\n              changed();\n            }}\n            readonly={!visible.editable}\n          />\n          {#if fieldError('title')}<p id=\"listing-title-error\">\n              {fieldError('title')?.message}\n            </p>{/if}\n        </div>\n        <div class=\"field\">\n          <label class=\"label\" for=\"listing-description\">Description</label\n          ><textarea\n            id=\"listing-description\"\n            aria-invalid={fieldError('description') ? 'true' : undefined}\n            aria-errormessage={fieldError('description')\n              ? 'listing-description-error'\n              : undefined}\n            rows=\"6\"\n            value={form.description}\n            oninput={(event) => {\n              form = { ...form, description: event.currentTarget.value };\n              changed();\n            }}\n            readonly={!visible.editable}></textarea>\n          {#if fieldError('description')}<p id=\"listing-description-error\">\n              {fieldError('description')?.message}\n            </p>{/if}\n        </div>\n        <div class=\"field\">\n          <label class=\"label\" for=\"listing-location\">Public area</label>\n          <p id=\"listing-location-hint\" class=\"text-small\">\n            A town or collection area is enough.\n          </p>\n          <input\n            id=\"listing-location\"\n            aria-invalid={fieldError('location') ? 'true' : undefined}\n            aria-errormessage={fieldError('location')\n              ? 'listing-location-error'\n              : undefined}\n            type=\"text\"\n            aria-describedby=\"listing-location-hint\"\n            value={form.location}\n            oninput={(event) => {\n              form = { ...form, location: event.currentTarget.value };\n              changed();\n            }}\n            readonly={!visible.editable}\n          />\n          {#if fieldError('location')}<p id=\"listing-location-error\">\n              {fieldError('location')?.message}\n            </p>{/if}\n        </div>\n        <div class=\"cluster\">\n          <div class=\"field\">\n            <label class=\"label\" for=\"listing-amount\">Price</label><input\n              id=\"listing-amount\"\n              aria-invalid={fieldError('amount') ? 'true' : undefined}\n              aria-errormessage={fieldError('amount')\n                ? 'listing-amount-error'\n                : undefined}\n              type=\"text\"\n              inputmode=\"decimal\"\n              value={form.amount}\n              oninput={(event) => {\n                form = { ...form, amount: event.currentTarget.value };\n                changed();\n              }}\n              readonly={!visible.editable}\n            />\n            {#if fieldError('amount')}<p id=\"listing-amount-error\">\n                {fieldError('amount')?.message}\n              </p>{/if}\n          </div>\n          <div class=\"field\">\n            <label class=\"label\" for=\"listing-currency\">Currency</label><input\n              id=\"listing-currency\"\n              aria-invalid={fieldError('currency') ? 'true' : undefined}\n              aria-errormessage={fieldError('currency')\n                ? 'listing-currency-error'\n                : undefined}\n              type=\"text\"\n              maxlength=\"3\"\n              value={form.currency}\n              oninput={(event) => {\n                form = { ...form, currency: event.currentTarget.value };\n                changed();\n              }}\n              readonly={!visible.editable}\n            />\n            {#if fieldError('currency')}<p id=\"listing-currency-error\">\n                {fieldError('currency')?.message}\n              </p>{/if}\n          </div>\n          <div class=\"field\">\n            <label class=\"label\" for=\"listing-unit\">Per</label><select\n              id=\"listing-unit\"\n              aria-invalid={fieldError('unit') ? 'true' : undefined}\n              aria-errormessage={fieldError('unit')\n                ? 'listing-unit-error'\n                : undefined}\n              value={form.unit}\n              onchange={(event) => {\n                form = { ...form, unit: event.currentTarget.value };\n                changed();\n              }}\n              disabled={!visible.editable}\n              ><option value=\"\">Choose a unit</option\n              >{#each foodUnits as unit (unit)}<option value={unit}\n                  >{unit}</option\n                >{/each}</select\n            >\n            {#if fieldError('unit')}<p id=\"listing-unit-error\">\n                {fieldError('unit')?.message}\n              </p>{/if}\n          </div>\n        </div>\n        <div class=\"field\">\n          <label class=\"label\" for=\"listing-quantity\"\n            >Advertised quantity (optional)</label\n          >\n          <p id=\"listing-quantity-unit\">\n            {form.unit || 'Uses the selected price unit'}\n          </p>\n          <input\n            id=\"listing-quantity\"\n            aria-invalid={fieldError('quantity') ? 'true' : undefined}\n            aria-errormessage={fieldError('quantity')\n              ? 'listing-quantity-error'\n              : undefined}\n            type=\"text\"\n            inputmode=\"decimal\"\n            aria-describedby=\"listing-quantity-unit\"\n            value={form.quantity}\n            oninput={(event) => {\n              form = { ...form, quantity: event.currentTarget.value };\n              changed();\n            }}\n            readonly={!visible.editable}\n          />\n          {#if fieldError('quantity')}<p id=\"listing-quantity-error\">\n              {fieldError('quantity')?.message}\n            </p>{/if}\n        </div>\n        <details>\n          <summary>Add public contact details (optional)</summary>\n          <div class=\"stack\">\n            <p>Only add contact details you want to include publicly.</p>\n            <div class=\"field\">\n              <label class=\"label\" for=\"listing-contact-type\"\n                >Public contact type</label\n              ><select\n                id=\"listing-contact-type\"\n                aria-invalid={fieldError('contactType') ? 'true' : undefined}\n                aria-errormessage={fieldError('contactType')\n                  ? 'listing-contact-type-error'\n                  : undefined}\n                value={form.contactType}\n                onchange={(event) => {\n                  const value = event.currentTarget.value;\n                  if (\n                    value === '' ||\n                    value === 'email' ||\n                    value === 'phone' ||\n                    value === 'https'\n                  ) {\n                    form = { ...form, contactType: value };\n                    changed();\n                  }\n                }}\n                disabled={!visible.editable}\n                ><option value=\"\">No public contact</option><option\n                  value=\"email\">Email</option\n                ><option value=\"phone\">Phone</option><option value=\"https\"\n                  >Website</option\n                ></select\n              >\n              {#if fieldError('contactType')}<p id=\"listing-contact-type-error\">\n                  {fieldError('contactType')?.message}\n                </p>{/if}\n            </div>\n            <div class=\"field\">\n              <label class=\"label\" for=\"listing-contact-value\"\n                >Public contact details</label\n              ><input\n                id=\"listing-contact-value\"\n                aria-invalid={fieldError('contactValue') ? 'true' : undefined}\n                aria-errormessage={fieldError('contactValue')\n                  ? 'listing-contact-value-error'\n                  : undefined}\n                type=\"text\"\n                value={form.contactValue}\n                oninput={(event) => {\n                  form = { ...form, contactValue: event.currentTarget.value };\n                  changed();\n                }}\n                readonly={!visible.editable}\n              />\n              {#if fieldError('contactValue')}<p\n                  id=\"listing-contact-value-error\"\n                >\n                  {fieldError('contactValue')?.message}\n                </p>{/if}\n            </div>\n          </div>\n        </details>\n      </form>\n      {#if visible.editable && route.mode !== 'edit'}<button\n          type=\"button\"\n          onclick={showReview}\n          disabled={reviewBusy || closeBusy}\n          >{reviewBusy ? 'Preparing review' : 'Review listing'}</button\n        >{/if}\n    {/if}\n    {#if visible.editable && route.mode !== 'edit' && setup}<InboxSetup\n        controller={setup}\n        inline\n      />{/if}\n    {#each visible.operations as operation (operation)}<OperationStatus\n        publicOperation={operation}\n      />{/each}\n    {#if visible.editable}<button\n        type=\"button\"\n        onclick={saveClose}\n        disabled={closeBusy || reviewBusy}>Save and close</button\n      >{/if}\n    {#if navigationFailed}<Notice\n        tone=\"warning\"\n        text=\"Navigation did not complete. Your saved local data remains in this browser.\"\n      />{/if}\n    {#if visible.editable}<p role=\"status\">\n        {visible.state === 'saved'\n          ? 'Saved in this browser.'\n          : visible.state === 'saving'\n            ? 'Saving in this browser.'\n            : visible.state === 'failed'\n              ? 'Not saved. Keep your current input here.'\n              : 'Not yet saved.'}\n      </p>\n      {#if visible.failure}<Notice\n          tone=\"warning\"\n          text=\"Saving did not complete. Keep your current input. Existing local data has not been reset.\"\n        />{/if}{/if}\n  {/if}\n</div>\n",
      sha256: '3b1d6248c256ce02a668a2c92654816dd0c2659b7b430d5e1d3aeba6f9326bfa'
    },
    'src/lib/components/ListingReview.svelte': {
      source:
        '<script lang="ts">\n  import Notice from \'./Notice.svelte\';\n  import {\n    editorReviewSnapshot,\n    type EditorReview\n  } from \'../publishing/editor-workflow.ts\';\n  let { review, onedit }: { review: EditorReview; onedit: () => void } =\n    $props();\n  const preview = $derived(editorReviewSnapshot(review));\n  function tag(name: string) {\n    return (\n      preview?.tags\n        .find((value) => value[0] === name)\n        ?.slice(1)\n        .join(\' \') ?? \'\'\n    );\n  }\n</script>\n\n{#if preview}\n  <section class="stack" aria-label="Public listing review">\n    <h2 id="listing-review-heading" tabindex="-1">Public review</h2>\n    <p>Check the complete public listing before any signing or publication.</p>\n    <h3>{tag(\'title\')}</h3>\n    <p class="description" id="listing-review-content">{preview.content}</p>\n    <dl>\n      <dt>Derived summary</dt>\n      <dd>{preview.summary}</dd>\n      <dt>Public area</dt>\n      <dd>{tag(\'location\')}</dd>\n      <dt>Price</dt>\n      <dd>{tag(\'price\')} per {tag(\'radroots:price_unit\')}</dd>\n      {#if tag(\'radroots:quantity\')}<dt>Advertised quantity</dt>\n        <dd>{tag(\'radroots:quantity\')}</dd>{/if}\n      <dt>Publisher</dt>\n      <dd class="key">{preview.owner}</dd>\n      <dt>Public destinations</dt>\n      <dd>\n        <ul>\n          {#each preview.targets as target (target)}<li class="key">\n              {target}\n            </li>{/each}\n        </ul>\n      </dd>\n    </dl>\n    <p>Messages: ready to receive replies</p>\n    <p>This review has not requested a signature or published the listing.</p>\n    <button type="button" onclick={onedit}>Edit details</button>\n  </section>\n{:else}\n  <Notice\n    tone="warning"\n    text="This review is no longer current. Return to the current listing values."\n  />\n  <button type="button" onclick={onedit}>Edit details</button>\n{/if}\n',
      sha256: 'bf4e6bf0fc76fa12672fabc441e1058d73c1f281a0a560aeafbdaa2aa36f6f90'
    },
    'src/lib/persistence/public-publications.ts': {
      source:
        "import type { PublicDraftRecord } from '../contracts/local-records.ts';\nimport { canonicalLocalId } from '../private-handles.ts';\nimport {\n  createPublicDraftRepository,\n  publicDraftRepositoryOwner,\n  type PublicDraftRepository\n} from './drafts.ts';\nimport {\n  browserDatabaseTransaction,\n  type BrowserDatabase\n} from './database.ts';\nimport {\n  createPublicQuotaRepository,\n  publicQuotaOwner,\n  claimPublicDraftOperation,\n  type PublicQuotaRepository,\n  type PublicQuotaFailure\n} from './quota.ts';\nimport {\n  decodePublicRecord,\n  publicRecordSnapshot,\n  type PublicRecordHandle\n} from './records.ts';\nimport {\n  publicDraftControllerOwnership,\n  type PublicDraftController\n} from '../publishing/draft-controller.ts';\nimport { exactLocalFields } from '../contracts/local-records.ts';\n\ndeclare const repositoryBrand: unique symbol;\nexport type PublicPublicationRepository = Readonly<{ [repositoryBrand]: true }>;\ntype Scope = Readonly<{\n  database: BrowserDatabase;\n  drafts: PublicDraftRepository;\n  quota: PublicQuotaRepository;\n  owner: string;\n}>;\nconst repositories = new WeakMap<PublicPublicationRepository, Scope>();\nfunction scopeOf(repository: PublicPublicationRepository): Scope | undefined {\n  const scope = repositories.get(repository);\n  return scope &&\n    publicDraftRepositoryOwner(scope.drafts) === scope.owner &&\n    publicQuotaOwner(scope.quota) === scope.owner\n    ? scope\n    : undefined;\n}\nexport function createPublicPublicationRepository(\n  database: BrowserDatabase,\n  owner: unknown\n): PublicPublicationRepository | undefined {\n  const drafts = createPublicDraftRepository(database, owner),\n    quota = createPublicQuotaRepository(database, owner);\n  const admittedOwner = drafts && publicDraftRepositoryOwner(drafts);\n  if (\n    !drafts ||\n    !quota ||\n    !admittedOwner ||\n    publicQuotaOwner(quota) !== admittedOwner\n  )\n    return undefined;\n  const handle = Object.freeze({}) as PublicPublicationRepository;\n  repositories.set(handle, { database, drafts, quota, owner: admittedOwner });\n  return handle;\n}\nexport function publicPublicationDraftRepository(\n  repository: PublicPublicationRepository\n): PublicDraftRepository | undefined {\n  return scopeOf(repository)?.drafts;\n}\nexport function publicPublicationOwner(\n  repository: PublicPublicationRepository\n): string | undefined {\n  return scopeOf(repository)?.owner;\n}\n// Original same-database quota custody only; it does not mint an identity,\n// extension approval, lock lease or public network capability.\nexport function publicPublicationQuotaRepository(\n  repository: PublicPublicationRepository\n): PublicQuotaRepository | undefined {\n  return scopeOf(repository)?.quota;\n}\nexport type PublicListingSave =\n  | Readonly<{ status: 'saved' | 'existing'; record: PublicRecordHandle }>\n  | Readonly<{ status: PublicQuotaFailure | 'busy' }>;\nexport type PublicListingRead =\n  | Readonly<{ status: 'found'; record: PublicRecordHandle }>\n  | Readonly<{ status: 'missing' | 'unavailable' | 'corrupt_record' }>;\n// Readback is exact local observation, never global absence or relay receipt.\nexport function readPublicListingIntent(\n  repository: PublicPublicationRepository,\n  command: unknown\n): Promise<PublicListingRead> {\n  const scope = scopeOf(repository),\n    id = canonicalLocalId(command);\n  if (!scope || !id) return Promise.resolve({ status: 'unavailable' });\n  let transaction: IDBTransaction;\n  try {\n    transaction = browserDatabaseTransaction(\n      scope.database,\n      ['public_operations'],\n      'readonly'\n    );\n  } catch {\n    return Promise.resolve({ status: 'unavailable' });\n  }\n  return new Promise((resolve) => {\n    let decision: PublicListingRead | undefined;\n    transaction.addEventListener('complete', () =>\n      resolve(\n        scopeOf(repository)\n          ? (decision ?? { status: 'unavailable' })\n          : { status: 'unavailable' }\n      )\n    );\n    transaction.addEventListener('abort', () =>\n      resolve({ status: 'unavailable' })\n    );\n    try {\n      const request = transaction\n        .objectStore('public_operations')\n        .get([scope.owner, id]);\n      request.addEventListener('success', () => {\n        const value: unknown = request.result;\n        if (value === undefined) {\n          decision = { status: 'missing' };\n          return;\n        }\n        if (\n          !exactLocalFields(value, ['owner', 'id', 'wire']) ||\n          value.owner !== scope.owner ||\n          value.id !== id\n        ) {\n          decision = { status: 'corrupt_record' };\n          return;\n        }\n        const decoded = decodePublicRecord(value.wire, scope.owner, id);\n        const row =\n          decoded.ok && publicRecordSnapshot(decoded.value, scope.owner, id);\n        decision =\n          decoded.ok && row && row.family === 'public_operation'\n            ? { status: 'found', record: decoded.value }\n            : { status: 'corrupt_record' };\n      });\n    } catch {\n      decision = { status: 'unavailable' };\n      try {\n        transaction.abort();\n      } catch {\n        resolve({ status: 'unavailable' });\n      }\n    }\n  });\n}\n// Only a short local transaction is held. Extension/relay effects are later\n// checkpoint owners and are absent from this repository.\nexport async function savePublicListingIntent(\n  repository: PublicPublicationRepository,\n  controller: PublicDraftController,\n  record: PublicRecordHandle,\n  expected: PublicDraftRecord,\n  editVersion: number,\n  command: string,\n  current: (this: void) => boolean\n): Promise<PublicListingSave> {\n  try {\n    const scope = scopeOf(repository),\n      custody = publicDraftControllerOwnership(controller);\n    const row = scope && publicRecordSnapshot(record, scope.owner, command);\n    if (\n      !scope ||\n      !custody ||\n      custody.repository !== scope.drafts ||\n      !row ||\n      row.family !== 'public_operation' ||\n      row.source.type !== 'draft'\n    )\n      return { status: 'invalid_scope' };\n    if (\n      !current() ||\n      typeof window === 'undefined' ||\n      !navigator.locks?.request\n    )\n      return { status: 'unavailable' };\n    return await navigator.locks.request(\n      'harvestcircle:owner:' + scope.owner,\n      { mode: 'exclusive', ifAvailable: true },\n      async (lock): Promise<PublicListingSave> => {\n        if (!lock) return { status: 'busy' };\n        const fresh = (): boolean => scopeOf(repository) === scope && current();\n        if (!fresh()) return { status: 'unavailable' };\n        const claimed = await claimPublicDraftOperation(\n          scope.quota,\n          row.id,\n          record,\n          {\n            expected,\n            current: fresh,\n            draftCurrent: () =>\n              custody.current(JSON.stringify(expected), editVersion)\n          }\n        );\n        if (!fresh()) return { status: 'unavailable' };\n        if (claimed.ok)\n          return {\n            status: claimed.value.state === 'created' ? 'saved' : 'existing',\n            record: claimed.value.record\n          };\n        if (claimed.reason === 'unknown_completion') {\n          const observed = await readPublicListingIntent(repository, row.id);\n          const original =\n            observed.status === 'found' &&\n            publicRecordSnapshot(observed.record, scope.owner, row.id);\n          if (!fresh()) return { status: 'unavailable' };\n          if (\n            observed.status === 'found' &&\n            original &&\n            original.family === 'public_operation' &&\n            JSON.stringify(original.source) === JSON.stringify(row.source) &&\n            JSON.stringify(original.capture) === JSON.stringify(row.capture)\n          )\n            return { status: 'existing', record: observed.record };\n          if (original) return { status: 'conflict' };\n        }\n        return { status: claimed.reason };\n      }\n    );\n  } catch {\n    return { status: 'unavailable' };\n  }\n}\n",
      sha256: '5a995002cbae1a62ecce9b34770baad102c5c90340da922593e3cc8d9f5c3384'
    },
    'src/lib/persistence/quota.ts': {
      source:
        "import { canonicalPublicKey } from '../contracts/public-key.ts';\nimport {\n  exactLocalFields,\n  type PublicRecord,\n  type PublicDraftRecord\n} from '../contracts/local-records.ts';\nimport { canonicalLocalId } from '../private-handles.ts';\nimport { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';\nimport {\n  browserDatabaseState,\n  browserDatabaseTransaction,\n  type BrowserDatabase\n} from './database.ts';\nimport {\n  decodePublicRecord,\n  publicRecordSnapshot,\n  publicRecordWire,\n  type PublicRecordHandle\n} from './records.ts';\nimport {\n  publicInventoryRow,\n  PUBLIC_CLEANUP_CONSEQUENCES,\n  type PublicInventoryRow\n} from './local-inventory.ts';\n\nimport {\n  publicTransitionIdentity,\n  publicTransitionSnapshot,\n  type PublicOperationTransition\n} from './artifact-records.ts';\nimport { decideFrozenTransition } from './operation-transactions.ts';\n\ndeclare const repositoryBrand: unique symbol;\ndeclare const inventoryBrand: unique symbol;\ndeclare const cleanupBrand: unique symbol;\nexport type PublicQuotaRepository = Readonly<{ [repositoryBrand]: true }>;\nexport type PublicInventory = Readonly<{ [inventoryBrand]: true }>;\nexport type PublicCleanup = Readonly<{ [cleanupBrand]: true }>;\nexport type PublicQuotaFailure =\n  | 'invalid_scope'\n  | 'invalid_record'\n  | 'invalid_selection'\n  | 'protected'\n  | 'conflict'\n  | 'capacity'\n  | 'corrupt_record'\n  | 'unavailable'\n  | 'aborted'\n  | 'unknown_completion';\nexport type PublicQuotaResult<T> =\n  | Readonly<{ ok: true; value: T }>\n  | Readonly<{ ok: false; reason: PublicQuotaFailure }>;\ntype Scope = Readonly<{ database: BrowserDatabase; owner: string }>;\ntype Store = 'public_drafts' | 'public_operations' | 'preference_operations';\ntype Row = Readonly<{\n  store: Store;\n  wire: string;\n  record: PublicRecord;\n  bytes: number;\n}>;\ntype Inspection = Readonly<{\n  repository: PublicQuotaRepository;\n  rows: readonly Row[];\n}>;\ntype Cleanup = Readonly<{ inventory: Inspection; selected: readonly string[] }>;\nconst repositories = new WeakMap<PublicQuotaRepository, Scope>();\nconst inventories = new WeakMap<PublicInventory, Inspection>();\nconst cleanups = new WeakMap<PublicCleanup, Cleanup>();\nconst stores: readonly Store[] = [\n  'public_drafts',\n  'public_operations',\n  'preference_operations'\n];\nfunction failed(reason: PublicQuotaFailure): PublicQuotaResult<never> {\n  return { ok: false, reason };\n}\nfunction scopeOf(repository: PublicQuotaRepository): Scope | undefined {\n  const scope = repositories.get(repository);\n  try {\n    return scope && browserDatabaseState(scope.database).state === 'ready'\n      ? scope\n      : undefined;\n  } catch {\n    return undefined;\n  }\n}\n// Metadata only; this does not grant installed-account or effect authority.\nexport function publicQuotaOwner(\n  repository: PublicQuotaRepository\n): string | undefined {\n  return scopeOf(repository)?.owner;\n}\n// A namespace capability only; installed-account authorization belongs to the\n// account workflow. This repository never acquires any private store.\nexport function createPublicQuotaRepository(\n  database: BrowserDatabase,\n  expectedOwner: unknown\n): PublicQuotaRepository | undefined {\n  const owner = canonicalPublicKey(expectedOwner);\n  try {\n    if (!owner || browserDatabaseState(database).state !== 'ready')\n      return undefined;\n    const handle = Object.freeze({}) as PublicQuotaRepository;\n    repositories.set(handle, { database, owner });\n    return handle;\n  } catch {\n    return undefined;\n  }\n}\nfunction stored(\n  value: unknown,\n  store: Store,\n  owner: string,\n  id: string\n): Row | undefined {\n  if (\n    !exactLocalFields(value, ['owner', 'id', 'wire']) ||\n    value.owner !== owner ||\n    value.id !== id ||\n    typeof value.wire !== 'string'\n  )\n    return undefined;\n  const admitted = decodePublicRecord(value.wire, owner, id);\n  if (!admitted.ok) return undefined;\n  const record = publicRecordSnapshot(admitted.value, owner, id);\n  const family =\n    store === 'public_drafts'\n      ? 'public_draft'\n      : store === 'public_operations'\n        ? 'public_operation'\n        : 'preference_operation';\n  return record?.family === family\n    ? {\n        store,\n        record,\n        wire: value.wire,\n        bytes: new TextEncoder().encode(value.wire).length\n      }\n    : undefined;\n}\nfunction transaction<T>(\n  scope: Scope,\n  mode: 'readonly' | 'readwrite',\n  finish: (\n    rows: readonly Row[],\n    transaction: IDBTransaction,\n    refuse: (reason: PublicQuotaFailure) => void\n  ) => PublicQuotaResult<T> | undefined\n): Promise<PublicQuotaResult<T>> {\n  let active: IDBTransaction;\n  try {\n    active = browserDatabaseTransaction(scope.database, stores, mode);\n  } catch {\n    return Promise.resolve(failed('unavailable'));\n  }\n  return new Promise((resolve) => {\n    let result: PublicQuotaResult<T> | undefined;\n    let rows = Array.from<Row>([]),\n      draftCount = 0,\n      operationCount = 0,\n      operationBytes = 0;\n    function refuse(reason: PublicQuotaFailure) {\n      result = failed(reason);\n      try {\n        active.abort();\n      } catch {\n        result = failed('unknown_completion');\n      }\n    }\n    active.addEventListener('complete', () =>\n      resolve(result ?? failed('unknown_completion'))\n    );\n    active.addEventListener('abort', () =>\n      resolve(result && !result.ok ? result : failed('aborted'))\n    );\n    function scan(index: number): void {\n      try {\n        const store = stores[index];\n        if (store === undefined) {\n          const finished = finish(rows, active, refuse);\n          if (finished !== undefined) result = finished;\n          return;\n        }\n        const request = active\n          .objectStore(store)\n          .index('by_owner')\n          .openCursor(IDBKeyRange.only(scope.owner));\n        request.addEventListener('success', () => {\n          try {\n            const cursor = request.result;\n            if (!cursor) {\n              scan(index + 1);\n              return;\n            }\n            const key = cursor.primaryKey;\n            const id =\n              Array.isArray(key) && key.length === 2 && key[0] === scope.owner\n                ? canonicalLocalId(key[1])\n                : undefined;\n            const row = id && stored(cursor.value, store, scope.owner, id);\n            if (!row) {\n              refuse('corrupt_record');\n              return;\n            }\n            if (row.record.family === 'public_draft') draftCount++;\n            else {\n              operationCount++;\n              operationBytes += row.bytes;\n            }\n            if (\n              draftCount > LOCAL_PERSISTENCE_BUDGETS.publicDrafts ||\n              operationCount > LOCAL_PERSISTENCE_BUDGETS.publicOperations ||\n              operationBytes > LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes\n            ) {\n              refuse('capacity');\n              return;\n            }\n            rows = rows.concat(row);\n            cursor.continue();\n          } catch {\n            refuse('aborted');\n          }\n        });\n      } catch {\n        refuse('aborted');\n      }\n    }\n    scan(0);\n  });\n}\n// Explicit new-row admission only. Artifact/CAS transitions retain HCP050.\n// Logical bytes include the exact stored record wire, not browser disk/heap use.\nexport function admitPublicOperation(\n  repository: PublicQuotaRepository,\n  expectedId: unknown,\n  handle: PublicRecordHandle\n): Promise<PublicQuotaResult<Readonly<{ id: string; revision: number }>>> {\n  const scope = scopeOf(repository),\n    id = canonicalLocalId(expectedId);\n  if (!scope) return Promise.resolve(failed('invalid_scope'));\n  if (!id) return Promise.resolve(failed('invalid_record'));\n  const record = publicRecordSnapshot(handle, scope.owner, id),\n    wire = publicRecordWire(handle, scope.owner, id);\n  if (\n    !record ||\n    !wire ||\n    record.family === 'public_draft' ||\n    record.revision !== 0\n  )\n    return Promise.resolve(failed('invalid_record'));\n  const store: Store =\n    record.family === 'public_operation'\n      ? 'public_operations'\n      : 'preference_operations';\n  const bytes = new TextEncoder().encode(wire).length;\n  return transaction(scope, 'readwrite', (rows, active, refuse) => {\n    const previous = rows.find(\n      (row) => row.store === store && row.record.id === id\n    );\n    if (previous)\n      return previous.wire === wire\n        ? { ok: true, value: { id, revision: 0 } }\n        : (refuse('conflict'), undefined);\n    const operations = rows.filter(\n      (row) => row.record.family !== 'public_draft'\n    );\n    if (\n      operations.length >= LOCAL_PERSISTENCE_BUDGETS.publicOperations ||\n      operations.reduce((sum, row) => sum + row.bytes, bytes) >\n        LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes\n    ) {\n      refuse('capacity');\n      return;\n    }\n    active.objectStore(store).put({ owner: scope.owner, id, wire });\n    return { ok: true, value: { id, revision: 0 } };\n  });\n}\n// Claim under the caller's origin/author Web Lock. A previous capture is never\n// overwritten or automatically signed again, even after a page lost its lock.\n// No extension/network await occurs inside the short transaction.\nexport function claimPublicOperation(\n  repository: PublicQuotaRepository,\n  expectedId: unknown,\n  handle: PublicRecordHandle\n): Promise<\n  PublicQuotaResult<\n    Readonly<{ state: 'created' | 'existing'; record: PublicRecordHandle }>\n  >\n> {\n  return claimOperation(repository, expectedId, handle);\n}\ntype DraftAdmission = Readonly<{\n  expected: PublicDraftRecord;\n  current(this: void): boolean;\n  draftCurrent(this: void): boolean;\n}>;\n// Storage-only atomic draft/CAS reservation. Current original account/review\n// ownership is supplied by the public listing command owner, never a UI flag.\n// Existing commands retain their original capture even after future draft edits.\nexport function claimPublicDraftOperation(\n  repository: PublicQuotaRepository,\n  expectedId: unknown,\n  handle: PublicRecordHandle,\n  admission: DraftAdmission\n): Promise<\n  PublicQuotaResult<\n    Readonly<{ state: 'created' | 'existing'; record: PublicRecordHandle }>\n  >\n> {\n  return claimOperation(repository, expectedId, handle, admission);\n}\nfunction claimOperation(\n  repository: PublicQuotaRepository,\n  expectedId: unknown,\n  handle: PublicRecordHandle,\n  draft?: DraftAdmission\n): Promise<\n  PublicQuotaResult<\n    Readonly<{ state: 'created' | 'existing'; record: PublicRecordHandle }>\n  >\n> {\n  const scope = scopeOf(repository),\n    id = canonicalLocalId(expectedId);\n  if (!scope) return Promise.resolve(failed('invalid_scope'));\n  if (!id) return Promise.resolve(failed('invalid_record'));\n  const record = publicRecordSnapshot(handle, scope.owner, id),\n    wire = publicRecordWire(handle, scope.owner, id);\n  if (\n    !record ||\n    !wire ||\n    record.family === 'public_draft' ||\n    record.revision !== 0 ||\n    record.artifact !== null\n  )\n    return Promise.resolve(failed('invalid_record'));\n  const store: Store =\n    record.family === 'public_operation'\n      ? 'public_operations'\n      : 'preference_operations';\n  const bytes = new TextEncoder().encode(wire).length;\n  return transaction<\n    Readonly<{ state: 'created' | 'existing'; record: PublicRecordHandle }>\n  >(scope, 'readwrite', (rows, active, refuse) => {\n    if (draft && !draft.current()) {\n      refuse('unavailable');\n      return;\n    }\n    const previous = rows.find(\n      (row) => row.record.family !== 'public_draft' && row.record.id === id\n    );\n    if (previous) {\n      const original = previous.record;\n      if (\n        original.family === 'public_draft' ||\n        original.family !== record.family ||\n        JSON.stringify(original.source) !== JSON.stringify(record.source) ||\n        JSON.stringify(original.capture) !== JSON.stringify(record.capture) ||\n        ('consent' in original ? original.consent : null) !==\n          ('consent' in record ? record.consent : null)\n      ) {\n        refuse('conflict');\n        return;\n      }\n      const decoded = decodePublicRecord(previous.wire, scope.owner, id);\n      if (!decoded.ok) {\n        refuse('corrupt_record');\n        return;\n      }\n      return { ok: true, value: { state: 'existing', record: decoded.value } };\n    }\n    if (draft) {\n      const source =\n        record.family === 'public_operation' ? record.source : undefined;\n      const previousDraft = rows.find(\n        (row) =>\n          row.record.family === 'public_draft' &&\n          row.record.id === draft.expected.id\n      );\n      if (\n        !source ||\n        source.type !== 'draft' ||\n        draft.expected.owner !== scope.owner ||\n        source.id !== draft.expected.id ||\n        source.revision !== draft.expected.revision ||\n        !previousDraft ||\n        JSON.stringify(previousDraft.record) !==\n          JSON.stringify(draft.expected) ||\n        !draft.draftCurrent()\n      ) {\n        refuse('conflict');\n        return;\n      }\n    }\n    const operations = rows.filter(\n      (row) => row.record.family !== 'public_draft'\n    );\n    if (\n      operations.length >= LOCAL_PERSISTENCE_BUDGETS.publicOperations ||\n      operations.reduce((sum, row) => sum + row.bytes, bytes) >\n        LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes\n    ) {\n      refuse('capacity');\n      return;\n    }\n    if (draft && (!draft.current() || !draft.draftCurrent())) {\n      refuse('conflict');\n      return;\n    }\n    active.objectStore(store).put({ owner: scope.owner, id, wire });\n    return { ok: true, value: { state: 'created', record: handle } };\n  });\n}\nexport function inspectPublicStorage(\n  repository: PublicQuotaRepository\n): Promise<PublicQuotaResult<PublicInventory>> {\n  const scope = scopeOf(repository);\n  if (!scope) return Promise.resolve(failed('invalid_scope'));\n  return transaction(scope, 'readonly', (rows) => {\n    const handle = Object.freeze({}) as PublicInventory;\n    inventories.set(handle, { repository, rows });\n    return { ok: true, value: handle };\n  });\n}\nexport function publicInventorySnapshot(\n  repository: PublicQuotaRepository,\n  handle: PublicInventory\n):\n  | Readonly<{ rows: readonly PublicInventoryRow[]; consequences: string }>\n  | undefined {\n  const inventory = inventories.get(handle);\n  return scopeOf(repository) && inventory?.repository === repository\n    ? {\n        rows: inventory.rows.map((row) =>\n          publicInventoryRow(row.record, row.bytes)\n        ),\n        consequences: PUBLIC_CLEANUP_CONSEQUENCES\n      }\n    : undefined;\n}\nexport function reviewPublicCleanup(\n  repository: PublicQuotaRepository,\n  handle: PublicInventory,\n  selection: unknown\n): PublicQuotaResult<PublicCleanup> {\n  const inventory = inventories.get(handle);\n  if (!scopeOf(repository) || inventory?.repository !== repository)\n    return failed('invalid_scope');\n  try {\n    if (!Array.isArray(selection)) return failed('invalid_selection');\n    const length = selection.length;\n    if (\n      !Number.isSafeInteger(length) ||\n      length <= 0 ||\n      length >\n        LOCAL_PERSISTENCE_BUDGETS.publicDrafts +\n          LOCAL_PERSISTENCE_BUDGETS.publicOperations\n    )\n      return failed('invalid_selection');\n    let selected = Array.from<string>([]);\n    for (let index = 0; index < length; index++) {\n      const key: unknown = selection[index];\n      if (typeof key !== 'string' || selected.includes(key))\n        return failed('invalid_selection');\n      const row = inventory.rows.find(\n        (row) => publicInventoryRow(row.record, row.bytes).key === key\n      );\n      if (!row) return failed('invalid_selection');\n      if (publicInventoryRow(row.record, row.bytes).state !== 'settled')\n        return failed('protected');\n      selected = selected.concat(key);\n    }\n    if (!scopeOf(repository)) return failed('invalid_scope');\n    const token = Object.freeze({}) as PublicCleanup;\n    cleanups.set(token, { inventory, selected });\n    return { ok: true, value: token };\n  } catch {\n    return failed('invalid_selection');\n  }\n}\nexport function publicCleanupSnapshot(\n  repository: PublicQuotaRepository,\n  handle: PublicCleanup\n):\n  | Readonly<{\n      selected: readonly PublicInventoryRow[];\n      logicalBytesRemoved: number;\n      consequences: string;\n    }>\n  | undefined {\n  const cleanup = cleanups.get(handle);\n  if (!scopeOf(repository) || cleanup?.inventory.repository !== repository)\n    return undefined;\n  const selected = cleanup.inventory.rows\n    .map((row) => publicInventoryRow(row.record, row.bytes))\n    .filter((row) => cleanup.selected.includes(row.key));\n  return {\n    selected,\n    logicalBytesRemoved: selected.reduce(\n      (sum, row) => sum + row.logicalBytes,\n      0\n    ),\n    consequences: PUBLIC_CLEANUP_CONSEQUENCES\n  };\n}\nexport function commitPublicCleanup(\n  repository: PublicQuotaRepository,\n  handle: PublicCleanup\n): Promise<PublicQuotaResult<readonly string[]>> {\n  const cleanup = cleanups.get(handle),\n    scope = scopeOf(repository);\n  if (!scope || cleanup?.inventory.repository !== repository)\n    return Promise.resolve(failed('invalid_scope'));\n  return transaction(scope, 'readwrite', (rows, active, refuse) => {\n    const before = cleanup.inventory.rows;\n    if (\n      rows.length !== before.length ||\n      rows.some(\n        (row, i) =>\n          row.store !== before[i].store ||\n          row.record.id !== before[i].record.id ||\n          row.wire !== before[i].wire\n      )\n    ) {\n      refuse('conflict');\n      return;\n    }\n    const selected = rows.filter((row) =>\n      cleanup.selected.includes(publicInventoryRow(row.record, row.bytes).key)\n    );\n    if (\n      selected.length !== cleanup.selected.length ||\n      selected.some(\n        (row) => publicInventoryRow(row.record, row.bytes).state !== 'settled'\n      )\n    ) {\n      refuse('protected');\n      return;\n    }\n    for (const row of selected)\n      active.objectStore(row.store).delete([scope.owner, row.record.id]);\n    return { ok: true, value: Array.from(cleanup.selected) };\n  });\n}\n\n// No signer/network await exists inside this transaction. It consumes only a\n// genuine immutable typed transition captured before storage acquisition.\n// Exact owner-scoped actual stored preference, not a caller's structural row.\n// Read is bounded by the same complete quota/codec scan and transaction ack.\nexport function loadPreferenceOperation(\n  repository: PublicQuotaRepository,\n  expectedId: unknown\n): Promise<PublicQuotaResult<PublicRecordHandle>> {\n  const scope = scopeOf(repository),\n    id = canonicalLocalId(expectedId);\n  if (!scope) return Promise.resolve(failed('invalid_scope'));\n  if (!id) return Promise.resolve(failed('invalid_record'));\n  return transaction(scope, 'readonly', (rows) => {\n    const row = rows.find(\n      (row) => row.store === 'preference_operations' && row.record.id === id\n    );\n    if (!row) return failed('invalid_record');\n    const decoded = decodePublicRecord(row.wire, scope.owner, id);\n    return decoded.ok ? decoded : failed('corrupt_record');\n  });\n}\nexport function commitPublicOperationTransition(\n  repository: PublicQuotaRepository,\n  handle: PublicOperationTransition\n): Promise<PublicQuotaResult<Readonly<{ id: string; revision: number }>>> {\n  const scope = scopeOf(repository);\n  if (!scope) return Promise.resolve(failed('invalid_scope'));\n  const identity = publicTransitionIdentity(handle);\n  const command =\n    identity && publicTransitionSnapshot(handle, scope.owner, identity.id);\n  if (!command) return Promise.resolve(failed('invalid_record'));\n  const store: Store =\n    command.family === 'public_operation'\n      ? 'public_operations'\n      : 'preference_operations';\n  return transaction(scope, 'readwrite', (rows, active, refuse) => {\n    const previous = rows.find(\n      (row) => row.store === store && row.record.id === command.id\n    );\n    const decision = decideFrozenTransition(\n      previous?.wire,\n      command.baseWire,\n      command.nextWire\n    );\n    if (decision === 'conflict') {\n      refuse('conflict');\n      return;\n    }\n    if (decision === 'committed')\n      return {\n        ok: true,\n        value: { id: command.id, revision: command.revision }\n      };\n    const operations = rows.filter(\n      (row) => row.record.family !== 'public_draft'\n    );\n    const bytes =\n      operations.reduce((sum, row) => sum + row.bytes, 0) -\n      (previous?.bytes ?? 0) +\n      new TextEncoder().encode(command.nextWire).length;\n    if (bytes > LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes) {\n      refuse('capacity');\n      return;\n    }\n    active\n      .objectStore(store)\n      .put({ owner: scope.owner, id: command.id, wire: command.nextWire });\n    return { ok: true, value: { id: command.id, revision: command.revision } };\n  });\n}\nexport async function observePublicOperationTransition(\n  repository: PublicQuotaRepository,\n  handle: PublicOperationTransition\n): Promise<\n  | Readonly<{ state: 'committed'; id: string; revision: number }>\n  | Readonly<{ state: 'base_observed' | 'conflict' | 'unavailable' }>\n> {\n  const scope = scopeOf(repository),\n    identity = publicTransitionIdentity(handle);\n  const command =\n    scope &&\n    identity &&\n    publicTransitionSnapshot(handle, scope.owner, identity.id);\n  if (!scope || !command) return { state: 'unavailable' };\n  const store: Store =\n    command.family === 'public_operation'\n      ? 'public_operations'\n      : 'preference_operations';\n  const read = await transaction(scope, 'readonly', (rows) => ({\n    ok: true,\n    value: decideFrozenTransition(\n      rows.find((row) => row.store === store && row.record.id === command.id)\n        ?.wire,\n      command.baseWire,\n      command.nextWire\n    )\n  }));\n  if (!read.ok || !scopeOf(repository)) return { state: 'unavailable' };\n  return read.value === 'committed'\n    ? { state: 'committed', id: command.id, revision: command.revision }\n    : { state: read.value };\n}\n",
      sha256: '7bff93f40c55d5f0899deb50309d4d69107fb299d1aee10c348fda23e71761bf'
    },
    'src/lib/nostr/listing-publisher.ts': {
      source:
        "import type { NostrEvent } from 'applesauce-core/helpers';\nimport {\n  publicRelayTargets,\n  readRelayPolicy,\n  type RelayPolicy\n} from '../config/relays.ts';\nimport {\n  listingPublicationOwnership,\n  type ListingPublication\n} from '../publishing/sign-listing.ts';\nimport {\n  listingResumeOwnership,\n  type ListingResumption\n} from '../publishing/resume-listing.ts';\nimport {\n  listingRetryOwnership,\n  type ListingRetry\n} from '../publishing/retry-listing.ts';\nimport {\n  identityMessagingOwnership,\n  identityPublicOperationOwnership,\n  recheckIdentityOwner,\n  type IdentitySession\n} from '../runtime/identity-session.ts';\nimport { preferencePolicyFingerprint } from './inbox-preference-publication.ts';\nimport { readPublicListingIntent } from '../persistence/public-publications.ts';\nimport {\n  decodePublicRecord,\n  publicRecordSnapshot,\n  type PublicRecordHandle\n} from '../persistence/records.ts';\ndeclare const leaseBrand: unique symbol;\ndeclare const permissionBrand: unique symbol;\nexport type ListingDeliveryLease = Readonly<{ [leaseBrand]: true }>;\nexport type ListingAttemptPermission = Readonly<{ [permissionBrand]: true }>;\ntype Original = NonNullable<\n  | ReturnType<typeof listingPublicationOwnership>\n  | ReturnType<typeof listingResumeOwnership>\n>;\ntype Owner = Readonly<{\n  original: Original;\n  policy: RelayPolicy;\n  signal?: AbortSignal;\n  current(): boolean;\n}>;\ntype Permission = Readonly<{\n  owner: Owner;\n  origin: string;\n  mode: 'publish' | 'readback';\n  wire: string;\n}>;\nconst leases = new WeakMap<ListingDeliveryLease, Owner>();\nconst permissions = new WeakMap<ListingAttemptPermission, Permission>();\n// Stop releases this action's wait and author lock. The extension promise may\n// settle later; it cannot resurrect the revoked network lease or a signer.\nasync function recheckDeliveryOwner(\n  identity: IdentitySession,\n  signal?: AbortSignal\n): Promise<boolean> {\n  if (signal?.aborted) return false;\n  if (!signal) {\n    await recheckIdentityOwner(identity);\n    return true;\n  }\n  let stop = () => {};\n  const stopped = new Promise<boolean>((resolve) => {\n    stop = () => resolve(false);\n    signal.addEventListener('abort', stop, { once: true });\n    if (signal.aborted) stop();\n  });\n  try {\n    return await Promise.race([\n      recheckIdentityOwner(identity).then(() => true),\n      stopped\n    ]);\n  } finally {\n    signal.removeEventListener('abort', stop);\n  }\n}\n// Receipt revisions may grow, but the complete original operation payload and\n// verified artifact stay exact. No new command, author or destination is adopted.\nexport async function readRetainedListingDelivery(\n  lease: ListingDeliveryLease\n): Promise<PublicRecordHandle | undefined> {\n  const owner = leases.get(lease);\n  if (!owner) return;\n  const original = owner.original;\n  const expected = decodePublicRecord(\n    original.recordWire,\n    original.owner,\n    original.command\n  );\n  if (!expected.ok) return;\n  const baseline = publicRecordSnapshot(\n    expected.value,\n    original.owner,\n    original.command\n  );\n  if (\n    baseline?.family !== 'public_operation' ||\n    baseline.capture.kind !== 30402 ||\n    !baseline.artifact ||\n    baseline.revision !== 2\n  )\n    return;\n  const read = await readPublicListingIntent(\n    original.repository,\n    original.command\n  );\n  if (read.status !== 'found') return;\n  const row = publicRecordSnapshot(\n    read.record,\n    original.owner,\n    original.command\n  );\n  if (\n    row?.family !== 'public_operation' ||\n    row.revision < baseline.revision ||\n    JSON.stringify({\n      ...row,\n      revision: baseline.revision,\n      receipts: baseline.receipts\n    }) !== JSON.stringify(baseline)\n  )\n    return;\n  return read.record;\n}\n// Current authority is mandatory for every new network permission. Retained\n// exact storage observation above remains usable for bookkeeping after Stop.\nexport async function readOwnedListingDelivery(\n  lease: ListingDeliveryLease\n): Promise<PublicRecordHandle | undefined> {\n  const owner = leases.get(lease);\n  if (!owner?.current()) return;\n  const read = readRetainedListingDelivery(lease);\n  let stop = () => {};\n  const stopped =\n    owner.signal &&\n    new Promise<undefined>((resolve) => {\n      stop = () => resolve(undefined);\n      owner.signal!.addEventListener('abort', stop, { once: true });\n      if (owner.signal!.aborted) stop();\n    });\n  let record: PublicRecordHandle | undefined;\n  try {\n    record = await (stopped ? Promise.race([read, stopped]) : read);\n  } finally {\n    owner.signal?.removeEventListener('abort', stop);\n  }\n  return owner.current() ? record : undefined;\n}\nexport function listingDeliveryOwnership(lease: ListingDeliveryLease) {\n  const owner = leases.get(lease);\n  return owner?.current()\n    ? {\n        repository: owner.original.repository,\n        owner: owner.original.owner,\n        command: owner.original.command,\n        current: owner.current\n      }\n    : undefined;\n}\nexport type ListingOwnershipResult<T> =\n  | Readonly<{ status: 'completed'; value: T }>\n  | Readonly<{ status: 'invalid' | 'unavailable' | 'busy' | 'stopped' }>;\n// One original author lock spans this finite explicitly requested network\n// action. It never spans a native IDB transaction and cannot mint a signer.\nexport async function withListingDeliveryOwnership<T>(\n  publication: ListingPublication | ListingResumption,\n  policy: RelayPolicy,\n  consent: unknown,\n  work: (lease: ListingDeliveryLease) => Promise<T>,\n  control?: ListingRetry\n): Promise<ListingOwnershipResult<T>> {\n  const signed = listingPublicationOwnership(publication as ListingPublication),\n    resumed = listingResumeOwnership(publication as ListingResumption),\n    original = signed ?? resumed,\n    retry = control && listingRetryOwnership(control);\n  if (\n    !original ||\n    (control\n      ? !retry ||\n        retry.source !== publication ||\n        retry.policy !== policy ||\n        retry.consent !== consent\n      : !signed || consent !== 'publish_stored_listing') ||\n    original.policy !== policy\n  )\n    return { status: 'invalid' };\n  if (retry && !retry.current()) return { status: 'stopped' };\n  if (typeof window === 'undefined' || !navigator.locks?.request)\n    return { status: 'unavailable' };\n  try {\n    return await navigator.locks.request(\n      'harvestcircle:owner:' + original.owner,\n      { mode: 'exclusive', ifAvailable: true },\n      async (lock): Promise<ListingOwnershipResult<T>> => {\n        if (!lock) return { status: 'busy' };\n        if (!(await recheckDeliveryOwner(original.identity, retry?.signal)))\n          return { status: 'stopped' };\n        const identity = identityMessagingOwnership(original.identity),\n          operation =\n            signed &&\n            identityPublicOperationOwnership(\n              original.identity,\n              signed.operation\n            );\n        if (\n          !identity ||\n          (signed && !operation) ||\n          identity.owner !== original.owner ||\n          (operation &&\n            (operation.owner !== original.owner ||\n              operation.session !== identity.session)) ||\n          !original.current() ||\n          (retry && !retry.current())\n        )\n          return { status: 'stopped' };\n        let active = true;\n        const owner: Owner = {\n          original,\n          policy,\n          signal: retry?.signal,\n          current: () =>\n            active &&\n            original.current() &&\n            identity.current() &&\n            (!operation || operation.current()) &&\n            (!retry || retry.current())\n        };\n        const lease = Object.freeze({}) as ListingDeliveryLease;\n        leases.set(lease, owner);\n        try {\n          if (!(await readOwnedListingDelivery(lease)))\n            return {\n              status: retry && !retry.current() ? 'stopped' : 'unavailable'\n            };\n          return { status: 'completed', value: await work(lease) };\n        } finally {\n          active = false;\n        }\n      }\n    );\n  } catch {\n    return { status: 'unavailable' };\n  }\n}\n// Each permission comes from actual stored signed bytes, fresh identity and\n// unchanged original policy. Preference/private capabilities cannot enter it.\nexport async function authorizeListingAttempt(\n  lease: ListingDeliveryLease,\n  origin: string,\n  mode: 'publish' | 'readback'\n): Promise<ListingAttemptPermission | undefined> {\n  const owner = leases.get(lease);\n  if (!owner || !owner.current() || (mode !== 'publish' && mode !== 'readback'))\n    return;\n  try {\n    const record = await readOwnedListingDelivery(lease),\n      original = owner.original;\n    const row =\n      record && publicRecordSnapshot(record, original.owner, original.command);\n    const manifest = readRelayPolicy(owner.policy),\n      targets = publicRelayTargets(owner.policy, 'write'),\n      fingerprint = await preferencePolicyFingerprint(owner.policy);\n    if (\n      row?.family !== 'public_operation' ||\n      !row.artifact ||\n      !manifest.postingEnabled ||\n      !manifest.messagingEnabled ||\n      fingerprint !== row.capture.policyFingerprint ||\n      JSON.stringify(targets) !== JSON.stringify(row.capture.targets) ||\n      !row.capture.targets.includes(origin)\n    )\n      return;\n    if (!(await recheckDeliveryOwner(original.identity, owner.signal))) return;\n    if (!owner.current()) return;\n    const token = Object.freeze({}) as ListingAttemptPermission;\n    permissions.set(token, { owner, origin, mode, wire: row.artifact.wire });\n    return token;\n  } catch {\n    return;\n  }\n}\n// Original once-only token: detached metadata, wrong role, policy or origin\n// cannot supply a raw event. SDK effects remain owned by the public pool.\nexport function takeListingPublication(\n  token: ListingAttemptPermission,\n  policy: RelayPolicy,\n  origin: string,\n  mode: 'publish' | 'readback'\n):\n  | Readonly<{ event: NostrEvent; wire: string; current(): boolean }>\n  | undefined {\n  const saved = permissions.get(token);\n  if (!saved) return;\n  permissions.delete(token);\n  if (\n    saved.owner.policy !== policy ||\n    saved.origin !== origin ||\n    saved.mode !== mode ||\n    !saved.owner.current()\n  )\n    return;\n  return {\n    event: JSON.parse(saved.wire) as NostrEvent,\n    wire: saved.wire,\n    current: saved.owner.current\n  };\n}\n",
      sha256: '1c8c3445513329d57fcddc75da63991f0c1c27c28caf87db381d32bba739c872'
    }
  };
  /** @param {string} web */
  async function restoreHistoricalPublicProducers(web) {
    for (const [name, value] of Object.entries(
      historicalPublicProducerClosure
    )) {
      assert.equal(
        createHash('sha256').update(value.source).digest('hex'),
        value.sha256
      );
      await writeFile(path.join(web, name), value.source);
    }
  }
  // Frozen public-only HCP055 producers, source6023db01169aa82cb6fe68ca94e1a73750247e72.
  // Compatibility evidence only; runtime tests never require ancestor Git history.
  /** @type {Record<string, {source:string, sha256:string}>} */
  const historicalPublicOnly = {
    'src/routes/+layout.svelte': {
      source:
        "<script lang=\"ts\">\n  import '../theme.css';\n  import '../app.css';\n  import { onMount, setContext, type Snippet } from 'svelte';\n  import {\n    createPublicRuntimeContext,\n    mountPublicRuntime,\n    closePublicRuntime,\n    PUBLIC_RUNTIME_CONTEXT\n  } from '../lib/runtime/public-runtime.ts';\n  import { page } from '$app/state';\n  import AppShell from '../lib/components/AppShell.svelte';\n\n  const publicContext = createPublicRuntimeContext();\n  setContext(PUBLIC_RUNTIME_CONTEXT, publicContext);\n  onMount(() => {\n    mountPublicRuntime(publicContext);\n    return () => closePublicRuntime(publicContext);\n  });\n\n  let { children }: { children: Snippet } = $props();\n</script>\n\n<AppShell\n  currentPath={page.url.pathname}\n  availableRoutes={[\n    '/search',\n    '/sell',\n    '/selling',\n    '/messages',\n    '/about',\n    '/privacy'\n  ]}\n>\n  {@render children()}\n</AppShell>\n",
      sha256: '956da247f23ad9695c3c6f8bff42801aceaa5b84a2606a1a8a5263fd0c637dc7'
    },
    'src/lib/components/AppShell.svelte': {
      source:
        "<script lang=\"ts\">\n  import type { Snippet } from 'svelte';\n  import { internalHref } from '../navigation-url';\n  import Button from './primitives/Button.svelte';\n  import Disclosure from './primitives/Disclosure.svelte';\n\n  type ShellRoute =\n    '/search' | '/sell' | '/selling' | '/messages' | '/about' | '/privacy';\n  type Identity =\n    | { kind: 'guest'; onconnect?: () => void }\n    | { kind: 'connected'; publicKey: string; ondisconnect?: () => void };\n  let {\n    children,\n    currentPath = '/',\n    availableRoutes = [],\n    identity = { kind: 'guest' }\n  }: {\n    children: Snippet;\n    currentPath?: string;\n    availableRoutes?: readonly ShellRoute[];\n    identity?: Identity;\n  } = $props();\n\n  const unavailable = $derived(\n    !availableRoutes.includes('/search') ||\n      !availableRoutes.includes('/about') ||\n      !availableRoutes.includes('/privacy') ||\n      (identity.kind === 'guest'\n        ? !availableRoutes.includes('/sell') || !identity.onconnect\n        : !availableRoutes.includes('/selling') ||\n          !availableRoutes.includes('/messages') ||\n          !identity.ondisconnect)\n  );\n\n  function available(href: string) {\n    return (\n      href === '/' ||\n      availableRoutes.some((route) => route === href.split('#')[0])\n    );\n  }\n</script>\n\n{#snippet navigation(label: string, href: string, brand = false)}\n  {#if available(href) && internalHref(href)}\n    <a\n      href={internalHref(href)}\n      class=\"shell-link\"\n      class:brand\n      aria-current={currentPath === href ? 'page' : undefined}>{label}</a\n    >\n  {:else}\n    <span\n      class=\"shell-link text-muted\"\n      aria-disabled=\"true\"\n      aria-describedby=\"shell-availability\"\n      aria-current={currentPath === href ? 'page' : undefined}>{label}</span\n    >\n  {/if}\n{/snippet}\n\n<a href={internalHref('#main-content')} class=\"skip-link visually-hidden\"\n  >Skip to main content</a\n>\n<header class=\"navbar\">\n  <nav aria-label=\"Primary\" class=\"page navbar__inner\">\n    <div class=\"cluster\">\n      {@render navigation('HarvestCircle', '/', true)}\n      {@render navigation('Search', '/search')}\n      {#if identity.kind === 'guest'}\n        {@render navigation('List food', '/sell')}\n        <Button\n          label=\"Connect extension\"\n          disabled={!identity.onconnect}\n          onclick={identity.onconnect}\n        />\n      {:else}\n        {@render navigation('Messages', '/messages')}\n        {@render navigation('Selling', '/selling')}\n        <Disclosure summary=\"Identity\">\n          <p class=\"key\">{identity.publicKey}</p>\n          <Button\n            label=\"Disconnect\"\n            disabled={!identity.ondisconnect}\n            onclick={identity.ondisconnect}\n          />\n        </Disclosure>\n      {/if}\n    </div>\n    {#if unavailable}\n      <p id=\"shell-availability\" class=\"text-small text-muted\">\n        Unavailable during development: disabled navigation and actions.\n      </p>\n    {/if}\n  </nav>\n</header>\n<main id=\"main-content\" tabindex=\"-1\" class=\"page\">\n  {@render children()}\n</main>\n<footer class=\"footer\">\n  <nav aria-label=\"Footer\" class=\"page cluster\">\n    {@render navigation('About', '/about')}\n    {@render navigation('Privacy', '/privacy')}\n    {@render navigation('Help / report', '/about#help')}\n  </nav>\n</footer>\n",
      sha256: 'c8afbb1ee62a16ad47558829518b347fe3e2a312e490312ded812a8b538d828f'
    },
    'src/lib/components/AccountGate.svelte': {
      source:
        '<script lang="ts">\n  import Button from \'./primitives/Button.svelte\';\n</script>\n\n<svelte:head>\n  <title>HarvestCircle</title>\n  <meta name="robots" content="noindex" />\n</svelte:head>\n<div class="page page--reading stack">\n  <h1>Connect or unlock</h1>\n  <p>Connect an extension and unlock your account to continue.</p>\n  <p class="notice">Account access is unavailable during development.</p>\n  <div class="cluster">\n    <Button label="Connect extension" disabled />\n    <Button label="Unlock" disabled />\n  </div>\n</div>\n',
      sha256: 'd5f90fb4b9af50037ed19dde626552e82df67017a39408bdbdeff1e712c10fbd'
    },
    // Exact predecessor AccountGate route bytes, historical compatibility only.
    'src/routes/sell/+page.svelte': {
      source:
        '<script lang="ts">\n  import AccountGate from \'../../lib/components/AccountGate.svelte\';\n</script>\n\n<AccountGate />\n',
      sha256: '23547dcfa10e85865e31b66c31ea0d043592e3c316c94da2d74bdf2dd419b38a'
    },
    'src/routes/selling/drafts/[draftId=local_id]/+page.svelte': {
      source:
        '<script lang="ts">\n  import AccountGate from \'../../../../lib/components/AccountGate.svelte\';\n</script>\n\n<AccountGate />\n',
      sha256: '465852393264789ceae4133e8d79416ebf37d5485b08fb54c61056fa3714dc5d'
    },
    'src/routes/products/[naddr=naddr]/edit/+page.svelte': {
      source:
        '<script lang="ts">\n  import AccountGate from \'../../../../lib/components/AccountGate.svelte\';\n</script>\n\n<AccountGate />\n',
      sha256: '465852393264789ceae4133e8d79416ebf37d5485b08fb54c61056fa3714dc5d'
    },
    // Historical public-only variants must also restore their original private
    // shell. The successor route imports owned identity context, which those
    // historical roots never provided. Current actual-output cases retain it.
    'src/routes/messages/+page.svelte': {
      source:
        '<script lang="ts">\n  import AccountGate from \'../../lib/components/AccountGate.svelte\';\n</script>\n\n<AccountGate />\n',
      sha256: '23547dcfa10e85865e31b66c31ea0d043592e3c316c94da2d74bdf2dd419b38a'
    },
    'src/routes/messages/[conversationId=local_id]/+page.svelte': {
      source:
        '<script lang="ts">\n  import AccountGate from \'../../../lib/components/AccountGate.svelte\';\n</script>\n\n<AccountGate />\n',
      sha256: '93e21d162901af304f2a142acc7ddce87f7826421735d8a3af4ae281744ab3a0'
    }
  };
  // Exact accepted HCP105 public product producers, frozen before enquiry.
  // Restore only this public-only graph; current output retains HCP106 source.
  const historicalPublicProduct = {
    'src/routes/products/[naddr=naddr]/+page.svelte': {
      source:
        "<script lang=\"ts\">\n  import { getContext, onMount } from 'svelte';\n  import { page } from '$app/state';\n  import {\n    PUBLIC_RUNTIME_CONTEXT,\n    publicRuntimeReady,\n    createPublicView,\n    disposePublicView,\n    type PublicView,\n    type PublicRuntime,\n    type PublicRuntimeContext\n  } from '../../../lib/runtime/public-runtime.ts';\n  import {\n    createProductView,\n    closeProductView,\n    type ProductView,\n    type ProductViewSnapshot\n  } from '../../../lib/catalog/product-view.ts';\n  import { decodeProductReference } from '../../../lib/nostr/references.ts';\n  import { readPublicContact } from '../../../lib/contracts/food-availability-v1/contact-read.ts';\n  import { copyProductReference } from '../../../lib/navigation-copy.ts';\n  import { internalHref, navigationHref } from '../../../lib/navigation-url.ts';\n  import ListingFacts from '../../../lib/components/ListingFacts.svelte';\n  import PublisherIdentity from '../../../lib/components/PublisherIdentity.svelte';\n  import Disclosure from '../../../lib/components/primitives/Disclosure.svelte';\n  import Button from '../../../lib/components/primitives/Button.svelte';\n  const context = getContext<PublicRuntimeContext>(PUBLIC_RUNTIME_CONTEXT);\n  let runtime = $state.raw<PublicRuntime | undefined>(),\n    result = $state.raw<ProductViewSnapshot | undefined>(),\n    error = $state<string | undefined>(),\n    copyNotice = $state<string | undefined>();\n  const reference = $derived(decodeProductReference(page.params.naddr));\n  const contact = $derived(readPublicContact(result?.food?.content));\n  let disposed = false;\n  onMount(() => {\n    const unavailable = () => {\n      if (!disposed) error = 'Food sources are unavailable.';\n    };\n    try {\n      void publicRuntimeReady(context)\n        .then((value) => {\n          if (!disposed) {\n            runtime = value;\n            if (!value) unavailable();\n          }\n        })\n        .catch(unavailable);\n    } catch {\n      unavailable();\n    }\n    return () => {\n      disposed = true;\n    };\n  });\n  $effect(() => {\n    const selected = reference,\n      shared = runtime;\n    result = undefined;\n    copyNotice = undefined;\n    if (!selected || !shared) return;\n    let active = true,\n      acquired: PublicView | undefined,\n      owner: ProductView | undefined;\n    try {\n      acquired = createPublicView(shared);\n      owner = createProductView(\n        acquired,\n        selected.naddr,\n        { nowSeconds: () => Math.floor(Date.now() / 1000) },\n        (value) => {\n          if (active && !disposed) result = value;\n        }\n      );\n      error = undefined;\n    } catch {\n      error = 'Food sources are unavailable.';\n      if (acquired)\n        try {\n          disposePublicView(acquired);\n        } catch {\n          /* Shared runtime keeps a failed disposer retryable. */\n        }\n    }\n    return () => {\n      active = false;\n      if (owner) closeProductView(owner);\n    };\n  });\n  async function copyLink() {\n    const selected = reference;\n    if (!selected) return;\n    const copied = await copyProductReference(selected.naddr, page.url.origin);\n    if (!disposed && reference?.naddr === selected.naddr)\n      copyNotice = copied\n        ? 'Link copied.'\n        : 'Copy was unavailable. Select the public link below.';\n  }\n</script>\n\n<svelte:head\n  ><title>{result?.food?.title ?? 'Food'} — HarvestCircle</title></svelte:head\n>\n<div class=\"page page--reading stack\">\n  <a href={internalHref('/search')}>Search food</a>\n  <h1>{result?.food?.title ?? 'Food'}</h1>\n  {#if !reference}<p class=\"notice\">This food reference is invalid.</p>\n  {:else if error || result?.outcome === 'unavailable'}<p class=\"notice\">\n      Food sources are unavailable.\n    </p>\n  {:else if !result || result.outcome === 'checking'}<p>\n      Checking food sources…\n    </p>\n  {:else if result.outcome === 'unobserved'}<p class=\"notice\">\n      This listing was not observed in the sources checked. It may be\n      unavailable or missing from these sources.\n    </p>\n  {:else if result.outcome === 'withdrawn'}<p class=\"notice\">\n      This listing was withdrawn by its publisher.\n    </p>\n  {:else if result.outcome === 'unsupported'}<p class=\"notice\">\n      The latest known listing uses an unsupported food format.\n    </p>\n  {:else if result.outcome === 'future_quarantined' || result.outcome === 'clock_unavailable'}<p\n      class=\"notice\"\n    >\n      Listing time could not be confirmed. Food details are unavailable.\n    </p>{/if}\n  {#if result}\n    <PublisherIdentity publisher={result.publisher} />\n    {#if result.lastKnown}<p class=\"notice\">\n        This is last-known information. The listing status check is incomplete.\n      </p>{/if}\n    {#if result.food && result.createdAt !== undefined}<ListingFacts\n        food={result.food}\n        createdAt={result.createdAt}\n      />{/if}\n    {#if result.outcome === 'sold'}<p class=\"notice\">\n        The seller marked this listing sold.\n      </p>{/if}\n    {#if result.outcome === 'active'}<p>\n        Confirm availability with the seller.\n      </p>\n      <Button label=\"Message seller\" disabled={true} />\n      <p class=\"notice\">\n        In-app messaging is unavailable during development.\n      </p>{/if}\n    <Disclosure summary=\"Listing and source details\">\n      {#if result.eventId}<p>\n          Listing version: <code>{result.eventId}</code>\n        </p>{/if}\n      {#each result.sources as source (source.result.context)}<p>\n          {source.kind}: {source.state}. Coverage: {source.result.coverage}.\n        </p>\n        {#each source.result.sources as relay (relay.source)}<p>\n            {relay.source}: {relay.state}.\n          </p>{/each}{/each}\n      <p>\n        These bounded checks do not prove complete coverage or current stock.\n      </p>\n    </Disclosure>\n    {#if contact && result.outcome === 'active'}<Disclosure\n        summary=\"Other contact details\"\n        ><a href={navigationHref(contact.href)}>{contact.href}</a></Disclosure\n      >{/if}\n  {/if}\n  {#if reference}<Button label=\"Copy link\" onclick={copyLink} /><a\n      href={internalHref(`/products/${reference.naddr}`)}>Public food link</a\n    >{/if}\n  {#if copyNotice}<p role=\"status\">{copyNotice}</p>{/if}\n</div>\n",
      sha256: '1869d4dc32ea794d5ee5c1d6cc6b4aac94483482d18cff0562c0cb10087b5278'
    },
    'src/lib/catalog/product-view.ts': {
      source:
        "import { PUBLIC_QUERY_BUDGETS } from '../config/budgets.ts';\nimport {\n  decodeProductReference,\n  type ProductReference\n} from '../nostr/references.ts';\nimport {\n  beginPublicViewRun,\n  disposePublicView,\n  publicViewProjectionAvailable,\n  publicViewRunCurrent,\n  publicViewOwnsRun,\n  subscribePublicView,\n  type PublicView\n} from '../runtime/public-runtime.ts';\nimport {\n  publicRequestScopeSnapshot,\n  publicRunSnapshot,\n  closePublicRequest,\n  disposePublicRun,\n  type PublicRequest\n} from '../nostr/request-scope.ts';\nimport {\n  createPublicHeadCandidate,\n  publicHeadSnapshot,\n  selectPublicHead,\n  type PublicHead\n} from './heads.ts';\nimport {\n  createPublicViewHeadResolver,\n  headResolutionSnapshot,\n  resolveHeads,\n  closeHeadResolver\n} from './resolve-head.ts';\nimport {\n  createPublisherViews,\n  readPublisherPage,\n  publisherSnapshot,\n  type PublisherView\n} from './publishers.ts';\nimport type { FoodProjection } from '../contracts/food-availability-v1/read.ts';\nimport type { WallClock } from './clock-policy.ts';\nimport type { ForegroundSchedule } from './search-view.ts';\nexport type ProductViewSnapshot = Readonly<{\n  reference: ProductReference;\n  outcome:\n    | 'checking'\n    | 'unobserved'\n    | 'unavailable'\n    | 'active'\n    | 'sold'\n    | 'withdrawn'\n    | 'unsupported'\n    | 'future_quarantined'\n    | 'clock_unavailable';\n  food: FoodProjection | undefined;\n  eventId: string | undefined;\n  createdAt: number | undefined;\n  publisher: PublisherView;\n  lastKnown: boolean;\n  sources: readonly ReturnType<typeof publicRequestScopeSnapshot>[];\n}>;\ndeclare const productBrand: unique symbol;\nexport type ProductView = Readonly<{ readonly [productBrand]: true }>;\ninterface Owner {\n  snapshot: () => ProductViewSnapshot;\n  close: () => void;\n}\nconst owners = new WeakMap<ProductView, Owner>();\nfunction ownerOf(token: ProductView): Owner {\n  const owner = owners.get(token);\n  if (!owner) throw Error('product_view_invalid');\n  return owner;\n}\nfunction foreground(callback: () => void, delay: number): () => void {\n  const timer = setTimeout(() => {\n    callback();\n  }, delay);\n  return () => {\n    clearTimeout(timer);\n  };\n}\n// One explicit route read, one coordinate, shared run/request budgets. No forged\n// seed, independent cache, relay hints, automatic refresh or additional deadline.\nexport function createProductView(\n  view: PublicView,\n  value: unknown,\n  clock: WallClock,\n  onUpdate: (value: ProductViewSnapshot) => void,\n  schedule: ForegroundSchedule = foreground\n): ProductView {\n  const decoded = decodeProductReference(value);\n  if (!decoded) throw Error('product_reference_invalid');\n  const reference = decoded;\n  const run = beginPublicViewRun(view),\n    resolver = createPublicViewHeadResolver(view, run, clock),\n    publishers = createPublisherViews(view, clock);\n  let closed = false,\n    failed = false,\n    head: PublicHead | undefined,\n    cancel: (() => void) | undefined;\n  const requests = new Map<PublicRequest, true>();\n  const current = () => !closed && publicViewRunCurrent(view, run);\n  function snapshot(): ProductViewSnapshot {\n    const available =\n      !closed &&\n      publicViewProjectionAvailable(view) &&\n      publicViewOwnsRun(view, run);\n    const row = available ? headResolutionSnapshot(resolver)[0] : undefined;\n    let scopes = Array.from(requests.keys(), publicRequestScopeSnapshot);\n    for (const scope of [row?.headSources, row?.deletionSources])\n      if (\n        scope &&\n        !scopes.some((s) => s.result.context === scope.result.context)\n      )\n        scopes = scopes.concat(scope);\n    const complete =\n      !failed &&\n      scopes.length > 0 &&\n      scopes.every(\n        (s) => s.state === 'eose' && s.result.coverage === 'bounded-eose'\n      );\n    let outcome: ProductViewSnapshot['outcome'];\n    if (!available) outcome = 'unavailable';\n    else if (row?.deletion.outcome === 'suppressed') outcome = 'withdrawn';\n    else if (\n      row?.state.display === 'future_quarantined' ||\n      row?.state.display === 'clock_unavailable'\n    )\n      outcome = row.state.display;\n    else if (row?.state.display === 'unsupported') outcome = 'unsupported';\n    else if (row?.state.food) outcome = row.state.food.status;\n    else if (\n      !row &&\n      (failed ||\n        (scopes.length > 0 &&\n          scopes.every(\n            (scope) =>\n              scope.result.sources.length > 0 &&\n              scope.result.sources.every(\n                (source) =>\n                  source.state === 'error' || source.state === 'closed'\n              )\n          )))\n    )\n      outcome = 'unavailable';\n    else\n      outcome =\n        current() && scopes.some((s) => s.state === 'active')\n          ? 'checking'\n          : 'unobserved';\n    const publisher = publisherSnapshot(publishers, reference.pubkey);\n    const stillAvailable =\n      !closed &&\n      publicViewProjectionAvailable(view) &&\n      publicViewOwnsRun(view, run);\n    return {\n      reference: { ...reference },\n      outcome: stillAvailable ? outcome : 'unavailable',\n      food: stillAvailable ? row?.state.food : undefined,\n      eventId: stillAvailable ? row?.state.head.id : undefined,\n      createdAt: stillAvailable ? row?.state.head.created_at : undefined,\n      publisher: stillAvailable\n        ? publisher\n        : {\n            pubkey: reference.pubkey,\n            label: reference.pubkey,\n            assertedName: false\n          },\n      lastKnown: !stillAvailable || !complete || (row?.lastKnown ?? true),\n      sources: scopes\n    };\n  }\n  function publish() {\n    const value = snapshot();\n    if (!closed && publicViewOwnsRun(view, run)) onUpdate(value);\n  }\n  function queue() {\n    if (!current() || publicRunSnapshot(run).activeRequests === 0) return;\n    const next = schedule(() => {\n      if (closed || !publicViewOwnsRun(view, run)) return;\n      cancel = undefined;\n      publish();\n      queue();\n    }, 250);\n    if (current()) cancel = next;\n    else next();\n  }\n  const token = Object.freeze({}) as ProductView;\n  owners.set(token, {\n    snapshot,\n    close() {\n      closed = true;\n      const previous = cancel;\n      cancel = undefined;\n      previous?.();\n      let error = false;\n      try {\n        closeHeadResolver(resolver);\n      } catch {\n        error = true;\n      }\n      try {\n        if (publicViewOwnsRun(view, run)) disposePublicView(view);\n        else disposePublicRun(run);\n      } catch {\n        error = true;\n      }\n      if (error) throw Error('product_view_close_failed');\n    }\n  });\n  try {\n    const filter =\n      reference.identifier === ''\n        ? {\n            kinds: [30402],\n            authors: [reference.pubkey],\n            limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay\n          }\n        : {\n            kinds: [30402],\n            authors: [reference.pubkey],\n            '#d': [reference.identifier],\n            limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay\n          };\n    const request = subscribePublicView(\n      view,\n      run,\n      'head',\n      [filter],\n      (proof) => {\n        if (!current()) return;\n        const candidate = createPublicHeadCandidate(proof);\n        if (!candidate) return;\n        const next = publicHeadSnapshot(candidate);\n        if (\n          next.kind !== 30402 ||\n          next.pubkey !== reference.pubkey ||\n          next.identifier !== reference.identifier\n        )\n          return;\n        head = selectPublicHead(head, candidate).head;\n        try {\n          resolveHeads(resolver, [head]);\n          if (current())\n            for (const request of readPublisherPage(publishers, run, [\n              reference.pubkey\n            ]))\n              requests.set(request, true);\n        } catch {\n          failed = true;\n        }\n      }\n    );\n    requests.set(request, true);\n    if (publicRequestScopeSnapshot(request).result.sources.length === 0) {\n      failed = true;\n      closePublicRequest(request);\n    }\n  } catch {\n    failed = true;\n  }\n  publish();\n  queue();\n  return token;\n}\nexport function productViewSnapshot(token: ProductView): ProductViewSnapshot {\n  return ownerOf(token).snapshot();\n}\nexport function closeProductView(token: ProductView): void {\n  ownerOf(token).close();\n}\n",
      sha256: 'e3739170e3c700246a80c1358c7b96541a5cbf1e4c26905e6ca60537adfce9cf'
    }
  };
  const base = await mkdtemp(path.join(os.tmpdir(), 'hc actual output '));
  let setupReady = false;
  /** @param {import('node:test').TestContext} t */
  function active(t) {
    if (t.signal.aborted) throw new Error('Output fixture test aborted');
  }
  /** @param {import('node:test').TestContext} t */
  function ready(t) {
    active(t);
    assert.ok(setupReady, 'Actual output setup must complete before consumers');
  }
  try {
    /** @param {string} root @param {string[]} args */
    const git = (root, ...args) =>
      execFileSync('git', ['-C', root, ...args], {
        env: Object.fromEntries(
          Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_'))
        ),
        encoding: 'utf8'
      });
    /** @param {import('node:test').TestContext} t */
    async function fixture(t) {
      ready(t);
      const root = await realpath(
        await mkdtemp(path.join(os.tmpdir(), 'hc output mutation '))
      );
      t.after(() => rm(root, { recursive: true, force: true }));
      // Audit consumers use actual client/prerendered/build bytes and all source
      // inputs. Server output and generated development/type caches are not audit
      // inputs; avoid copying them for every mutation under the fixed deadline.
      const excluded = new Set([
        path.join(base, 'web/node_modules'),
        path.join(base, 'web/.svelte-kit/output/server'),
        path.join(base, 'web/.svelte-kit/generated'),
        path.join(base, 'web/.svelte-kit/types')
      ]);
      await cp(base, root, {
        recursive: true,
        filter: (name) => !excluded.has(name)
      });
      active(t);
      const web = path.join(root, 'web');
      return { root, web, audit: () => auditOutput(web) };
    }
    /** @param {{root: string, web: string}} f */
    async function refresh(f) {
      git(f.root, 'add', '--', 'web/src');
      await generateBuildInfo(f.web);
      const bytes = await readFile(path.join(f.web, 'static/build-info.json'));
      for (const name of [
        'build/build-info.json',
        '.svelte-kit/output/client/build-info.json'
      ])
        await writeFile(path.join(f.web, name), bytes);
    }

    const sdkKey =
      'node_modules/.pnpm/applesauce-relay@6.2.1_typescript@6.0.3/node_modules/applesauce-relay/dist/negentropy.js';
    /** @param {import('node:test').TestContext} t @param {string} from @param {string} web */
    async function copyDependencies(t, from, web) {
      active(t);
      const start = performance.now();
      await cp(
        path.join(from, 'web/node_modules'),
        path.join(web, 'node_modules'),
        {
          recursive: true,
          verbatimSymlinks: true
        }
      );
      active(t);
      t.diagnostic(
        `actual dependency copy milliseconds: ${performance.now() - start}`
      );
    }
    /** @param {import('node:test').TestContext} t @param {string} web */
    function compile(t, web) {
      active(t);
      const start = performance.now();
      try {
        execFileSync(
          process.execPath,
          [path.join(source, 'web/node_modules/vite/bin/vite.js'), 'build'],
          { cwd: web, stdio: 'pipe', timeout: 1800000 }
        );
        active(t);
      } finally {
        t.diagnostic(
          `actual Vite compilation milliseconds: ${performance.now() - start}`
        );
      }
    }
    await test(
      'actual output fixture setup',
      { timeout: 1800000 },
      async (t) => {
        // Compile actual current source once in isolation. No install/native subprocess.
        for (const name of git(
          source,
          'ls-files',
          '--cached',
          '--others',
          '--exclude-standard',
          '-z',
          '--',
          'web',
          '.gitignore',
          'core/Cargo.lock',
          'radroots.lib.source-lock.v1.toml'
        )
          .split('\0')
          .filter(Boolean)) {
          const stat = await lstat(path.join(source, name));
          assert.ok(
            stat.isFile() && stat.nlink === 1 && stat.size <= 8 * 1024 * 1024,
            'Only safe actual source fixture inputs'
          );
          await mkdir(path.dirname(path.join(base, name)), { recursive: true });
          await writeFile(
            path.join(base, name),
            await readFile(path.join(source, name))
          );
        }
        git(base, 'init', '-q');
        git(base, 'add', '.');
        git(
          base,
          '-c',
          'user.name=Output Test',
          '-c',
          'user.email=output@example.invalid',
          'commit',
          '-qm',
          'actual inputs'
        );
        await copyDependencies(t, source, path.join(base, 'web'));
        await generateBuildInfo(path.join(base, 'web'));
        compile(t, path.join(base, 'web'));
        setupReady = true;
      }
    );
    await test(
      'HCP115 exact shared-editor predecessor remains independently compilable',
      { timeout: 1800000 },
      async (t) => {
        const f = await fixture(t);
        await restoreHistoricalPublicProducers(f.web);
        const historical = {
          'src/routes/sell/+page.svelte': {
            source:
              "<script lang=\"ts\">\n  import ListingEditor from '../../lib/components/ListingEditor.svelte';\n</script>\n\n<ListingEditor route={{ mode: 'new' }} />\n",
            sha256:
              '3e99a6121e9ea317d30e631d9edfb05e6596301678f9ae0538e446f0438ae3f4'
          },
          'src/routes/selling/drafts/[draftId=local_id]/+page.svelte': {
            source:
              "<script lang=\"ts\">\n  import { page } from '$app/state';\n  import ListingEditor from '../../../../lib/components/ListingEditor.svelte';\n  import type { ListingEditorRoute } from '../../../../lib/publishing/listing-editor.ts';\n  const route = $derived({\n    mode: 'draft',\n    draftId: page.params.draftId ?? ''\n  } satisfies ListingEditorRoute);\n</script>\n\n<ListingEditor {route} />\n",
            sha256:
              '98935ebb01f950aee5da850758801c28303b042ba219759c4fb04c4974376a76'
          },
          'src/routes/products/[naddr=naddr]/edit/+page.svelte': {
            source:
              "<script lang=\"ts\">\n  import { page } from '$app/state';\n  import ListingEditor from '../../../../lib/components/ListingEditor.svelte';\n  import type { ListingEditorRoute } from '../../../../lib/publishing/listing-editor.ts';\n  const route = $derived({\n    mode: 'edit',\n    naddr: page.params.naddr ?? ''\n  } satisfies ListingEditorRoute);\n</script>\n\n<ListingEditor {route} />\n",
            sha256:
              '5c50dc2ba4c5734ffe4bd1c2e406e6699581b374fab3849d5c5a0072d9bad1e6'
          },
          'src/lib/components/ListingEditor.svelte': {
            source:
              '<script lang="ts">\n  import { getContext, onMount } from \'svelte\';\n  import CapabilityGate from \'./CapabilityGate.svelte\';\n  import Notice from \'./Notice.svelte\';\n  import { foodUnits } from \'../contracts/food-availability-v1/values.ts\';\n  import type { PublicDraftForm } from \'../contracts/local-records.ts\';\n  import {\n    IDENTITY_VIEW_CONTEXT,\n    identityViewSnapshot,\n    subscribeIdentityView,\n    connectIdentityView,\n    disconnectIdentityView,\n    type IdentityViewContext\n  } from \'../runtime/view-context.ts\';\n  import {\n    openListingEditor,\n    listingEditorSnapshot,\n    editListingEditor,\n    closeListingEditor,\n    subscribeListingEditor,\n    type ListingEditor as Editor,\n    type ListingEditorRoute,\n    type ListingEditorSnapshot\n  } from \'../publishing/listing-editor.ts\';\n  import { internalHref } from \'../navigation-url.ts\';\n  let { route }: { route: ListingEditorRoute } = $props();\n  const context = getContext<IdentityViewContext>(IDENTITY_VIEW_CONTEXT);\n  let identity = $state(identityViewSnapshot(context)),\n    mounted = $state(false),\n    editor = $state.raw<Editor | undefined>(),\n    snapshot = $state<ListingEditorSnapshot>(),\n    unavailable = $state(false);\n  type Form = {\n    -readonly [Key in keyof PublicDraftForm]: PublicDraftForm[Key];\n  };\n  const blankForm = (): Form => ({\n    title: \'\',\n    description: \'\',\n    location: \'\',\n    amount: \'\',\n    currency: \'\',\n    unit: \'\',\n    quantity: \'\',\n    contactType: \'\',\n    contactValue: \'\'\n  });\n  let form = $state<Form>(blankForm());\n  const admittedAuthor = $derived(\n    identity.identity.state !== \'guest\' && identity.identity.state !== \'pending\'\n      ? identity.identity.publicKey\n      : undefined\n  );\n  const active = $derived(mounted && identity.mounted);\n  const visible = $derived(\n    snapshot &&\n      editor &&\n      identity.mounted &&\n      identity.identity.state !== \'guest\' &&\n      identity.identity.state !== \'pending\' &&\n      identity.identity.publicKey === snapshot.owner &&\n      listingEditorSnapshot(editor)\n  );\n  onMount(() => {\n    mounted = true;\n    const off = subscribeIdentityView(context, (next) => {\n      identity = next;\n    });\n    return () => {\n      mounted = false;\n      off();\n    };\n  });\n  $effect(() => {\n    const resource = route,\n      author = admittedAuthor;\n    if (!active || !author) {\n      editor = undefined;\n      snapshot = undefined;\n      form = blankForm();\n      unavailable = false;\n      return;\n    }\n    const cancellation = new AbortController();\n    let alive = true,\n      owned: Editor | undefined,\n      off = () => {};\n    editor = undefined;\n    snapshot = undefined;\n    form = blankForm();\n    unavailable = false;\n    void openListingEditor(context, resource, cancellation.signal).then(\n      (result) => {\n        if (!alive) {\n          if (result.status === \'ready\') closeListingEditor(result.editor);\n          return;\n        }\n        if (result.status !== \'ready\') {\n          unavailable = true;\n          return;\n        }\n        owned = result.editor;\n        editor = owned;\n        const next = listingEditorSnapshot(owned);\n        if (!next) {\n          unavailable = true;\n          return;\n        }\n        snapshot = next;\n        form = { ...next.form };\n        off = subscribeListingEditor(owned, () => {\n          snapshot = owned && listingEditorSnapshot(owned);\n          if (!snapshot) {\n            unavailable = true;\n            form = blankForm();\n          }\n        });\n      }\n    );\n    return () => {\n      alive = false;\n      cancellation.abort();\n      off();\n      if (owned) closeListingEditor(owned);\n      form = blankForm();\n    };\n  });\n  function changed() {\n    const original = editor;\n    if (!original || !listingEditorSnapshot(original)?.editable) return;\n    void editListingEditor(original, { ...form });\n  }\n  function connect() {\n    return connectIdentityView(context);\n  }\n  function disconnect() {\n    disconnectIdentityView(context);\n  }\n  const title = $derived(\n    route.mode === \'new\'\n      ? \'New listing\'\n      : route.mode === \'draft\'\n        ? \'Continue draft\'\n        : \'Edit listing\'\n  );\n</script>\n\n<svelte:head\n  ><title>{title} \u00b7 HarvestCircle</title><meta\n    name="robots"\n    content="noindex"\n  /></svelte:head\n>\n<div class="page page--reading stack">\n  <a\n    href={internalHref(\n      route.mode === \'edit\' ? `/products/${route.naddr}` : \'/selling\'\n    )}>{route.mode === \'edit\' ? \'Back to listing\' : \'Back to Selling\'}</a\n  >\n  <h1>{title}</h1>\n  {#if !visible}\n    <CapabilityGate\n      identity={identity.identity}\n      required="connection"\n      focusTarget="protected"\n      onconnect={mounted ? connect : undefined}\n      ondisconnect={disconnect}\n    />\n    {#if unavailable}<Notice\n        tone="warning"\n        text={route.mode === \'draft\'\n          ? \'This draft is not available here.\'\n          : route.mode === \'edit\'\n            ? \'Supported local listing values are not available for editing. Keep this listing read-only.\'\n            : \'Browser storage is unavailable. Your existing local data has not been reset.\'}\n      />{:else if identity.identity.state !== \'guest\' && identity.identity.state !== \'pending\'}<p\n        role="status"\n      >\n        Loading local values.\n      </p>{/if}\n  {:else}\n    {#if !visible.editable}<Notice\n        tone="info"\n        text="These retained local values are read-only until the current listing is checked."\n      />{:else}<p>\n        Changes are saved in this browser. Browser storage is not a backup.\n      </p>{/if}\n    <form class="stack" onsubmit={(event) => event.preventDefault()} novalidate>\n      <div class="field">\n        <label class="label" for="listing-title">Product name</label><input\n          id="listing-title"\n          type="text"\n          value={form.title}\n          oninput={(event) => {\n            form = { ...form, title: event.currentTarget.value };\n            changed();\n          }}\n          readonly={!visible.editable}\n        />\n      </div>\n      <div class="field">\n        <label class="label" for="listing-description">Description</label\n        ><textarea\n          id="listing-description"\n          rows="6"\n          value={form.description}\n          oninput={(event) => {\n            form = { ...form, description: event.currentTarget.value };\n            changed();\n          }}\n          readonly={!visible.editable}></textarea>\n      </div>\n      <div class="field">\n        <label class="label" for="listing-location">Public area</label>\n        <p id="listing-location-hint" class="text-small">\n          A town or collection area is enough.\n        </p>\n        <input\n          id="listing-location"\n          type="text"\n          aria-describedby="listing-location-hint"\n          value={form.location}\n          oninput={(event) => {\n            form = { ...form, location: event.currentTarget.value };\n            changed();\n          }}\n          readonly={!visible.editable}\n        />\n      </div>\n      <div class="cluster">\n        <div class="field">\n          <label class="label" for="listing-amount">Price</label><input\n            id="listing-amount"\n            type="text"\n            inputmode="decimal"\n            value={form.amount}\n            oninput={(event) => {\n              form = { ...form, amount: event.currentTarget.value };\n              changed();\n            }}\n            readonly={!visible.editable}\n          />\n        </div>\n        <div class="field">\n          <label class="label" for="listing-currency">Currency</label><input\n            id="listing-currency"\n            type="text"\n            maxlength="3"\n            value={form.currency}\n            oninput={(event) => {\n              form = { ...form, currency: event.currentTarget.value };\n              changed();\n            }}\n            readonly={!visible.editable}\n          />\n        </div>\n        <div class="field">\n          <label class="label" for="listing-unit">Per</label><select\n            id="listing-unit"\n            value={form.unit}\n            onchange={(event) => {\n              form = { ...form, unit: event.currentTarget.value };\n              changed();\n            }}\n            disabled={!visible.editable}\n            ><option value="">Choose a unit</option\n            >{#each foodUnits as unit (unit)}<option value={unit}>{unit}</option\n              >{/each}</select\n          >\n        </div>\n      </div>\n      <div class="field">\n        <label class="label" for="listing-quantity"\n          >Advertised quantity (optional)</label\n        >\n        <p id="listing-quantity-unit">\n          {form.unit || \'Uses the selected price unit\'}\n        </p>\n        <input\n          id="listing-quantity"\n          type="text"\n          inputmode="decimal"\n          aria-describedby="listing-quantity-unit"\n          value={form.quantity}\n          oninput={(event) => {\n            form = { ...form, quantity: event.currentTarget.value };\n            changed();\n          }}\n          readonly={!visible.editable}\n        />\n      </div>\n      <details>\n        <summary>Add public contact details (optional)</summary>\n        <div class="stack">\n          <p>Only add contact details you want to include publicly.</p>\n          <div class="field">\n            <label class="label" for="listing-contact-type"\n              >Public contact type</label\n            ><select\n              id="listing-contact-type"\n              value={form.contactType}\n              onchange={(event) => {\n                const value = event.currentTarget.value;\n                if (\n                  value === \'\' ||\n                  value === \'email\' ||\n                  value === \'phone\' ||\n                  value === \'https\'\n                ) {\n                  form = { ...form, contactType: value };\n                  changed();\n                }\n              }}\n              disabled={!visible.editable}\n              ><option value="">No public contact</option><option value="email"\n                >Email</option\n              ><option value="phone">Phone</option><option value="https"\n                >Website</option\n              ></select\n            >\n          </div>\n          <div class="field">\n            <label class="label" for="listing-contact-value"\n              >Public contact details</label\n            ><input\n              id="listing-contact-value"\n              type="text"\n              value={form.contactValue}\n              oninput={(event) => {\n                form = { ...form, contactValue: event.currentTarget.value };\n                changed();\n              }}\n              readonly={!visible.editable}\n            />\n          </div>\n        </div>\n      </details>\n    </form>\n    {#if visible.editable}<p role="status">\n        {visible.state === \'saved\'\n          ? \'Saved in this browser.\'\n          : visible.state === \'saving\'\n            ? \'Saving in this browser.\'\n            : visible.state === \'failed\'\n              ? \'Not saved. Keep your current input here.\'\n              : \'Not yet saved.\'}\n      </p>\n      {#if visible.failure}<Notice\n          tone="warning"\n          text="Saving did not complete. Keep your current input. Existing local data has not been reset."\n        />{/if}{/if}\n  {/if}\n</div>\n',
            sha256:
              'fed47cd7257ab3fe1878ef5779b74a4a3e01404d93956806477943190ffb2b18'
          },
          'src/lib/publishing/listing-editor.ts': {
            source:
              "import {\n  exactLocalFields,\n  type PublicDraftForm\n} from '../contracts/local-records.ts';\nimport { canonicalLocalId } from '../private-handles.ts';\nimport { decodeProductReference } from '../nostr/references.ts';\nimport { inspectLocalTemplate } from '../nostr/local-template.ts';\nimport { projectFoodParts } from '../contracts/food-availability-v1/read.ts';\nimport { buildFoodTemplate } from '../contracts/food-availability-v1/write.ts';\nimport {\n  openBrowserDatabase,\n  closeBrowserDatabase,\n  type BrowserDatabase\n} from '../persistence/database.ts';\nimport {\n  createPublicPublicationRepository,\n  publicPublicationDraftRepository,\n  publicPublicationQuotaRepository,\n  readPublicListingIntent\n} from '../persistence/public-publications.ts';\nimport {\n  inspectPublicStorage,\n  publicInventorySnapshot\n} from '../persistence/quota.ts';\nimport { publicRecordSnapshot } from '../persistence/records.ts';\nimport {\n  readPublicDraft,\n  publicDraftRepositoryOwner,\n  type PublicDraftRepository\n} from '../persistence/drafts.ts';\nimport {\n  createPublicDraftController,\n  publicDraftControllerSnapshot,\n  editPublicDraft,\n  flushPublicDraft,\n  closePublicDraftController,\n  type PublicDraftController\n} from './draft-controller.ts';\nimport {\n  identityViewSession,\n  identityViewMatchesOwner,\n  type IdentityViewContext\n} from '../runtime/view-context.ts';\nimport {\n  identitySessionSnapshot,\n  subscribeIdentityInvalidation\n} from '../runtime/identity-session.ts';\n\ndeclare const editorBrand: unique symbol;\nexport type ListingEditor = Readonly<{ [editorBrand]: true }>;\nexport type ListingEditorRoute =\n  | Readonly<{ mode: 'new' }>\n  | Readonly<{ mode: 'draft'; draftId: string }>\n  | Readonly<{ mode: 'edit'; naddr: string }>;\nexport type ListingEditorSnapshot = Readonly<{\n  mode: ListingEditorRoute['mode'];\n  owner: string;\n  form: PublicDraftForm;\n  editable: boolean;\n  draftId: string | null;\n  state: 'editing' | 'saving' | 'saved' | 'failed' | 'read-only';\n  failure: string | null;\n  coordinate: string | null;\n  sourceEventId: string | null;\n}>;\ntype Editor = Readonly<{\n  current(): boolean;\n  retire(): void;\n  snapshot(): ListingEditorSnapshot | undefined;\n  edit(form: unknown): Promise<boolean>;\n  flush(): Promise<boolean>;\n  subscribe(listener: () => void): () => void;\n}>;\nconst editors = new WeakMap<ListingEditor, Editor>();\nconst active = new WeakMap<\n  IdentityViewContext,\n  Readonly<{ stamp: symbol; retire(): void }>\n>();\nconst emptyForm: PublicDraftForm = {\n  title: '',\n  description: '',\n  location: '',\n  amount: '',\n  currency: '',\n  unit: '',\n  quantity: '',\n  contactType: '',\n  contactValue: ''\n};\nfunction routeSnapshot(value: unknown): ListingEditorRoute | undefined {\n  try {\n    if (exactLocalFields(value, ['mode']) && value.mode === 'new')\n      return { mode: 'new' };\n    if (\n      exactLocalFields(value, ['mode', 'draftId']) &&\n      value.mode === 'draft'\n    ) {\n      const draftId = canonicalLocalId(value.draftId);\n      return draftId ? { mode: 'draft', draftId } : undefined;\n    }\n    if (exactLocalFields(value, ['mode', 'naddr']) && value.mode === 'edit') {\n      const reference = decodeProductReference(value.naddr);\n      return reference ? { mode: 'edit', naddr: reference.naddr } : undefined;\n    }\n  } catch {\n    return;\n  }\n}\n// Owner admission precedes database opening and all protected hydration. Edit\n// mode presents validated local values; independent head authorization is later.\nexport async function openListingEditor(\n  context: IdentityViewContext,\n  input: unknown,\n  signal?: AbortSignal\n): Promise<\n  | Readonly<{ status: 'ready'; editor: ListingEditor }>\n  | Readonly<{\n      status: 'unavailable';\n      reason: 'owner' | 'resource' | 'storage' | 'source';\n    }>\n> {\n  const session = identityViewSession(context),\n    route = routeSnapshot(input);\n  if (!session || signal?.aborted)\n    return { status: 'unavailable', reason: 'owner' };\n  const identity = identitySessionSnapshot(session),\n    owner = 'publicKey' in identity ? identity.publicKey : undefined;\n  if (!owner || !identityViewMatchesOwner(context, owner))\n    return { status: 'unavailable', reason: 'owner' };\n  if (\n    !route ||\n    (route.mode === 'edit' &&\n      decodeProductReference(route.naddr)?.pubkey !== owner)\n  )\n    return { status: 'unavailable', reason: 'resource' };\n  if (typeof window === 'undefined' || !navigator.locks?.request)\n    return { status: 'unavailable', reason: 'storage' };\n  active.get(context)?.retire();\n  const stamp = Symbol(),\n    listeners = new Map<symbol, () => void>(),\n    cancellation = new AbortController();\n  let alive = true,\n    unsubscribe = () => {},\n    database: BrowserDatabase | undefined,\n    repository: PublicDraftRepository | undefined,\n    controller: PublicDraftController | undefined;\n  function current() {\n    return (\n      alive &&\n      active.get(context)?.stamp === stamp &&\n      identityViewSession(context) === session &&\n      identityViewMatchesOwner(context, owner) &&\n      (!repository || publicDraftRepositoryOwner(repository) === owner)\n    );\n  }\n  let notifying = false;\n  function notify() {\n    if (notifying) return;\n    notifying = true;\n    try {\n      for (const [key, listener] of Array.from(listeners.entries()))\n        if (listeners.get(key) === listener) {\n          try {\n            listener();\n          } catch {\n            /*Presentation observers cannot authorize or retain ownership.*/\n          }\n        }\n    } finally {\n      notifying = false;\n    }\n  }\n  function retire() {\n    if (!alive) return;\n    alive = false;\n    unsubscribe();\n    signal?.removeEventListener('abort', retire);\n    cancellation.abort();\n    if (controller) closePublicDraftController(controller);\n    if (database) closeBrowserDatabase(database);\n    if (active.get(context)?.stamp === stamp) active.delete(context);\n    notify();\n    listeners.clear();\n  }\n  active.set(context, { stamp, retire });\n  unsubscribe = subscribeIdentityInvalidation(session, retire);\n  signal?.addEventListener('abort', retire, { once: true });\n  if (signal?.aborted) retire();\n  function unavailable(reason: 'resource' | 'storage' | 'source') {\n    retire();\n    return { status: 'unavailable' as const, reason };\n  }\n  try {\n    const opened = await openBrowserDatabase(cancellation.signal);\n    if (opened.state !== 'ready') return unavailable('storage');\n    if (!current()) {\n      closeBrowserDatabase(opened.owner);\n      return unavailable('storage');\n    }\n    database = opened.owner;\n    const publications = createPublicPublicationRepository(database, owner);\n    repository = publications && publicPublicationDraftRepository(publications);\n    if (!publications || !repository || !current())\n      return unavailable('storage');\n    const drafts = repository;\n    let sourceForm: PublicDraftForm | undefined,\n      coordinate: string | null = null,\n      sourceEventId: string | null = null;\n    if (route.mode === 'edit') {\n      const reference = decodeProductReference(route.naddr),\n        quota = publicPublicationQuotaRepository(publications);\n      if (!reference || !quota) return unavailable('source');\n      const inspected = await inspectPublicStorage(quota);\n      if (!current() || !inspected.ok) return unavailable('source');\n      const inventory = publicInventorySnapshot(quota, inspected.value);\n      if (!inventory) return unavailable('source');\n      let matching:\n        Readonly<{ form: PublicDraftForm; eventId: string }> | undefined;\n      for (const item of inventory.rows) {\n        if (item.family !== 'public_operation') continue;\n        const read = await readPublicListingIntent(publications, item.id);\n        if (!current()) return unavailable('source');\n        const row =\n          read.status === 'found' &&\n          publicRecordSnapshot(read.record, owner, item.id);\n        if (\n          !row ||\n          row.family !== 'public_operation' ||\n          row.capture.kind !== 30402 ||\n          !row.artifact ||\n          row.source.type !== 'draft'\n        )\n          continue;\n        const template = inspectLocalTemplate(row.capture.wire, owner, 30402);\n        if (!template) continue;\n        const projected = projectFoodParts(template);\n        if (\n          projected.outcome !== 'focused' ||\n          projected.projection.identifier !== reference.identifier\n        )\n          continue;\n        const source = await readPublicDraft(drafts, row.source.id);\n        if (!current()) return unavailable('source');\n        if (!source.ok || source.value.revision !== row.source.revision)\n          continue;\n        const value = source.value.form,\n          p = projected.projection;\n        if (value.contactType === '' && value.contactValue !== '') continue;\n        const built = buildFoodTemplate({\n          identifier: p.identifier,\n          title: value.title,\n          description: value.description,\n          location: value.location,\n          amount: value.amount,\n          currency: value.currency,\n          unit: value.unit,\n          ...(value.quantity === '' ? {} : { quantity: value.quantity }),\n          published_at: p.published_at,\n          created_at: template.created_at,\n          status: p.status,\n          ...(value.contactType === ''\n            ? {}\n            : {\n                contact: {\n                  type: value.contactType,\n                  value: value.contactValue,\n                  public: true as const\n                }\n              })\n        });\n        if (\n          !built.ok ||\n          JSON.stringify({\n            pubkey: owner,\n            kind: 30402,\n            created_at: template.created_at,\n            tags: built.wire_parts.tags,\n            content: built.wire_parts.content\n          }) !== row.capture.wire\n        )\n          continue;\n        if (matching) return unavailable('source');\n        matching = { form: value, eventId: row.artifact.eventId };\n      }\n      if (!matching) return unavailable('source');\n      sourceForm = matching.form;\n      coordinate = route.naddr;\n      sourceEventId = matching.eventId;\n    } else {\n      const acquired = await createPublicDraftController(\n        drafts,\n        emptyForm,\n        route.mode === 'draft' ? route.draftId : undefined\n      );\n      if (!current()) {\n        if (acquired) closePublicDraftController(acquired);\n        return unavailable('resource');\n      }\n      controller = acquired;\n      if (!controller) return unavailable('resource');\n    }\n    if (!current()) return unavailable('source');\n    const value: Editor = {\n      current,\n      retire,\n      snapshot() {\n        if (!current()) {\n          retire();\n          return;\n        }\n        const local = controller && publicDraftControllerSnapshot(controller);\n        return {\n          mode: route.mode,\n          owner,\n          form: local ? local.form : { ...(sourceForm ?? emptyForm) },\n          editable: route.mode !== 'edit',\n          draftId: local?.draft?.id ?? null,\n          state: local\n            ? local.state === 'closed'\n              ? 'failed'\n              : local.state\n            : 'read-only',\n          failure: local?.failure ?? null,\n          coordinate,\n          sourceEventId\n        };\n      },\n      async edit(input) {\n        if (!current()) {\n          retire();\n          return false;\n        }\n        if (!controller || route.mode === 'edit') return false;\n        if (!editPublicDraft(controller, input)) return false;\n        notify();\n        const pending = flushPublicDraft(controller);\n        await Promise.resolve();\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        const saved = await pending;\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        return saved;\n      },\n      async flush() {\n        if (!current()) {\n          retire();\n          return false;\n        }\n        if (!controller || route.mode === 'edit') return false;\n        const pending = flushPublicDraft(controller);\n        await Promise.resolve();\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        const saved = await pending;\n        if (!current()) {\n          retire();\n          return false;\n        }\n        notify();\n        return saved;\n      },\n      subscribe(listener) {\n        if (!current()) return () => {};\n        const key = Symbol();\n        listeners.set(key, listener);\n        return () => {\n          listeners.delete(key);\n        };\n      }\n    };\n    const editor = Object.freeze({}) as ListingEditor;\n    editors.set(editor, value);\n    return { status: 'ready', editor };\n  } catch {\n    return unavailable('storage');\n  }\n}\nexport function listingEditorSnapshot(\n  editor: ListingEditor\n): ListingEditorSnapshot | undefined {\n  return editors.get(editor)?.snapshot();\n}\nexport function editListingEditor(\n  editor: ListingEditor,\n  form: unknown\n): Promise<boolean> {\n  return editors.get(editor)?.edit(form) ?? Promise.resolve(false);\n}\nexport function flushListingEditor(editor: ListingEditor): Promise<boolean> {\n  return editors.get(editor)?.flush() ?? Promise.resolve(false);\n}\nexport function closeListingEditor(editor: ListingEditor): void {\n  editors.get(editor)?.retire();\n}\nexport function subscribeListingEditor(\n  editor: ListingEditor,\n  listener: () => void\n): () => void {\n  return editors.get(editor)?.subscribe(listener) ?? (() => {});\n}\n",
            sha256:
              'f2f1be3c614c861f50293f33f5bf1d3ae1851f4b4452fb9e2f3a7b8963a1d1f9'
          }
        };
        for (const [name, value] of Object.entries(historical)) {
          assert.equal(
            createHash('sha256').update(value.source).digest('hex'),
            value.sha256
          );
          await writeFile(path.join(f.web, name), value.source);
        }
        await copyDependencies(t, base, f.web);
        await refresh(f);
        compile(t, f.web);
        const manifest = JSON.parse(
          await readFile(
            path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
            'utf8'
          )
        );
        assert.equal(Object.keys(manifest).length, 43);
        assert.ok(
          Object.values(manifest).some((row) => row.name === 'view-context')
        );
        assert.ok((await f.audit()).length > 0);
        const handles = Object.values(manifest).find(
          (row) => row.name === 'private-handles'
        );
        assert.ok(
          handles,
          'Historical HCP115 keeps its exact private-handles compiler role'
        );
        handles.imports = ['forged-compiler-edge'];
        await writeFile(
          path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
          JSON.stringify(manifest)
        );
        await assert.rejects(f.audit, /compiler/);
      }
    );
    await test(
      'public-only owned lifecycle remains independently compilable',
      { timeout: 1800000 },
      async (t) => {
        const f = await fixture(t);
        await restoreHistoricalPublicProducers(f.web);
        for (const [name, value] of Object.entries(historicalPublicOnly)) {
          assert.equal(
            createHash('sha256').update(value.source).digest('hex'),
            value.sha256
          );
          await writeFile(path.join(f.web, name), value.source);
        }
        for (const [name, value] of Object.entries(historicalPublicProduct)) {
          assert.equal(
            createHash('sha256').update(value.source).digest('hex'),
            value.sha256
          );
          await writeFile(path.join(f.web, name), value.source);
        }
        await copyDependencies(t, base, f.web);
        await refresh(f);
        compile(t, f.web);
        const manifest = JSON.parse(
          await readFile(
            path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
            'utf8'
          )
        );
        assert.ok(
          !Object.values(manifest).some(
            (record) =>
              record.name === 'view-context' || record.name === 'CapabilityGate'
          )
        );
        assert.ok(
          Object.values(manifest).some((record) => record.name === 'budgets')
        );
        assert.equal((await f.audit()).length, 47);
        const manifestFile = path.join(
          f.web,
          '.svelte-kit/output/client/.vite/manifest.json'
        );
        const originalManifest = await readFile(manifestFile);
        for (const mutation of ['shared-edge', 'button-import']) {
          await t.test(
            `public-only listing compatibility rejects extra ${mutation}`,
            { timeout: 1800000 },
            async () => {
              const changed = JSON.parse(originalManifest.toString());
              const records = Object.entries(changed);
              const additional = records.find(
                ([, record]) => record.name === 'entry/payload'
              );
              const target = records.find(
                ([, record]) =>
                  record.name ===
                  (mutation === 'shared-edge' ? 'Disclosure' : 'Button')
              );
              assert.ok(additional);
              assert.ok(target);
              target[1].imports.push(additional[0]);
              try {
                await writeFile(manifestFile, JSON.stringify(changed));
                await assert.rejects(
                  f.audit(),
                  mutation === 'shared-edge'
                    ? /Invalid owned shared compiler dependencies/
                    : /Invalid owned public-only Button compiler dependencies/
                );
              } finally {
                await writeFile(manifestFile, originalManifest);
              }
            }
          );
        }
      }
    );
    await test(
      'predecessor SDK graph remains qualified with the historical search form fixture',
      { timeout: 1800000 },
      async (t) => {
        const f = await fixture(t);
        await restoreHistoricalPublicProducers(f.web);
        // This historical graph predates identity activation. Restore its
        // immutable public-only producers before compiling the former routes.
        for (const [name, value] of Object.entries(historicalPublicOnly))
          await writeFile(path.join(f.web, name), value.source);
        // Immutable former route bytes are a labelled verification variant only.
        const historical =
          "<script lang=\"ts\">\n  import { page } from '$app/state';\n  import SearchForm from '../../lib/components/SearchForm.svelte';\n  import {\n    normalizePublicQuery,\n    queryErrorMessage,\n    readPublicQuery\n  } from '../../lib/catalog/query-input';\n  import type { PublicQuery } from '../../lib/catalog/query-input';\n  import { searchHref } from '../../lib/routes';\n  import { internalHref } from '../../lib/navigation-url';\n  let query = $state<PublicQuery>(normalizePublicQuery(''));\n  let error = $state<string | undefined>();\n  $effect(() => {\n    query = readPublicQuery(page.url);\n    error = undefined;\n  });\n  function search(input: string) {\n    const next = normalizePublicQuery(input);\n    if (!next.ok) {\n      error = queryErrorMessage(next.error);\n      return;\n    }\n    error = undefined;\n    const target = internalHref(searchHref(next.text));\n    if (target === undefined) {\n      error = queryErrorMessage('invalid_query');\n      return;\n    }\n    globalThis.location.assign(target);\n  }\n</script>\n\n<svelte:head><title>Search food \u2014 HarvestCircle</title></svelte:head>\n<div class=\"page page--reading stack\">\n  <h1>Search food</h1>\n  <SearchForm\n    onsubmit={search}\n    initialValue={query.ok ? query.text : ''}\n    error={error ?? (query.ok ? undefined : queryErrorMessage(query.error))}\n  />\n  <p class=\"notice\">Search data is unavailable during development.</p>\n</div>\n";
        await writeFile(
          path.join(f.web, 'src/routes/search/+page.svelte'),
          historical
        );
        await writeFile(
          path.join(f.web, 'src/routes/products/[naddr=naddr]/+page.svelte'),
          historicalProduct
        );
        await refresh(f);
        await copyDependencies(t, base, f.web);
        compile(t, f.web);
        const manifest = JSON.parse(
          await readFile(
            path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
            'utf8'
          )
        );
        assert.ok(
          Object.values(manifest).some((record) => record.name === 'dist')
        );
        assert.ok(
          !Object.values(manifest).some(
            (record) =>
              record.name === 'Disclosure' || record.name === 'navigation'
          )
        );
        assert.equal((await f.audit()).length, 45);
      }
    );
    await test(
      'HCP043 search graph remains qualified with the exact historical product shell',
      { timeout: 1800000 },
      async (t) => {
        const f = await fixture(t);
        await restoreHistoricalPublicProducers(f.web);
        for (const [name, value] of Object.entries(historicalPublicOnly))
          await writeFile(path.join(f.web, name), value.source);
        await writeFile(
          path.join(f.web, 'src/routes/products/[naddr=naddr]/+page.svelte'),
          historicalProduct
        );
        await refresh(f);
        await copyDependencies(t, base, f.web);
        compile(t, f.web);
        assert.equal((await f.audit()).length, 46);
      }
    );
    await test(
      'actual output qualification',
      { timeout: 1800000 },
      async (t) => {
        /** @param {string} name @param {(context: import('node:test').TestContext) => Promise<void>} run @param {number} [timeout] */
        const check = (name, run, timeout = 1800000) =>
          t.test(name, { timeout }, run);
        await check(
          'actual compiled payload passes and build dispatch invokes the guard last',
          async (t) => {
            const f = await fixture(t);
            const files = await f.audit();
            // Exact eleven-route module/page/static admission; CSS stays once-imported.
            assert.equal(files.length, 53);
            // The actual SDK payload crosses the reader scratch boundary; a reused
            // scratch buffer must still preserve every compiler/static byte exactly.
            const sizes = await Promise.all(
              files
                .filter((name) => name.endsWith('.js'))
                .map(
                  async (name) =>
                    (await lstat(path.join(f.web, 'build', name))).size
                )
            );
            assert.ok(sizes.some((bytes) => bytes > 64 * 1024));
            assert.ok(files.includes('search.html'));
            assert.equal(
              files.filter((name) => name.endsWith('.css')).length,
              1
            );
            assert.ok(
              files.some((name) =>
                /^_app\/immutable\/assets\/[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.css$/.test(
                  name
                )
              )
            );
            assert.ok(files.includes('200.html'));
            const pkg = JSON.parse(
              await readFile(path.join(f.web, 'package.json'), 'utf8')
            );
            assert.match(
              pkg.scripts.build,
              /vite build && node tools\/check-output\.mjs$/
            );
          }
        );

        for (const sourceName of [
          'src/routes/messages/+page.svelte',
          'src/lib/components/InboxStatus.svelte',
          'src/lib/messaging/inbox-view.ts',
          'src/lib/messaging/inbox-sync.ts',
          'src/lib/messaging/decryption-queue.ts',
          'src/lib/components/ConversationRow.svelte',
          'src/lib/messaging/inbox-list.ts',
          'src/lib/components/ConversationThread.svelte',
          'src/lib/components/MessageItem.svelte',
          'src/lib/components/ConversationOptions.svelte',
          'src/lib/messaging/cited-product.ts',
          'src/lib/nostr/enquiry-references.ts',
          'src/lib/components/MessageComposer.svelte',
          'src/lib/messaging/reply-controller.ts',
          'src/lib/messaging/rumor-plan.ts',
          'src/lib/messaging/conversation-thread.ts',
          'src/lib/persistence/private-storage.ts',
          'src/routes/messages/[conversationId=local_id]/+page.svelte'
        ])
          await check(
            'linked inbox presentation requires exact owned source ' +
              sourceName,
            async (t) => {
              const f = await fixture(t),
                file = path.join(f.web, sourceName);
              await writeFile(
                file,
                (await readFile(file, 'utf8')) +
                  '\n// changed presentation source\n'
              );
              // Match the actual altered source identity before probing the
              // independent exact producer pin. The stale identity guard stays.
              await refresh(f);
              await assert.rejects(f.audit, /inbox presentation source/);
            }
          );
        for (const mutation of [
          'missing-private-cache-directory',
          'extra-private-edge',
          'missing-private-edge',
          'private-dynamic',
          'publisher-extra-edge'
        ])
          await check(
            'actual inbox compiler rejects ' + mutation,
            async (t) => {
              const f = await fixture(t),
                file = path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const find = (/** @type {string} */ name) => {
                const row = Object.entries(manifest).find(
                  ([, v]) => v.name === name
                );
                assert.ok(row);
                return row;
              };
              const [key, row] = find('InboxStatus');
              if (mutation === 'missing-private-cache-directory')
                delete manifest[key];
              if (mutation === 'extra-private-edge')
                row.imports = row.imports.concat(find('references')[0]);
              if (mutation === 'missing-private-edge')
                row.imports = row.imports.slice(1);
              if (mutation === 'private-dynamic')
                row.dynamicImports = [find('InboxSetup')[0]];
              if (mutation === 'publisher-extra-edge')
                find('publishers')[1].imports = find(
                  'publishers'
                )[1].imports.concat(find('references')[0]);
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler|references/);
            }
          );

        for (const mutation of [
          'directory-missing-context',
          'directory-entry-role',
          'directory-duplicate-role',
          'thread-missing-edge',
          'thread-dynamic',
          'thread-entry-role',
          'reply-missing-navigation',
          'reply-missing-owner-key',
          'context-resolver-dynamic',
          'context-resolver-entry',
          'enquiry-view-dynamic',
          'enquiry-view-entry',
          'enquiry-setup-dynamic',
          'enquiry-setup-entry',
          'enquiry-gate-dynamic',
          'enquiry-gate-entry',
          'enquiry-root-missing-gate',
          'enquiry-root-missing-css',
          'enquiry-root-extra-css'
        ])
          await check(
            'actual inbox list compiler rejects ' + mutation,
            async (t) => {
              const f = await fixture(t),
                file = path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                ),
                manifest = JSON.parse(await readFile(file, 'utf8'));
              const entry = Object.entries(manifest).find(
                ([, row]) =>
                  row.name ===
                  (mutation.startsWith('enquiry-view-')
                    ? 'inbox-view'
                    : mutation.startsWith('enquiry-setup-')
                      ? 'InboxSetup'
                      : mutation.startsWith('enquiry-gate-')
                        ? 'InboxSetup'
                        : mutation.startsWith('enquiry-root-')
                          ? 'nodes/0'
                          : mutation.startsWith('context-')
                            ? 'publishers'
                            : mutation.startsWith('reply-')
                              ? 'MessageComposer'
                              : mutation.startsWith('thread-')
                                ? 'nodes/5'
                                : 'InboxStatus')
              );
              assert.ok(entry);
              const [, row] = entry;
              if (mutation === 'enquiry-root-missing-css') row.css = [];
              if (mutation === 'enquiry-root-extra-css')
                row.css = [...row.css, row.css[0]];
              if (
                mutation.startsWith('enquiry-') &&
                mutation.endsWith('-dynamic')
              )
                row.dynamicImports = [];
              if (
                mutation.startsWith('enquiry-') &&
                mutation.endsWith('-entry')
              )
                row.isEntry = true;
              if (mutation === 'enquiry-root-missing-gate')
                row.imports = row.imports.filter(
                  (/** @type {string} */ value) =>
                    !['view-context', 'CapabilityGate'].includes(
                      manifest[value].name
                    )
                );
              if (mutation === 'context-resolver-dynamic')
                row.dynamicImports = [];
              if (mutation === 'context-resolver-entry') row.isEntry = true;
              if (mutation === 'directory-missing-context')
                row.imports = row.imports.filter(
                  (/** @type {string} */ value) =>
                    manifest[value].name !== 'InboxSetup'
                );
              if (mutation === 'directory-entry-role') row.isEntry = true;
              if (mutation === 'directory-duplicate-role')
                manifest['_forged_directory.js'] = { ...row };
              if (mutation === 'thread-missing-edge')
                row.imports = row.imports.slice(1);
              if (mutation === 'thread-dynamic')
                row.dynamicImports = [
                  Object.keys(manifest).find(
                    (key) => manifest[key].name === 'InboxSetup'
                  )
                ];
              if (mutation === 'thread-entry-role') row.isEntry = false;
              if (mutation === 'reply-missing-navigation')
                row.imports = row.imports.filter(
                  (/** @type {string} */ value) =>
                    manifest[value].name !== 'navigation'
                );
              if (mutation === 'reply-missing-owner-key') {
                const key = Object.keys(manifest).find(
                  (key) => manifest[key].name === 'public-runtime'
                );
                assert.ok(
                  key && row.imports.includes(key),
                  'Mutate an actual current owner-key compiler edge'
                );
                row.imports = row.imports.filter(
                  (/** @type {string} */ value) =>
                    manifest[value].name !== 'public-runtime'
                );
              }
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler|duplicate|inventory/);
            }
          );

        for (const name of [
          'components/InboxSetup.svelte',
          'messaging/inbox-setup-view.ts',
          'runtime/view-context.ts'
        ])
          await check(
            'linked inbox setup requires exact owned source ' + name,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(f.web, 'src/lib', name);
              await writeFile(
                file,
                (await readFile(file, 'utf8')) + '\n// changed source\n'
              );
              await refresh(f);
              await assert.rejects(f.audit, /inbox compiler source/);
            }
          );
        for (const name of [
          'references',
          'public-runtime',
          'AccountGate',
          'Disclosure'
        ])
          await check(
            'linked inbox setup rejects changed exact compiler edge ' + name,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(
                f.web,
                '.svelte-kit/output/client/.vite/manifest.json'
              );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const entry = Object.entries(manifest).find(
                ([, row]) => row.name === name
              );
              assert.ok(entry, 'Missing actual linked inbox compiler role');
              entry[1].imports = ['forged-compiler-edge'];
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler/);
            }
          );
        await check(
          'owned root preserves its lifecycle across a 64 KiB read boundary',
          async (t) => {
            const f = await fixture(t);
            const file = path.join(f.web, 'src/routes/+layout.svelte');
            const layout = await readFile(file, 'utf8');
            // A large harmless comment after the real lifecycle must not overwrite its
            // first read chunk when the file reader reuses a scratch buffer.
            const expanded = layout.replace(
              '</script>',
              '/*' + 'owned reader boundary '.repeat(4000) + '*/\n</script>'
            );
            assert.ok(Buffer.byteLength(expanded) > 64 * 1024);
            await writeFile(file, expanded);
            await refresh(f);
            assert.equal((await f.audit()).length, 53);
          }
        );

        for (const mutation of [
          'unknown-name',
          'missing-references',
          'missing-dist',
          'missing-negentropy',
          'missing-root-edge',
          'random-key',
          'wrong-src',
          'wrong-version',
          'wrong-peer',
          'wrong-basename',
          'wrong-dynamic-flag',
          'entry-flag',
          'css',
          'wrong-import',
          'chunk-src',
          'chunk-entry'
        ])
          await check(
            'owned root SDK compiler admission rejects ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(
                f.web,
                '.svelte-kit/output/client/.vite/manifest.json'
              );
              const m = JSON.parse(await readFile(file, 'utf8'));
              const root = Object.values(m).find((v) => v.name === 'nodes/0');
              const dist = Object.entries(m).find(
                ([, v]) =>
                  v.name ===
                  (Object.values(m).some((row) => row.name === 'public-runtime')
                    ? 'public-runtime'
                    : 'Disclosure')
              );
              assert.ok(
                m[sdkKey] && root && dist,
                'Actual successor SDK identities'
              );
              if (mutation === 'unknown-name')
                m[sdkKey].name = 'unapproved-sdk';
              if (
                mutation.startsWith('missing-') &&
                mutation !== 'missing-root-edge'
              ) {
                const entry = Object.entries(m).find(
                  ([, v]) =>
                    v.name ===
                    (mutation === 'missing-dist'
                      ? dist[1].name
                      : mutation.slice(8))
                );
                assert.ok(entry);
                delete m[entry[0]];
              }
              if (mutation === 'missing-root-edge') dist[1].dynamicImports = [];
              if (
                [
                  'random-key',
                  'wrong-version',
                  'wrong-peer',
                  'wrong-basename'
                ].includes(mutation)
              ) {
                const next =
                  mutation === 'random-key'
                    ? '_HCrandom.js'
                    : sdkKey.replace(
                        mutation === 'wrong-version'
                          ? '6.2.1'
                          : mutation === 'wrong-peer'
                            ? '6.0.3'
                            : 'negentropy.js',
                        mutation === 'wrong-basename' ? 'other.js' : '0.0.0'
                      );
                m[next] = m[sdkKey];
                delete m[sdkKey];
                m[next].src = next;
                dist[1].dynamicImports = dist[1].dynamicImports.map(
                  (/** @type {string} */ k) => (k === sdkKey ? next : k)
                );
              }
              if (mutation === 'wrong-src') m[sdkKey].src = '_HCwrong.js';
              if (mutation === 'wrong-dynamic-flag')
                m[sdkKey].isDynamicEntry = false;
              if (mutation === 'entry-flag') m[sdkKey].isEntry = false;
              if (mutation === 'css') m[sdkKey].css = [];
              if (mutation === 'wrong-import')
                m[sdkKey].imports = [
                  Object.keys(m).find((k) => m[k].name === 'Button')
                ];
              if (mutation === 'chunk-src') dist[1].src = dist[0];
              if (mutation === 'chunk-entry') dist[1].isEntry = false;
              await writeFile(file, JSON.stringify(m));
              await assert.rejects(f.audit, /compiler|runtime/i);
            }
          );
        for (const mutation of [
          'copied-files-only',
          'comment-only',
          'string-only',
          'shadowed',
          'rebound',
          'aliased',
          'missing-cleanup',
          'missing-context',
          'wrong-context'
        ])
          await check(
            'root runtime activation rejects ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(f.web, 'src/routes/+layout.svelte');
              let layout = await readFile(file, 'utf8');
              if (
                ['copied-files-only', 'comment-only', 'string-only'].includes(
                  mutation
                )
              ) {
                const script = layout.slice(
                  layout.indexOf('>') + 1,
                  layout.indexOf('</script>')
                );
                layout =
                  '<script lang="ts">' +
                  (mutation === 'comment-only'
                    ? '/*' + script + '*/'
                    : mutation === 'string-only'
                      ? 'const text = ' + JSON.stringify(script) + ';'
                      : '') +
                  '</script><main>Controlled root</main>';
              }
              if (mutation === 'shadowed')
                layout = layout.replace(
                  'onMount(() => {',
                  'onMount((publicContext) => {'
                );
              if (mutation === 'rebound')
                layout = layout.replace(
                  'const publicContext',
                  'let publicContext'
                );
              if (mutation === 'aliased')
                layout = layout
                  .replace(
                    '    mountPublicRuntime,',
                    '    mountPublicRuntime as otherMount,'
                  )
                  .replace(
                    'mountPublicRuntime(publicContext)',
                    'otherMount(publicContext)'
                  );
              if (mutation === 'missing-cleanup')
                layout = layout.replace(
                  'return () => closePublicRuntime(publicContext);',
                  'return () => {};'
                );
              if (mutation === 'missing-context')
                layout = layout.replace(
                  'setContext(PUBLIC_RUNTIME_CONTEXT, publicContext);',
                  ''
                );
              if (mutation === 'wrong-context')
                layout = layout.replace(
                  'closePublicRuntime(publicContext)',
                  'closePublicRuntime({})'
                );
              await writeFile(file, layout);
              await refresh(f);
              await assert.rejects(f.audit, /compiler|runtime/i);
            }
          );
        for (const mutation of [
          'missing-mount',
          'missing-subscription',
          'missing-unsubscribe',
          'missing-close',
          'wrong-context',
          'duplicate-context',
          'extra-mount',
          'aliased-mount',
          'shadowed',
          'missing-context',
          'rebound-context'
        ])
          await check(
            'owned identity lifecycle rejects ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(f.web, 'src/routes/+layout.svelte');
              const original = await readFile(file, 'utf8');
              let layout = original;
              assert.ok(layout.includes('mountIdentityView(identityContext);'));
              if (mutation === 'missing-mount')
                layout = layout.replace(
                  'mountIdentityView(identityContext);',
                  ''
                );
              if (mutation === 'missing-subscription')
                layout = layout.replace(
                  'subscribeIdentityView(identityContext,',
                  'subscribeIdentityView({},'
                );
              if (mutation === 'missing-unsubscribe')
                layout = layout.replace('      off();', '');
              if (mutation === 'missing-close')
                layout = layout.replace(
                  'closeIdentityView(identityContext);',
                  ''
                );
              if (mutation === 'wrong-context')
                layout = layout.replace(
                  'closeIdentityView(identityContext)',
                  'closeIdentityView({})'
                );
              if (mutation === 'duplicate-context')
                layout = layout.replace(
                  'const identityContext = createIdentityViewContext();',
                  'const identityContext = createIdentityViewContext(); const other = createIdentityViewContext();'
                );
              if (mutation === 'extra-mount')
                layout = layout.replace(
                  'mountIdentityView(identityContext);',
                  'mountIdentityView(identityContext); mountIdentityView(identityContext);'
                );
              if (mutation === 'aliased-mount')
                layout = layout
                  .replace(
                    '    mountIdentityView,',
                    '    mountIdentityView as otherMount,'
                  )
                  .replace(
                    'mountIdentityView(identityContext);',
                    'otherMount(identityContext);'
                  );
              if (mutation === 'shadowed')
                layout = layout.replace(
                  'const off = subscribeIdentityView',
                  'const identityContext = {}; const off = subscribeIdentityView'
                );
              if (mutation === 'missing-context')
                layout = layout.replace(
                  'setContext(IDENTITY_VIEW_CONTEXT, identityContext);',
                  ''
                );
              if (mutation === 'rebound-context')
                layout = layout.replace(
                  'const identityContext =',
                  'let identityContext ='
                );
              assert.notEqual(layout, original);
              await writeFile(file, layout);
              await refresh(f);
              await assert.rejects(
                f.audit,
                /Invalid owned root runtime activation/
              );
            }
          );
        for (const mutation of [
          'missing-view',
          'missing-heads',
          'extra-view-field',
          'wrong-view-imports',
          'wrong-heads-imports',
          'missing-root-edge',
          'missing-gate-edge'
        ])
          await check(
            'owned identity compiler topology rejects ' + mutation,
            async (t) => {
              const f = await fixture(t),
                file = path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              /** @param {string} name */
              const find = (name) => {
                const entry = Object.entries(manifest).find(
                  ([, record]) => record.name === name
                );
                assert.ok(entry, 'Missing observed compiler role: ' + name);
                return entry;
              };
              const identityCompilerRole = Object.values(manifest).some(
                (record) => record.name === 'identity-session'
              )
                ? 'identity-session'
                : Object.values(manifest).some(
                      (record) => record.name === 'public-runtime'
                    )
                  ? 'public-runtime'
                  : 'heads';
              const [identityKey, identity] = find(
                  Object.values(manifest).some(
                    (record) => record.name === 'view-context'
                  )
                    ? 'view-context'
                    : 'CapabilityGate'
                ),
                [headsKey, heads] = find(identityCompilerRole);
              if (mutation === 'missing-view') delete manifest[identityKey];
              if (mutation === 'missing-heads') delete manifest[headsKey];
              if (mutation === 'extra-view-field') identity.dynamicImports = [];
              if (mutation === 'wrong-view-imports') identity.imports = [];
              if (mutation === 'wrong-heads-imports') heads.imports = [];
              if (
                mutation === 'missing-root-edge' ||
                mutation === 'missing-gate-edge'
              ) {
                const [, consumer] = find(
                  mutation === 'missing-root-edge' ? 'nodes/0' : 'AccountGate'
                );
                consumer.imports = consumer.imports.filter(
                  /** @param {string} dependency */
                  (dependency) => dependency !== identityKey
                );
              }
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler/i);
            }
          );
        for (const name of [
          'runtime/public-runtime.ts',
          'nostr/request-scope.ts',
          'nostr/public-pool.ts',
          'nostr/public-store.ts',
          'nostr/request-result.ts',
          'nostr/ingress.ts',
          'nostr/verified-envelope.ts',
          'nostr/envelope-bounds.ts',
          'nostr/exports.ts',
          'catalog/observations.ts',
          'config/budgets.ts',
          'config/relays.ts',
          'config/deployment-relays.ts'
        ])
          await check(
            'root runtime admission requires owned ' + name,
            async (t) => {
              const f = await fixture(t);
              await rm(path.join(f.web, 'src/lib', name));
              await refresh(f);
              await assert.rejects(f.audit);
            }
          );
        for (const routeSource of [
          'search/+page.svelte',
          'messages/+page.svelte'
        ])
          await check(
            routeSource +
              ' admission requires its owned source and exact module/page counterparts',
            async (t) => {
              const f = await fixture(t);
              const source = path.join(f.web, 'src/routes', routeSource);
              const original = await readFile(source);
              await rm(source);
              git(f.root, 'add', '--', 'web/src/routes/' + routeSource);
              await generateBuildInfo(f.web);
              const metadata = await readFile(
                path.join(f.web, 'static/build-info.json')
              );
              await writeFile(
                path.join(f.web, 'build/build-info.json'),
                metadata
              );
              await writeFile(
                path.join(f.web, '.svelte-kit/output/client/build-info.json'),
                metadata
              );
              await assert.rejects(f.audit, /Undeclared compiler module/);
              await writeFile(source, original);
            }
          );
        for (const mutation of [
          'unknown-name',
          'duplicate',
          'missing-module',
          'missing-page',
          'changed-page'
        ]) {
          await check(
            'exact search compiler admission rejects ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(
                f.web,
                '.svelte-kit/output/client/.vite/manifest.json'
              );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const entry = Object.entries(manifest).find(
                ([, record]) => record.name === 'routes'
              );
              assert.ok(entry, 'The actual shared routes module must exist');
              const [key, module] = entry;
              if (mutation === 'unknown-name')
                module.name = 'unapproved-routes';
              if (mutation === 'duplicate') manifest.duplicate = { ...module };
              if (mutation === 'missing-module') delete manifest[key];
              if (mutation === 'missing-page')
                await rm(path.join(f.web, 'build/search.html'));
              if (mutation === 'changed-page')
                await writeFile(
                  path.join(f.web, 'build/search.html'),
                  '<h1>Different source</h1>'
                );
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit);
            }
          );
        }
        for (const mutation of ['missing', 'changed']) {
          await check(
            'full-route static robots rejects ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const target = path.join(f.web, 'build/robots.txt');
              if (mutation === 'missing') await rm(target);
              else await writeFile(target, 'User-agent: *\nAllow: /\n');
              await assert.rejects(f.audit);
            }
          );
        }
        await check(
          'full-route compiler rejects an unowned thirteenth node',
          async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const manifest = JSON.parse(await readFile(file, 'utf8'));
            const entry = Object.entries(manifest).find(
              ([, record]) => record.name === 'nodes/12'
            );
            assert.ok(entry);
            entry[1].name = 'nodes/13';
            await writeFile(file, JSON.stringify(manifest));
            await assert.rejects(f.audit, /Undeclared compiler module/);
          }
        );
        await check(
          'actual shared budgets chunk has owned consumers and counterpart',
          async (t) => {
            const f = await fixture(t);
            const manifest = JSON.parse(
              await readFile(
                path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                ),
                'utf8'
              )
            );
            const entries = Object.entries(manifest).filter(
              ([, record]) => record.name === 'Button'
            );
            assert.equal(entries.length, 1);
            const [key, budget] = entries[0];
            assert.deepEqual(Object.keys(budget).sort(), [
              'file',
              'imports',
              'name'
            ]);
            const client = Object.entries(manifest).find(
              ([, record]) => record.name === 'client'
            );
            assert.ok(client);
            assert.deepEqual(budget.imports, [client[0]]);
            for (const name of [
              Object.values(manifest).some(
                (row) => row.name === 'public-runtime'
              )
                ? 'public-runtime'
                : 'Disclosure',
              'nodes/9',
              'routes'
            ])
              assert.ok(
                Object.values(manifest)
                  .find((record) => record.name === name)
                  .imports.includes(key)
              );
            assert.deepEqual(
              await readFile(path.join(f.web, 'build', budget.file)),
              await readFile(
                path.join(f.web, '.svelte-kit/output/client', budget.file)
              )
            );
            assert.ok((await f.audit()).includes(budget.file));
          }
        );
        for (const name of [
          'ListingEditor',
          'public-runtime',
          'nodes/8',
          'nodes/10',
          'nodes/12'
        ]) {
          for (const mutation of [
            'missing',
            'missing-edge',
            'extra-edge',
            'duplicate-edge',
            'permuted-edges',
            'dynamic',
            'entry',
            'css'
          ]) {
            await check(
              'exact listing editor compiler rejects ' + name + ' ' + mutation,
              async (t) => {
                const f = await fixture(t),
                  file = path.join(
                    f.web,
                    '.svelte-kit/output/client/.vite/manifest.json'
                  );
                const manifest = JSON.parse(await readFile(file, 'utf8'));
                const entry = Object.entries(manifest).find(
                  ([, row]) => row.name === name
                );
                assert.ok(entry);
                const [key, row] = entry;
                if (mutation === 'missing') delete manifest[key];
                if (mutation === 'missing-edge')
                  row.imports = row.imports.slice(1);
                if (mutation === 'extra-edge')
                  row.imports = row.imports.concat(
                    Object.keys(manifest).find(
                      (key) => manifest[key].name === 'AccountGate'
                    )
                  );
                if (mutation === 'duplicate-edge')
                  row.imports = row.imports.concat(row.imports[0]);
                if (mutation === 'permuted-edges')
                  row.imports = [...row.imports].reverse();
                if (mutation === 'dynamic')
                  row.dynamicImports = [row.imports[0]];
                if (mutation === 'entry') {
                  if (row.isEntry === true) delete row.isEntry;
                  else row.isEntry = true;
                }
                if (mutation === 'css') row.css = [];
                await writeFile(file, JSON.stringify(manifest));
                await assert.rejects(f.audit, /compiler|references|import/);
              }
            );
          }
        }
        for (const producer of [
          'src/routes/sell/+page.svelte',
          'src/routes/selling/drafts/[draftId=local_id]/+page.svelte',
          'src/routes/products/[naddr=naddr]/edit/+page.svelte',
          'src/lib/components/ListingEditor.svelte',
          'src/lib/publishing/listing-editor.ts',
          'src/lib/components/ListingReview.svelte',
          'src/lib/publishing/editor-workflow.ts',
          'src/lib/components/ConflictNotice.svelte',
          'src/lib/publishing/edit-source.ts',
          'src/lib/publishing/prepare-listing.ts',
          'src/lib/publishing/new-listing-readiness.ts',
          'src/lib/runtime/public-runtime.ts',
          'src/lib/persistence/public-publications.ts',
          'src/lib/persistence/quota.ts',
          'src/lib/publishing/revise-listing.ts',
          'src/lib/publishing/mark-sold.ts',
          'src/lib/components/ConfirmPanel.svelte',
          'src/lib/nostr/listing-publisher.ts'
        ]) {
          await check(
            'listing editor rejects changed owned producer ' + producer,
            async (t) => {
              const f = await fixture(t);
              await writeFile(
                path.join(f.web, producer),
                (await readFile(path.join(f.web, producer), 'utf8')) +
                  '\n// owned editor source mutation\n'
              );
              await refresh(f);
              await assert.rejects(
                f.audit,
                /Invalid owned listing editor source/
              );
            }
          );
        }
        for (const route of [
          'src/routes/sell/+page.svelte',
          'src/routes/selling/drafts/[draftId=local_id]/+page.svelte',
          'src/routes/products/[naddr=naddr]/edit/+page.svelte'
        ]) {
          await check(
            'listing editor rejects partial three-route activation ' + route,
            async (t) => {
              const f = await fixture(t);
              const historical = historicalPublicOnly[route];
              assert.equal(
                createHash('sha256').update(historical.source).digest('hex'),
                historical.sha256
              );
              await writeFile(path.join(f.web, route), historical.source);
              await refresh(f);
              await assert.rejects(
                f.audit,
                /Invalid owned listing editor activation/
              );
            }
          );
        }
        for (const producer of [
          'src/routes/products/[naddr=naddr]/+page.svelte',
          'src/lib/catalog/product-view.ts',
          'src/lib/components/ProductEnquiry.svelte',
          'src/lib/messaging/product-enquiry.ts',
          'src/lib/messaging/product-request-controller.ts',
          'src/lib/messaging/enquiry-readiness.ts',
          'src/lib/messaging/resolve-inbox.ts',
          'src/lib/components/CapabilityGate.svelte',
          'src/lib/messaging/enquiry-context.ts',
          'src/lib/components/ListingFacts.svelte',
          'src/lib/components/PublisherIdentity.svelte',
          'src/lib/contracts/food-availability-v1/contact-read.ts',
          'src/lib/navigation-copy.ts',
          'src/lib/runtime/public-runtime.ts',
          'src/lib/catalog/publishers.ts',
          'src/lib/catalog/resolve-head.ts'
        ]) {
          await check(
            'real product compiler rejects changed owned producer ' + producer,
            async (t) => {
              const f = await fixture(t);
              await writeFile(
                path.join(f.web, producer),
                (await readFile(path.join(f.web, producer), 'utf8')) +
                  '\n// source mutation\n'
              );
              await refresh(f);
              await assert.rejects(
                f.audit,
                producer === 'src/lib/runtime/public-runtime.ts'
                  ? /Invalid owned (product presentation|listing editor) source/
                  : /Invalid owned product presentation source/
              );
            }
          );
        }
        for (const mutation of [
          'permuted-edges',
          'extra-edge',
          'duplicate-edge'
        ]) {
          await check(
            'product compiler dependency order: ' + mutation,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(
                f.web,
                '.svelte-kit/output/client/.vite/manifest.json'
              );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const find = (/** @type {string} */ name) => {
                const row = Object.entries(manifest).find(
                  ([, v]) => v.name === name
                );
                assert.ok(row);
                return row;
              };
              for (const name of ['nodes/7', 'nodes/9']) {
                const row = find(name)[1];
                row.imports = [...row.imports].reverse();
                if (mutation === 'extra-edge')
                  row.imports = row.imports.concat(find('AccountGate')[0]);
                if (mutation === 'duplicate-edge')
                  row.imports = row.imports.concat(row.imports[0]);
              }
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler|references/);
            }
          );
        }
        for (const mutation of [
          'missing',
          'extra-edge',
          'missing-edge',
          'duplicate-edge',
          'dynamic',
          'entry',
          'css'
        ]) {
          await check(
            'inline request shared composer compiler rejects ' + mutation,
            async (t) => {
              const f = await fixture(t),
                file = path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const entry = Object.entries(manifest).find(
                ([, row]) => row.name === 'MessageComposer'
              );
              assert.ok(entry);
              const [key, row] = entry;
              if (mutation === 'missing') delete manifest[key];
              if (mutation === 'extra-edge')
                row.imports = row.imports.concat(
                  Object.keys(manifest).find(
                    (name) => manifest[name].name === 'AccountGate'
                  )
                );
              if (mutation === 'missing-edge')
                row.imports = row.imports.slice(1);
              if (mutation === 'duplicate-edge')
                row.imports = row.imports.concat(row.imports[0]);
              if (mutation === 'dynamic') row.dynamicImports = [];
              if (mutation === 'entry') row.isEntry = true;
              if (mutation === 'css') row.css = [];
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler|references/);
            }
          );
        }
        for (const mutation of [
          'missing-publishers',
          'publisher-budget',
          'publisher-shared',
          'publisher-dynamic',
          'product-shared',
          'product-publisher',
          'product-budget',
          'product-dynamic',
          'search-publisher'
        ]) {
          await check(
            'real product compiler rejects ' + mutation,
            async (t) => {
              const f = await fixture(t),
                file = path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                );
              const manifest = JSON.parse(await readFile(file, 'utf8'));
              const records = Object.entries(manifest);
              const find = (/** @type {string} */ name) => {
                const row = records.find(([, v]) => v.name === name);
                assert.ok(row);
                return row;
              };
              const [publisherKey, publisher] = find('publishers');
              if (mutation === 'missing-publishers')
                delete manifest[publisherKey];
              else if (mutation === 'publisher-dynamic')
                publisher.dynamicImports = [find('client-entry')[0]];
              else if (mutation === 'product-dynamic')
                find('nodes/7')[1].dynamicImports = [find('client-entry')[0]];
              else if (
                mutation === 'publisher-budget' ||
                mutation === 'publisher-shared'
              )
                publisher.imports = publisher.imports.filter(
                  (/** @type {string} */ key) =>
                    key !==
                    find(
                      mutation === 'publisher-budget'
                        ? 'Button'
                        : Object.values(manifest).some(
                              (row) => row.name === 'public-runtime'
                            )
                          ? 'public-runtime'
                          : 'Disclosure'
                    )[0]
                );
              else {
                const target =
                  mutation === 'search-publisher'
                    ? 'nodes/9'
                    : mutation === 'product-publisher'
                      ? 'MessageComposer'
                      : 'nodes/7';
                const dependency =
                  mutation === 'product-shared'
                    ? 'Disclosure'
                    : mutation === 'product-budget'
                      ? 'Button'
                      : 'publishers';
                find(target)[1].imports = find(target)[1].imports.filter(
                  (/** @type {string} */ key) => key !== find(dependency)[0]
                );
              }
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit, /compiler|budgets/);
            }
          );
        }
        for (const producer of [
          'src/routes/search/+page.svelte',
          'src/lib/catalog/search-view.ts',
          'src/lib/components/ListingRow.svelte',
          'src/lib/components/SourceStatus.svelte',
          'src/lib/navigation-scroll.ts',
          'src/lib/components/primitives/Disclosure.svelte'
        ]) {
          await check(
            'real search compiler rejects changed owned producer ' + producer,
            async (t) => {
              const f = await fixture(t);
              await writeFile(
                path.join(f.web, producer),
                (await readFile(path.join(f.web, producer), 'utf8')) +
                  '\n// source mutation\n'
              );
              await refresh(f);
              await assert.rejects(
                f.audit,
                /Invalid owned search presentation source/
              );
            }
          );
        }
        for (const mutation of [
          'missing-shared',
          'missing-navigation',
          'root-edge',
          'search-edge',
          'budget-edge',
          'negentropy-edge',
          'sdk-backedge',
          'navigation-edge',
          'navigation-import',
          'navigation-dynamic',
          'shared-import'
        ]) {
          await check('real search compiler rejects ' + mutation, async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const manifest = JSON.parse(await readFile(file, 'utf8'));
            const records = Object.entries(manifest);
            const find = (/** @type {string} */ name) => {
              const entry = records.find(([, v]) => v.name === name);
              assert.ok(entry, 'Missing actual compiler record ' + name);
              return entry;
            };
            const [sharedKey, shared] = find(
              Object.values(manifest).some(
                (row) => row.name === 'public-runtime'
              )
                ? 'public-runtime'
                : 'Disclosure'
            );
            const [budgetKey] = find('Button');
            if (mutation === 'missing-shared') delete manifest[sharedKey];
            else if (mutation === 'missing-navigation')
              delete manifest[find('navigation')[0]];
            else if (mutation === 'root-edge')
              find('nodes/0')[1].imports = find('nodes/0')[1].imports.filter(
                (/** @type {string} */ k) => k !== sharedKey
              );
            else if (mutation === 'search-edge')
              find('nodes/9')[1].imports = find('nodes/9')[1].imports.filter(
                (/** @type {string} */ k) => k !== sharedKey
              );
            else if (mutation === 'budget-edge')
              shared.imports = shared.imports.filter(
                (/** @type {string} */ k) => k !== budgetKey
              );
            else if (mutation === 'sdk-backedge')
              find('negentropy')[1].imports = [];
            else if (mutation === 'navigation-edge')
              find('nodes/9')[1].imports = find('nodes/9')[1].imports.filter(
                (/** @type {string} */ k) => k !== find('navigation')[0]
              );
            else if (mutation === 'navigation-import')
              find('navigation')[1].imports = [find('client')[0]];
            else if (mutation === 'navigation-dynamic')
              find('navigation')[1].dynamicImports = [find('client-entry')[0]];
            else if (mutation === 'shared-import')
              shared.imports = shared.imports.filter(
                (/** @type {string} */ k) => k !== find('references')[0]
              );
            else shared.dynamicImports = [];
            await writeFile(file, JSON.stringify(manifest));
            await assert.rejects(f.audit, /compiler|SDK|budgets/);
          });
        }
        for (const mutation of [
          'unknown-name',
          'duplicate',
          'missing-module',
          'extra-field',
          'missing-root-edge',
          'missing-routes-edge',
          'missing-counterpart',
          'changed-counterpart'
        ]) {
          await check('budgets module fails closed: ' + mutation, async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const manifest = JSON.parse(await readFile(file, 'utf8'));
            const entry = Object.entries(manifest).find(
              ([, record]) => record.name === 'Button'
            );
            assert.ok(entry);
            const [key, budget] = entry;
            if (mutation === 'unknown-name') budget.name = 'unapproved-budgets';
            if (mutation === 'duplicate')
              manifest['_HCduplicate.js'] = { ...budget };
            if (mutation === 'missing-module') delete manifest[key];
            if (mutation === 'extra-field') budget.dynamicImports = [];
            if (
              mutation === 'missing-root-edge' ||
              mutation === 'missing-routes-edge'
            ) {
              const name =
                mutation === 'missing-root-edge' ? 'nodes/9' : 'routes';
              const consumer = Object.values(manifest).find(
                (record) => record.name === name
              );
              consumer.imports = consumer.imports.filter(
                /** @param {string} dependency */
                (dependency) => dependency !== key
              );
            }
            if (mutation === 'missing-counterpart')
              await rm(path.join(f.web, 'build', budget.file));
            if (mutation === 'changed-counterpart')
              await writeFile(
                path.join(f.web, 'build', budget.file),
                'changed counterpart'
              );
            await writeFile(file, JSON.stringify(manifest));
            await assert.rejects(
              f.audit,
              /Undeclared compiler module|Unresolved compiler import|Invalid owned budgets compiler|Invalid owned listing editor compiler graph|Static output inventory mismatch|Static output differs from owned compiler input/
            );
          });
        }
        await check(
          'actual route-state compiler module has an owned client counterpart',
          async (t) => {
            const f = await fixture(t);
            const manifest = JSON.parse(
              await readFile(
                path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                ),
                'utf8'
              )
            );
            const entries = Object.entries(manifest).filter(
              ([, record]) => record.name === 'state'
            );
            assert.equal(entries.length, 1);
            const [key, state] = entries[0];
            assert.match(key, /^_[A-Za-z0-9_-]+\.js$/);
            assert.ok(
              state.imports.some(
                /** @param {string} dependency */
                (dependency) => manifest[dependency].name === 'client.svelte'
              )
            );
            assert.deepEqual(
              await readFile(path.join(f.web, 'build', state.file)),
              await readFile(
                path.join(f.web, '.svelte-kit/output/client', state.file)
              )
            );
            assert.ok((await f.audit()).includes(state.file));
          }
        );
        for (const mutation of [
          'unknown-name',
          'duplicate-state',
          'missing-counterpart',
          'changed-counterpart'
        ]) {
          await check('state module fails closed: ' + mutation, async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const manifest = JSON.parse(await readFile(file, 'utf8'));
            const state = Object.values(manifest).find(
              (record) => record.name === 'state'
            );
            assert.ok(
              state,
              'The actual route-state module must exist before mutation'
            );
            if (mutation === 'unknown-name') state.name = 'state-unapproved';
            if (mutation === 'duplicate-state')
              manifest['_HCduplicate.js'] = { ...state };
            if (mutation === 'missing-counterpart')
              await rm(path.join(f.web, 'build', state.file));
            if (mutation === 'changed-counterpart')
              await writeFile(
                path.join(f.web, 'build', state.file),
                Buffer.concat([
                  await readFile(path.join(f.web, 'build', state.file)),
                  Buffer.from('changed counterpart')
                ])
              );
            await writeFile(file, JSON.stringify(manifest));
            await assert.rejects(
              f.audit,
              mutation.endsWith('name') || mutation === 'duplicate-state'
                ? /Undeclared compiler module/
                : /Static output inventory mismatch|Static output differs from owned compiler input/
            );
          });
        }
        await check(
          'actual shared Button compiler module has an owned client counterpart',
          async (t) => {
            const f = await fixture(t);
            const manifest = JSON.parse(
              await readFile(
                path.join(
                  f.web,
                  '.svelte-kit/output/client/.vite/manifest.json'
                ),
                'utf8'
              )
            );
            const entries = Object.entries(manifest).filter(
              ([, record]) => record.name === 'Button'
            );
            assert.equal(entries.length, 1);
            const [key, button] = entries[0];
            assert.match(key, /^_[A-Za-z0-9_-]+\.js$/);
            assert.ok(
              button.imports.some(
                /** @param {string} dependency */
                (dependency) => manifest[dependency].name === 'client'
              )
            );
            for (const consumer of ['nodes/0', 'nodes/2']) {
              assert.ok(
                Object.values(manifest).some(
                  (record) =>
                    record.name === consumer && record.imports.includes(key)
                )
              );
            }
            assert.deepEqual(
              await readFile(path.join(f.web, 'build', button.file)),
              await readFile(
                path.join(f.web, '.svelte-kit/output/client', button.file)
              )
            );
            assert.ok((await f.audit()).includes(button.file));
          }
        );
        for (const mutation of [
          'unknown-name',
          'duplicate-Button',
          'missing-counterpart',
          'changed-counterpart'
        ]) {
          await check('Button module fails closed: ' + mutation, async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const manifest = JSON.parse(await readFile(file, 'utf8'));
            const state = Object.values(manifest).find(
              (record) => record.name === 'Button'
            );
            assert.ok(
              state,
              'The actual shared Button module must exist before mutation'
            );
            if (mutation === 'unknown-name') state.name = 'Button-unapproved';
            if (mutation === 'duplicate-Button')
              manifest['_HCduplicate.js'] = { ...state };
            if (mutation === 'missing-counterpart')
              await rm(path.join(f.web, 'build', state.file));
            if (mutation === 'changed-counterpart')
              await writeFile(
                path.join(f.web, 'build', state.file),
                Buffer.concat([
                  await readFile(path.join(f.web, 'build', state.file)),
                  Buffer.from('changed counterpart')
                ])
              );
            await writeFile(file, JSON.stringify(manifest));
            await assert.rejects(
              f.audit,
              mutation.endsWith('name') || mutation === 'duplicate-Button'
                ? /Undeclared compiler module/
                : /Static output inventory mismatch|Static output differs from owned compiler input/
            );
          });
        }
        for (const name of [
          'notes.txt',
          '_app/immutable/chunks/innocent.ABC12345.js',
          'docs/parent.html',
          'state.css',
          'source.map',
          'contracts/vector.json'
        ]) {
          await check(
            `undeclared copied output is rejected: ${name}`,
            async (t) => {
              const f = await fixture(t);
              const file = path.join(f.web, 'build', name);
              await mkdir(path.dirname(file), { recursive: true });
              await writeFile(file, 'ordinary public text');
              await assert.rejects(f.audit);
            }
          );
        }
        for (const payload of [
          Buffer.from('SQLite format 3\0'),
          Buffer.from([0x7f, 0x45, 0x4c, 0x46]),
          Buffer.from([0xcf, 0xfa, 0xed, 0xfe]),
          Buffer.from('PK\x03\x04'),
          Buffer.from('MZnative'),
          Buffer.from('generated UniFFI bindings'),
          Buffer.from('HC_TEST_ONLY_SECRET'),
          Buffer.from('nsec1' + 'a'.repeat(60)),
          Buffer.from('-----BEGIN PRIVATE KEY-----'),
          Buffer.from('private_transcript: controlled disclosure'),
          Buffer.from('class TestSigner {}')
        ]) {
          await check(
            `manifest-admitted contamination ${payload.subarray(0, 18).toString('hex')}`,
            async (t) => {
              const f = await fixture(t);
              const manifest = JSON.parse(
                await readFile(
                  path.join(
                    f.web,
                    '.svelte-kit/output/client/.vite/manifest.json'
                  ),
                  'utf8'
                )
              );
              const name = Object.values(manifest).find(
                (entry) => entry.name === 'client'
              ).file;
              // Poison both sides: byte mapping alone does not qualify contents.
              for (const directory of ['build', '.svelte-kit/output/client'])
                await writeFile(path.join(f.web, directory, name), payload);
              await assert.rejects(f.audit, /contamination|binary|text/);
            }
          );
        }
        for (const name of [
          'index.html',
          '200.html',
          '_app/version.json',
          'build-info.json'
        ]) {
          await check(`missing/tampered owned output: ${name}`, async (t) => {
            const f = await fixture(t);
            const file = path.join(f.web, 'build', name);
            const original = await readFile(file);
            await rm(file);
            await assert.rejects(f.audit);
            await writeFile(
              file,
              Buffer.concat([original, Buffer.from('tampered')])
            );
            await assert.rejects(f.audit);
          });
        }
        for (const mutation of [
          'unknown',
          'private',
          'revision',
          'dirty',
          'lock',
          'config',
          'source',
          'generated'
        ]) {
          await check(`provenance fails closed: ${mutation}`, async (t) => {
            const f = await fixture(t);
            const metadata = JSON.parse(
              await readFile(path.join(f.web, 'build/build-info.json'), 'utf8')
            );
            if (mutation === 'unknown') metadata.web_source.unknown = 'extra';
            if (mutation === 'private')
              metadata.private_transcript = 'controlled disclosure';
            if (mutation === 'revision')
              metadata.web_source.revision = 'a'.repeat(40);
            if (mutation === 'dirty')
              metadata.web_source.dirty = !metadata.web_source.dirty;
            if (mutation === 'lock')
              await writeFile(
                path.join(f.web, 'pnpm-lock.yaml'),
                'changed lock'
              );
            if (mutation === 'config')
              await writeFile(
                path.join(f.web, 'vite.config.ts'),
                'changed config'
              );
            if (mutation === 'source')
              await writeFile(
                path.join(f.web, 'src/app.html'),
                'changed source'
              );
            if (mutation === 'generated')
              git(f.root, 'add', '-f', 'web/static/build-info.json');
            if (
              ['unknown', 'private', 'revision', 'dirty'].includes(mutation)
            ) {
              for (const directory of [
                'build',
                'static',
                '.svelte-kit/output/client'
              ])
                await writeFile(
                  path.join(f.web, directory, 'build-info.json'),
                  JSON.stringify(metadata)
                );
            }
            await assert.rejects(f.audit);
          });
        }
        for (const kind of [
          'symlink',
          'hardlink',
          'fifo',
          'directory',
          'oversize',
          'parent'
        ]) {
          await check(`unsafe output refuses ${kind} promptly`, async (t) => {
            const f = await fixture(t);
            const file = path.join(f.web, 'build/index.html');
            await rm(file);
            if (kind === 'symlink') await symlink('../src/app.html', file);
            if (kind === 'hardlink')
              await link(path.join(f.web, 'src/app.html'), file);
            if (kind === 'fifo') execFileSync('mkfifo', [file]);
            if (kind === 'directory') await mkdir(file);
            if (kind === 'oversize')
              await writeFile(file, Buffer.alloc(8 * 1024 * 1024 + 1));
            if (kind === 'parent') {
              await rm(path.join(f.web, 'build'), { recursive: true });
              await symlink(
                path.join(base, 'web/build'),
                path.join(f.web, 'build')
              );
            }
            await assert.rejects(f.audit);
          });
        }
        await check(
          'compiler manifest cannot authorize arbitrary resources, escapes or unresolved imports',
          async (t) => {
            const f = await fixture(t);
            const file = path.join(
              f.web,
              '.svelte-kit/output/client/.vite/manifest.json'
            );
            const original = await readFile(file, 'utf8');
            for (const mutation of [
              'extra',
              'escape',
              'reference',
              'unknown'
            ]) {
              const manifest = JSON.parse(original);
              const record = Object.values(manifest)[0];
              if (mutation === 'extra')
                manifest.extra = {
                  name: 'provider',
                  file: '_app/immutable/chunks/provider.ABC12345.js'
                };
              if (mutation === 'escape') record.file = '../outside.js';
              if (mutation === 'reference') record.imports = ['absent'];
              if (mutation === 'unknown') record.secret = 'private';
              await writeFile(file, JSON.stringify(manifest));
              await assert.rejects(f.audit);
            }
          }
        );

        await check(
          'generated provenance must remain ignored even when freshly regenerated',
          async (t) => {
            const f = await fixture(t);
            await writeFile(path.join(f.web, '.gitignore'), '');
            await generateBuildInfo(f.web);
            await assert.rejects(f.audit, /must be ignored/);
          }
        );
        for (const name of [
          'static/build-info.json',
          '.svelte-kit/output/client/.vite/manifest.json',
          '.svelte-kit/output/prerendered/pages/index.html',
          '.svelte-kit/output/prerendered/pages/search.html',
          'src/routes/search/+page.svelte',
          'static/robots.txt',
          '.svelte-kit/output/prerendered/pages/messages.html'
        ]) {
          for (const kind of ['fifo', 'symlink', 'hardlink']) {
            await check(
              `unsafe compiler/static counterpart ${name}: ${kind}`,
              async (t) => {
                const f = await fixture(t);
                const file = path.join(f.web, name);
                await rm(file);
                if (kind === 'fifo') execFileSync('mkfifo', [file]);
                if (kind === 'symlink')
                  await symlink(path.join(base, 'web/src/app.html'), file);
                if (kind === 'hardlink')
                  await link(path.join(f.web, 'src/app.html'), file);
                await assert.rejects(f.audit);
              }
            );
          }
        }
        await check(
          'empty output directory and excessive depth are rejected',
          async (t) => {
            const f = await fixture(t);
            await mkdir(path.join(f.web, 'build/extra'));
            await assert.rejects(f.audit);
            await rm(path.join(f.web, 'build/extra'), { recursive: true });
            await mkdir(path.join(f.web, 'build/a/b/c/d/e/f/g/h/i'), {
              recursive: true
            });
            await assert.rejects(f.audit);
          }
        );
        await check(
          'forged public output failures do not echo private poisoned bytes',
          async (t) => {
            const f = await fixture(t);
            const marker = 'CONTROLLED_SENSITIVE_DO_NOT_ECHO';
            const file = path.join(f.web, 'static/build-info.json');
            await writeFile(file, marker);
            await assert.rejects(
              f.audit,
              (error) => !String(error).includes(marker)
            );
          }
        );
        await check(
          'output entry and aggregate-byte bounds reject oversized inventories',
          async (t) => {
            const f = await fixture(t);
            for (let i = 0; i < 513; i++)
              await writeFile(path.join(f.web, 'build', `tiny-${i}.js`), 'x');
            // Confine the actual audit's allocation measurement to a fresh child;
            // prior source/output fixture copies must not enter this baseline/peak.
            const measurement = JSON.parse(
              execFileSync(
                process.execPath,
                [
                  '--input-type=module',
                  '-e',
                  `
          const { auditOutput } = await import(process.argv[2]);
          const baseline = process.memoryUsage().arrayBuffers;
          let peak = baseline, rejection;
          const sample = () => { peak = Math.max(peak, process.memoryUsage().arrayBuffers); };
          const timer = setInterval(sample, 1);
          try { await auditOutput(process.argv[1]); }
          catch (error) { rejection = error instanceof Error ? error.message : 'unexpected'; }
          finally { sample(); clearInterval(timer); }
          if (rejection !== 'Unbounded output inventory') throw new Error('Unexpected inventory measurement rejection');
          process.stdout.write(JSON.stringify({ baseline, peak, growth: peak - baseline, rejection }));
        `,
                  f.web,
                  new URL('../../tools/check-output.mjs', import.meta.url).href
                ],
                {
                  cwd: f.web,
                  encoding: 'utf8',
                  timeout: 1800000,
                  maxBuffer: 4096
                }
              )
            );
            assert.deepEqual(Object.keys(measurement).sort(), [
              'baseline',
              'growth',
              'peak',
              'rejection'
            ]);
            assert.equal(measurement.rejection, 'Unbounded output inventory');
            for (const name of ['baseline', 'peak', 'growth'])
              assert.ok(
                Number.isSafeInteger(measurement[name]) &&
                  measurement[name] >= 0
              );
            assert.equal(
              measurement.growth,
              measurement.peak - measurement.baseline
            );
            t.diagnostic(
              'isolated actual inventory buffers: ' +
                JSON.stringify(measurement)
            );
            assert.ok(
              measurement.growth < 64 * 1024 * 1024,
              'Tiny files must not retain a maximum-sized buffer per entry: ' +
                JSON.stringify(measurement)
            );
            for (let i = 0; i < 513; i++)
              await rm(path.join(f.web, 'build', `tiny-${i}.js`));
            for (let i = 0; i < 33; i++)
              await writeFile(
                path.join(f.web, 'build', `large-${i}.js`),
                Buffer.alloc(1024 * 1024, 97)
              );
            await assert.rejects(f.audit, /Unbounded output bytes/);
          }
        );
      }
    );
    await test(
      'actual compiler admits nonempty approved theme/app CSS',
      { timeout: 1800000 },
      async (t) => {
        ready(t);
        const layout = path.join(base, 'web/src/routes/+layout.svelte');
        await writeFile(
          layout,
          (await readFile(layout, 'utf8')).replace(
            '<script lang="ts">',
            "<script lang=\"ts\">\n  import '../theme.css';\n  import '../app.css';"
          )
        );
        await writeFile(
          path.join(base, 'web/src/theme.css'),
          ':root { --hc-accent: #336633; }\n'
        );
        await writeFile(
          path.join(base, 'web/src/app.css'),
          'main { color: var(--hc-accent); }\n'
        );
        await generateBuildInfo(path.join(base, 'web'));
        compile(t, path.join(base, 'web'));
        const files = await auditOutput(await realpath(path.join(base, 'web')));
        assert.ok(files.some((name) => name.endsWith('.css')));
      }
    );
  } finally {
    // Await the complete serial sequence before reclaiming its shared build.
    await rm(base, { recursive: true, force: true });
  }
}
