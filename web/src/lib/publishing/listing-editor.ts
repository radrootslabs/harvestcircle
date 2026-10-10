import {
  exactLocalFields,
  type PublicDraftForm
} from '../contracts/local-records.ts';
import { canonicalLocalId, newLocalId } from '../private-handles.ts';
import type { InboxSetupView } from '../messaging/inbox-setup-view.ts';
import {
  captureNewListingReadiness,
  closeNewListingReadiness,
  type NewListingReadiness
} from './new-listing-readiness.ts';
import {
  reviewNewListing,
  listingReviewSnapshot,
  listingReviewOwnership,
  closeListingReview,
  type ListingReview,
  type ListingReviewResult
} from './prepare-listing.ts';
import { reviewOwnedRevision } from './revise-listing.ts';
import { reviewMarkSold } from './mark-sold.ts';
import { decodeProductReference } from '../nostr/references.ts';
import {
  openOwnedEditSource,
  ownedEditSourceSnapshot,
  authorizeOwnedEditSource,
  closeOwnedEditSource,
  type OwnedEditSource
} from './edit-source.ts';
import type { PublicRuntime } from '../runtime/public-runtime.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  type BrowserDatabase
} from '../persistence/database.ts';
import {
  createPublicPublicationRepository,
  publicPublicationDraftRepository,
  publicPublicationQuotaRepository,
  readPublicListingIntent
} from '../persistence/public-publications.ts';
import {
  inspectPublicStorage,
  publicInventorySnapshot
} from '../persistence/quota.ts';
import { publicRecordSnapshot } from '../persistence/records.ts';
import {
  publicDraftRepositoryOwner,
  type PublicDraftRepository
} from '../persistence/drafts.ts';
import {
  createPublicDraftController,
  publicDraftControllerSnapshot,
  editPublicDraft,
  flushPublicDraft,
  retryPublicDraft,
  saveAndClosePublicDraft,
  closePublicDraftController,
  type PublicDraftController
} from './draft-controller.ts';
import {
  identityViewSession,
  identityViewMatchesOwner,
  type IdentityViewContext
} from '../runtime/view-context.ts';
import {
  identitySessionSnapshot,
  subscribeIdentityInvalidation
} from '../runtime/identity-session.ts';
import {
  readListingOperationView,
  closeListingOperationView,
  type ListingOperationView
} from './listing-operation-view.ts';

declare const editorBrand: unique symbol;
export type ListingEditor = Readonly<{ [editorBrand]: true }>;
export type ListingEditorRoute =
  | Readonly<{ mode: 'new' }>
  | Readonly<{ mode: 'draft'; draftId: string }>
  | Readonly<{ mode: 'edit'; naddr: string }>;
export type ListingEditorSnapshot = Readonly<{
  mode: ListingEditorRoute['mode'];
  owner: string;
  form: PublicDraftForm;
  editable: boolean;
  draftId: string | null;
  state: 'editing' | 'saving' | 'saved' | 'failed' | 'read-only';
  failure: string | null;
  coordinate: string | null;
  sourceEventId: string | null;
  sourcePublishedAtSeconds?: number;
  operations: readonly ListingOperationView[];
}>;
type Editor = Readonly<{
  current(): boolean;
  retire(): void;
  snapshot(): ListingEditorSnapshot | undefined;
  edit(form: unknown): Promise<boolean>;
  flush(): Promise<boolean>;
  saveClose(): Promise<boolean>;
  review(
    setup: InboxSetupView | undefined,
    form: PublicDraftForm,
    status: 'active' | 'sold'
  ): Promise<ListingReviewResult>;
  reviewCurrent(review: ListingReview): boolean;
  source(): OwnedEditSource | undefined;
  observe(): void;
  subscribe(listener: () => void): () => void;
}>;
const editors = new WeakMap<ListingEditor, Editor>();
const active = new WeakMap<
  IdentityViewContext,
  Readonly<{ stamp: symbol; retire(): void }>
>();
const emptyForm: PublicDraftForm = {
  title: '',
  description: '',
  location: '',
  amount: '',
  currency: '',
  unit: '',
  quantity: '',
  contactType: '',
  contactValue: ''
};
function routeSnapshot(value: unknown): ListingEditorRoute | undefined {
  try {
    if (exactLocalFields(value, ['mode']) && value.mode === 'new')
      return { mode: 'new' };
    if (
      exactLocalFields(value, ['mode', 'draftId']) &&
      value.mode === 'draft'
    ) {
      const draftId = canonicalLocalId(value.draftId);
      return draftId ? { mode: 'draft', draftId } : undefined;
    }
    if (exactLocalFields(value, ['mode', 'naddr']) && value.mode === 'edit') {
      const reference = decodeProductReference(value.naddr);
      return reference ? { mode: 'edit', naddr: reference.naddr } : undefined;
    }
  } catch {
    return;
  }
}
// Owner admission precedes database opening and all protected hydration. Edit
// mode presents validated local values; independent head authorization is later.
export async function openListingEditor(
  context: IdentityViewContext,
  input: unknown,
  signal?: AbortSignal,
  runtime?: PublicRuntime
): Promise<
  | Readonly<{ status: 'ready'; editor: ListingEditor }>
  | Readonly<{
      status: 'unavailable';
      reason: 'owner' | 'resource' | 'storage' | 'source';
    }>
> {
  const session = identityViewSession(context),
    route = routeSnapshot(input);
  if (!session || signal?.aborted)
    return { status: 'unavailable', reason: 'owner' };
  const identity = identitySessionSnapshot(session),
    owner = 'publicKey' in identity ? identity.publicKey : undefined;
  if (!owner || !identityViewMatchesOwner(context, owner))
    return { status: 'unavailable', reason: 'owner' };
  if (
    !route ||
    (route.mode === 'edit' &&
      decodeProductReference(route.naddr)?.pubkey !== owner)
  )
    return { status: 'unavailable', reason: 'resource' };
  if (typeof window === 'undefined' || !navigator.locks?.request)
    return { status: 'unavailable', reason: 'storage' };
  active.get(context)?.retire();
  const stamp = Symbol(),
    listeners = new Map<symbol, () => void>(),
    cancellation = new AbortController();
  let alive = true,
    closing = false,
    unsubscribe = () => {},
    database: BrowserDatabase | undefined,
    repository: PublicDraftRepository | undefined,
    controller: PublicDraftController | undefined;
  const operations = new Map<string, ListingOperationView>();
  let editSource: OwnedEditSource | undefined;
  let acquiringEditDraft:
    Promise<PublicDraftController | undefined> | undefined;
  let reviewed:
    | {
        setup: InboxSetupView | undefined;
        proof?: NewListingReadiness;
        status: 'active' | 'sold';
        review: ListingReview;
      }
    | undefined;
  let reviewing: Promise<ListingReviewResult> | undefined;
  let reviewingForm: string | undefined;
  let reviewingSetup: InboxSetupView | undefined;
  let reviewingStatus: 'active' | 'sold' | undefined;
  function clearReview() {
    if (reviewed?.proof) closeNewListingReadiness(reviewed.proof);
    if (reviewed) closeListingReview(reviewed.review);
    reviewed = undefined;
  }
  function current() {
    return (
      alive &&
      active.get(context)?.stamp === stamp &&
      identityViewSession(context) === session &&
      identityViewMatchesOwner(context, owner) &&
      (!repository || publicDraftRepositoryOwner(repository) === owner)
    );
  }
  let notifying = false;
  function notify() {
    if (notifying) return;
    notifying = true;
    try {
      for (const [key, listener] of Array.from(listeners.entries()))
        if (listeners.get(key) === listener) {
          try {
            listener();
          } catch {
            /*Presentation observers cannot authorize or retain ownership.*/
          }
        }
    } finally {
      notifying = false;
    }
  }
  function retire() {
    if (!alive) return;
    alive = false;
    unsubscribe();
    signal?.removeEventListener('abort', retire);
    cancellation.abort();
    clearReview();
    if (editSource) closeOwnedEditSource(editSource);
    if (controller) closePublicDraftController(controller);
    for (const operation of operations.values())
      closeListingOperationView(operation);
    if (database) closeBrowserDatabase(database);
    if (active.get(context)?.stamp === stamp) active.delete(context);
    notify();
    listeners.clear();
  }
  active.set(context, { stamp, retire });
  unsubscribe = subscribeIdentityInvalidation(session, () => {
    // An owned revision's delivery rechecks the same installed author. Keep
    // this exact editor while its original route/account/storage stay current.
    if (route.mode !== 'edit' || !current()) retire();
  });
  signal?.addEventListener('abort', retire, { once: true });
  if (signal?.aborted) retire();
  function unavailable(reason: 'resource' | 'storage' | 'source') {
    retire();
    return { status: 'unavailable' as const, reason };
  }
  try {
    const opened = await openBrowserDatabase(cancellation.signal);
    if (opened.state !== 'ready') return unavailable('storage');
    if (!current()) {
      closeBrowserDatabase(opened.owner);
      return unavailable('storage');
    }
    database = opened.owner;
    const publications = createPublicPublicationRepository(database, owner);
    repository = publications && publicPublicationDraftRepository(publications);
    if (!publications || !repository || !current())
      return unavailable('storage');
    const drafts = repository;
    let sourceForm: PublicDraftForm | undefined,
      coordinate: string | null = null,
      sourceEventId: string | null = null;
    if (route.mode === 'edit') {
      editSource = await openOwnedEditSource(
        publications,
        context,
        route.naddr,
        runtime,
        cancellation.signal
      );
      if (!current()) return unavailable('source');
      const retained = editSource && ownedEditSourceSnapshot(editSource);
      if (!retained) return unavailable('source');
      sourceForm = retained.form;
      coordinate = route.naddr;
      sourceEventId = retained.sourceEventId;
    } else {
      const acquired = await createPublicDraftController(
        drafts,
        emptyForm,
        route.mode === 'draft' ? route.draftId : undefined
      );
      if (!current()) {
        if (acquired) closePublicDraftController(acquired);
        return unavailable('resource');
      }
      controller = acquired;
      if (!controller) return unavailable('resource');
      if (route.mode === 'draft') {
        const quota = publicPublicationQuotaRepository(publications);
        if (!quota) return unavailable('source');
        const inspected = await inspectPublicStorage(quota);
        if (!current() || !inspected.ok) return unavailable('source');
        const inventory = publicInventorySnapshot(quota, inspected.value);
        if (!inventory) return unavailable('source');
        for (const item of inventory.rows) {
          if (item.family !== 'public_operation') continue;
          const read = await readPublicListingIntent(publications, item.id);
          if (!current()) return unavailable('source');
          const row =
            read.status === 'found' &&
            publicRecordSnapshot(read.record, owner, item.id);
          if (!row || row.family !== 'public_operation')
            return unavailable('source');
          if (row.source.type !== 'draft' || row.source.id !== route.draftId)
            continue;
          if (operations.size >= 100) return unavailable('source');
          const operation = await readListingOperationView(
            publications,
            session,
            item.id
          );
          if (!current()) {
            if (operation.status === 'found')
              closeListingOperationView(operation.view);
            return unavailable('source');
          }
          if (operation.status !== 'found') return unavailable('source');
          operations.set(item.id, operation.view);
        }
      }
    }
    if (!current()) return unavailable('source');
    function reviewCurrent(review: ListingReview) {
      const preview = listingReviewSnapshot(review),
        local = controller && publicDraftControllerSnapshot(controller);
      return !!(
        current() &&
        !closing &&
        !operations.size &&
        route &&
        reviewed?.review === review &&
        listingReviewOwnership(review)?.current() &&
        preview &&
        local?.state === 'saved' &&
        local.draft &&
        preview.owner === owner &&
        preview.draftId === local.draft.id &&
        preview.draftRevision === local.draft.revision &&
        JSON.stringify(preview.form) === JSON.stringify(local.form)
      );
    }
    const value: Editor = {
      current,
      retire,
      source: () => (current() ? editSource : undefined),
      observe: () => {
        if (current()) notify();
      },
      snapshot() {
        if (!current()) {
          retire();
          return;
        }
        const local = controller && publicDraftControllerSnapshot(controller);
        return {
          mode: route.mode,
          owner,
          form: local ? local.form : { ...(sourceForm ?? emptyForm) },
          editable:
            operations.size === 0 &&
            (route.mode !== 'edit' ||
              !!(editSource && authorizeOwnedEditSource(editSource))),
          draftId: local?.draft?.id ?? null,
          state:
            local && operations.size === 0
              ? local.state === 'closed'
                ? 'failed'
                : local.state
              : 'read-only',
          failure: local?.failure ?? null,
          coordinate,
          sourceEventId,
          ...(editSource
            ? {
                sourcePublishedAtSeconds:
                  ownedEditSourceSnapshot(editSource)?.publishedAtSeconds
              }
            : {}),
          operations: Array.from(operations.values())
        };
      },
      async edit(input) {
        if (!current()) {
          retire();
          return false;
        }
        if (route.mode === 'edit') {
          if (
            !editSource ||
            !authorizeOwnedEditSource(editSource) ||
            !sourceForm
          )
            return false;
          if (!controller) {
            acquiringEditDraft ??= createPublicDraftController(
              drafts,
              sourceForm
            );
            const acquired = await acquiringEditDraft;
            if (!current() || !authorizeOwnedEditSource(editSource)) {
              if (acquired) closePublicDraftController(acquired);
              return false;
            }
            controller = acquired;
          }
        }
        if (!controller || operations.size) return false;
        if (!editPublicDraft(controller, input)) return false;
        clearReview();
        notify();
        const pending = flushPublicDraft(controller);
        await Promise.resolve();
        if (!current()) {
          retire();
          return false;
        }
        notify();
        const saved = await pending;
        if (!current()) {
          retire();
          return false;
        }
        notify();
        return saved;
      },
      async flush() {
        if (!current()) {
          retire();
          return false;
        }
        if (!controller || operations.size) return false;
        const pending = flushPublicDraft(controller);
        await Promise.resolve();
        if (!current()) {
          retire();
          return false;
        }
        notify();
        const saved = await pending;
        if (!current()) {
          retire();
          return false;
        }
        notify();
        return saved;
      },
      async saveClose() {
        if (!current() || !controller || operations.size || closing)
          return false;
        closing = true;
        try {
          if (publicDraftControllerSnapshot(controller)?.state === 'failed') {
            const retried = await retryPublicDraft(controller);
            if (!current()) {
              retire();
              return false;
            }
            notify();
            if (!retried) return false;
          }
          const saved = await saveAndClosePublicDraft(controller);
          if (!current()) {
            retire();
            return false;
          }
          if (!saved) {
            notify();
            return false;
          }
          retire();
          return true;
        } finally {
          closing = false;
        }
      },
      review(setup, input, status) {
        const local = controller && publicDraftControllerSnapshot(controller);
        if (
          !current() ||
          closing ||
          !controller ||
          !local ||
          (route.mode === 'edit' && !editSource) ||
          (route.mode !== 'edit' && status !== 'active') ||
          operations.size ||
          !exactLocalFields(input, Object.keys(emptyForm)) ||
          JSON.stringify(input) !== JSON.stringify(local.form)
        )
          return Promise.resolve({
            status: 'blocked',
            reason: 'invalid_scope'
          });
        if (
          reviewed &&
          reviewed.status === status &&
          reviewed.setup === setup &&
          reviewCurrent(reviewed.review)
        )
          return Promise.resolve({ status: 'review', review: reviewed.review });
        const formWire = JSON.stringify(local.form);
        if (reviewing)
          return reviewingForm === formWire &&
            reviewingSetup === setup &&
            reviewingStatus === status
            ? reviewing
            : Promise.resolve({ status: 'blocked', reason: 'conflict' });
        clearReview();
        const proof =
            route.mode === 'edit'
              ? undefined
              : captureNewListingReadiness(
                  session,
                  setup,
                  'reviewed_new_listing_inbox'
                ),
          command = newLocalId();
        if ((!proof && route.mode !== 'edit') || !command) {
          if (proof) closeNewListingReadiness(proof);
          return Promise.resolve({ status: 'blocked', reason: 'unavailable' });
        }
        const original = controller;
        reviewingForm = formWire;
        reviewingSetup = setup;
        reviewingStatus = status;
        const prepared =
          route.mode === 'edit' && editSource
            ? status === 'sold'
              ? reviewMarkSold(
                  editSource,
                  original,
                  command,
                  'reviewed_sold_availability'
                )
              : reviewOwnedRevision(
                  editSource,
                  original,
                  command,
                  'reviewed_owned_revision'
                )
            : proof
              ? reviewNewListing(
                  publications,
                  original,
                  proof,
                  command,
                  'reviewed_new_listing'
                )
              : Promise.resolve<ListingReviewResult>({
                  status: 'blocked',
                  reason: 'unavailable'
                });
        const pending = prepared
          .then((result): ListingReviewResult => {
            const actual = publicDraftControllerSnapshot(original);
            if (
              !current() ||
              closing ||
              !actual ||
              JSON.stringify(actual.form) !== formWire ||
              result.status !== 'review'
            ) {
              if (proof) closeNewListingReadiness(proof);
              if (result.status === 'review') closeListingReview(result.review);
              return result.status === 'blocked'
                ? result
                : { status: 'blocked', reason: 'conflict' };
            }
            reviewed = { setup, proof, status, review: result.review };
            if (!reviewCurrent(result.review)) {
              clearReview();
              return { status: 'blocked', reason: 'conflict' };
            }
            return result;
          })
          .finally(() => {
            if (reviewing === pending) {
              reviewing = undefined;
              reviewingForm = undefined;
              reviewingSetup = undefined;
              reviewingStatus = undefined;
            }
          });
        reviewing = pending;
        return pending;
      },
      reviewCurrent,
      subscribe(listener) {
        if (!current()) return () => {};
        const key = Symbol();
        listeners.set(key, listener);
        return () => {
          listeners.delete(key);
        };
      }
    };
    const editor = Object.freeze({}) as ListingEditor;
    editors.set(editor, value);
    return { status: 'ready', editor };
  } catch {
    return unavailable('storage');
  }
}
export function listingEditorSnapshot(
  editor: ListingEditor
): ListingEditorSnapshot | undefined {
  return editors.get(editor)?.snapshot();
}
export function editListingEditor(
  editor: ListingEditor,
  form: unknown
): Promise<boolean> {
  return editors.get(editor)?.edit(form) ?? Promise.resolve(false);
}
export function flushListingEditor(editor: ListingEditor): Promise<boolean> {
  return editors.get(editor)?.flush() ?? Promise.resolve(false);
}
export function closeListingEditor(editor: ListingEditor): void {
  editors.get(editor)?.retire();
}
export function listingEditorEditSource(
  editor: ListingEditor
): OwnedEditSource | undefined {
  return editors.get(editor)?.source();
}
export function observeListingEditor(editor: ListingEditor): void {
  editors.get(editor)?.observe();
}
export function saveAndCloseListingEditor(
  editor: ListingEditor
): Promise<boolean> {
  return editors.get(editor)?.saveClose() ?? Promise.resolve(false);
}
export function subscribeListingEditor(
  editor: ListingEditor,
  listener: () => void
): () => void {
  return editors.get(editor)?.subscribe(listener) ?? (() => {});
}
// Original editor custody remains private. Review never reserves or signs.
export function reviewListingEditor(
  editor: ListingEditor,
  setup: InboxSetupView | undefined,
  form: PublicDraftForm,
  status: 'active' | 'sold' = 'active'
): Promise<ListingReviewResult> {
  return (
    editors.get(editor)?.review(setup, form, status) ??
    Promise.resolve({ status: 'blocked', reason: 'invalid_scope' })
  );
}
export function listingEditorReviewCurrent(
  editor: ListingEditor,
  review: ListingReview
): boolean {
  return editors.get(editor)?.reviewCurrent(review) ?? false;
}
