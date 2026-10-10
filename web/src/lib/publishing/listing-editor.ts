import {
  exactLocalFields,
  type PublicDraftForm
} from '../contracts/local-records.ts';
import { canonicalLocalId } from '../private-handles.ts';
import { decodeProductReference } from '../nostr/references.ts';
import { inspectLocalTemplate } from '../nostr/local-template.ts';
import { projectFoodParts } from '../contracts/food-availability-v1/read.ts';
import { buildFoodTemplate } from '../contracts/food-availability-v1/write.ts';
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
  readPublicDraft,
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
  operations: readonly ListingOperationView[];
}>;
type Editor = Readonly<{
  current(): boolean;
  retire(): void;
  snapshot(): ListingEditorSnapshot | undefined;
  edit(form: unknown): Promise<boolean>;
  flush(): Promise<boolean>;
  saveClose(): Promise<boolean>;
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
  signal?: AbortSignal
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
    if (controller) closePublicDraftController(controller);
    for (const operation of operations.values())
      closeListingOperationView(operation);
    if (database) closeBrowserDatabase(database);
    if (active.get(context)?.stamp === stamp) active.delete(context);
    notify();
    listeners.clear();
  }
  active.set(context, { stamp, retire });
  unsubscribe = subscribeIdentityInvalidation(session, retire);
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
      const reference = decodeProductReference(route.naddr),
        quota = publicPublicationQuotaRepository(publications);
      if (!reference || !quota) return unavailable('source');
      const inspected = await inspectPublicStorage(quota);
      if (!current() || !inspected.ok) return unavailable('source');
      const inventory = publicInventorySnapshot(quota, inspected.value);
      if (!inventory) return unavailable('source');
      let matching:
        Readonly<{ form: PublicDraftForm; eventId: string }> | undefined;
      for (const item of inventory.rows) {
        if (item.family !== 'public_operation') continue;
        const read = await readPublicListingIntent(publications, item.id);
        if (!current()) return unavailable('source');
        const row =
          read.status === 'found' &&
          publicRecordSnapshot(read.record, owner, item.id);
        if (
          !row ||
          row.family !== 'public_operation' ||
          row.capture.kind !== 30402 ||
          !row.artifact ||
          row.source.type !== 'draft'
        )
          continue;
        const template = inspectLocalTemplate(row.capture.wire, owner, 30402);
        if (!template) continue;
        const projected = projectFoodParts(template);
        if (
          projected.outcome !== 'focused' ||
          projected.projection.identifier !== reference.identifier
        )
          continue;
        const source = await readPublicDraft(drafts, row.source.id);
        if (!current()) return unavailable('source');
        if (!source.ok || source.value.revision !== row.source.revision)
          continue;
        const value = source.value.form,
          p = projected.projection;
        if (value.contactType === '' && value.contactValue !== '') continue;
        const built = buildFoodTemplate({
          identifier: p.identifier,
          title: value.title,
          description: value.description,
          location: value.location,
          amount: value.amount,
          currency: value.currency,
          unit: value.unit,
          ...(value.quantity === '' ? {} : { quantity: value.quantity }),
          published_at: p.published_at,
          created_at: template.created_at,
          status: p.status,
          ...(value.contactType === ''
            ? {}
            : {
                contact: {
                  type: value.contactType,
                  value: value.contactValue,
                  public: true as const
                }
              })
        });
        if (
          !built.ok ||
          JSON.stringify({
            pubkey: owner,
            kind: 30402,
            created_at: template.created_at,
            tags: built.wire_parts.tags,
            content: built.wire_parts.content
          }) !== row.capture.wire
        )
          continue;
        if (matching) return unavailable('source');
        matching = { form: value, eventId: row.artifact.eventId };
      }
      if (!matching) return unavailable('source');
      sourceForm = matching.form;
      coordinate = route.naddr;
      sourceEventId = matching.eventId;
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
    const value: Editor = {
      current,
      retire,
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
          editable: route.mode !== 'edit' && operations.size === 0,
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
          operations: Array.from(operations.values())
        };
      },
      async edit(input) {
        if (!current()) {
          retire();
          return false;
        }
        if (!controller || route.mode === 'edit' || operations.size)
          return false;
        if (!editPublicDraft(controller, input)) return false;
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
        if (!controller || route.mode === 'edit' || operations.size)
          return false;
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
        if (
          !current() ||
          !controller ||
          route.mode === 'edit' ||
          operations.size ||
          closing
        )
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
