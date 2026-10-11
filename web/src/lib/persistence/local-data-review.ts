import {
  identityViewSession,
  subscribeIdentityView,
  type IdentityViewContext
} from '../runtime/view-context.ts';
import {
  identityLocalOwnership,
  identitySessionSnapshot,
  subscribeIdentityInvalidation
} from '../runtime/identity-session.ts';
import { createPrivateSession } from '../runtime/private-session.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  type BrowserDatabase
} from './database.ts';
import {
  createPublicQuotaRepository,
  inspectPublicStorage,
  publicInventorySnapshot,
  reviewPublicCleanup,
  commitPublicCleanup,
  type PublicQuotaRepository,
  type PublicInventory,
  type PublicCleanup
} from './quota.ts';
import {
  createPrivateStorageRepository,
  inspectPrivateStorage,
  listPrivateReceivedRecords,
  type PrivateStorageRepository
} from './private-storage.ts';
import { privateRecordIdentity, privateRecordWire } from './private-records.ts';
import {
  captureReceivedCleanup,
  deleteReviewedReceivedCopies,
  stopReceivedCleanup,
  type ReceivedCleanup
} from './inbox-retention.ts';
import {
  PUBLIC_CLEANUP_CONSEQUENCES,
  type PublicInventoryRow
} from './local-inventory.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';

declare const reviewBrand: unique symbol;
export type LocalDataReview = Readonly<{ [reviewBrand]: true }>;
type Category = 'public' | 'received';
type Preview = Readonly<{
  category: Category;
  selected: number;
  consequences: string;
}>;
type Inventory = Readonly<{
  owner: string;
  publicRows: readonly PublicInventoryRow[];
  received: readonly Readonly<{ id: string; logicalBytes: number }>[];
  outbox: Readonly<{ count: number; bytes: number }>;
  receivedRemoval: boolean;
}>;
export type LocalDataReviewSnapshot =
  | Readonly<{ state: 'guest' | 'closed' }>
  | Readonly<{ state: 'loading' }>
  | Readonly<{ state: 'unavailable'; notice: string }>
  | (Inventory &
      Readonly<{
        state: 'ready';
        busy: boolean;
        preview?: Preview;
        notice?: string;
      }>);
type Controller = {
  snapshot(): LocalDataReviewSnapshot;
  review(consent: unknown): Promise<void>;
  preview(category: unknown, selection: unknown): Promise<boolean>;
  confirm(consent: unknown): Promise<boolean>;
  cancel(): void;
  close(): void;
  subscribe(listener: () => void): () => void;
};
const reviews = new WeakMap<LocalDataReview, Controller>();
const receivedConsequences =
  'Removing selected encrypted copies loses those local messages. Relay recovery is not guaranteed. Other participants and relays may retain copies. Outbox recovery records stay unchanged. Browser-local storage is not a backup.';
const unavailable =
  'Local inventory or removal was not confirmed. Existing local data has not been reset. You can still browse public food. Review current data before trying again; an unknown completion may have changed selected local copies.';

function selection(value: unknown, maximum: number): string[] | undefined {
  if (
    typeof value !== 'string' ||
    new TextEncoder().encode(value).length > 160000
  )
    return;
  try {
    const parsed: unknown = JSON.parse(value);
    if (
      !Array.isArray(parsed) ||
      !parsed.length ||
      parsed.length > maximum ||
      !parsed.every((id: unknown) => typeof id === 'string') ||
      new Set(parsed).size !== parsed.length
    )
      return;
    return parsed.slice();
  } catch {
    return;
  }
}

// Pure per-view construction. Merely visiting Privacy never opens the inventory,
// acquires an extension permission, probes messaging or creates a private socket.
export function createLocalDataReview(
  context: IdentityViewContext
): LocalDataReview {
  const token = Object.freeze({}) as LocalDataReview;
  const listeners = new Map<symbol, () => void>();
  let closed = false,
    generation = Symbol(),
    busy = false;
  // Whole immutable snapshots are replaced; no caller object is mutated.
  let state: LocalDataReviewSnapshot;
  state = { state: 'guest' };
  let ownership: ReturnType<typeof identityLocalOwnership>;
  let database: BrowserDatabase | undefined,
    cancellation: AbortController | undefined;
  let publicRepository: PublicQuotaRepository | undefined,
    inventory: PublicInventory | undefined;
  let privateRepository: PrivateStorageRepository | undefined;
  let publicCleanup: PublicCleanup | undefined,
    receivedCleanup: ReceivedCleanup | undefined;
  let offIdentity = () => {};
  function notify() {
    for (const listener of listeners.values()) {
      try {
        listener();
      } catch {
        /* Presentation cannot interrupt storage settlement. */
      }
    }
  }
  function cancelPreview() {
    publicCleanup = undefined;
    if (receivedCleanup) stopReceivedCleanup(receivedCleanup);
    receivedCleanup = undefined;
    if (state.state === 'ready') state = { ...state, preview: undefined };
  }
  function retire() {
    generation = Symbol();
    cancellation?.abort();
    cancellation = undefined;
    cancelPreview();
    if (database) closeBrowserDatabase(database);
    database = undefined;
    publicRepository = undefined;
    privateRepository = undefined;
    inventory = undefined;
    ownership = undefined;
    busy = false;
    offIdentity();
    offIdentity = () => {};
    state = closed ? { state: 'closed' } : { state: 'guest' };
  }
  function current(original: symbol) {
    return !closed && generation === original && ownership?.current() === true;
  }
  function observe() {
    if (ownership && !ownership.current()) {
      retire();
      notify();
    }
  }
  const offView = subscribeIdentityView(context, observe);
  async function review(consent: unknown) {
    if (closed || busy || consent !== 'reviewed_local_inventory') return;
    retire();
    const identity = identityViewSession(context);
    const captured = identity && identityLocalOwnership(identity);
    if (!identity || !captured?.current() || typeof window === 'undefined') {
      notify();
      return;
    }
    ownership = captured;
    busy = true;
    cancellation = new AbortController();
    const original = generation;
    offIdentity = subscribeIdentityInvalidation(identity, observe);
    state = { state: 'loading' };
    notify();
    try {
      const opened = await openBrowserDatabase(cancellation.signal);
      if (!current(original)) {
        if (opened.state === 'ready') closeBrowserDatabase(opened.owner);
        return;
      }
      if (opened.state !== 'ready') throw Error('local storage unavailable');
      database = opened.owner;
      const pub = createPublicQuotaRepository(database, captured.owner),
        priv = createPrivateStorageRepository(database, captured.owner);
      if (!pub || !priv) throw Error('local owner namespace unavailable');
      const scan = await inspectPublicStorage(pub);
      if (!current(original)) return;
      if (!scan.ok) throw Error('public inventory unavailable');
      const publicState = publicInventorySnapshot(pub, scan.value);
      const received = await listPrivateReceivedRecords(priv);
      if (!current(original)) return;
      const outbox = await inspectPrivateStorage(priv, 'private_sends');
      if (!current(original)) return;
      if (!publicState || !received.ok || !outbox.ok)
        throw Error('private inventory unavailable');
      const copies = received.value.map((handle) => {
        const row = privateRecordIdentity(handle),
          wire = row && privateRecordWire(handle, captured.owner, row.id);
        if (!row || row.owner !== captured.owner || !wire)
          throw Error('received inventory unavailable');
        return {
          id: row.id,
          logicalBytes: new TextEncoder().encode(
            JSON.stringify({ owner: captured.owner, id: row.id, wire })
          ).length
        };
      });
      publicRepository = pub;
      privateRepository = priv;
      inventory = scan.value;
      state = {
        state: 'ready',
        busy: false,
        owner: captured.owner,
        publicRows: publicState.rows,
        received: copies,
        outbox: outbox.value,
        receivedRemoval:
          identitySessionSnapshot(identity).state === 'messaging_capable'
      };
    } catch {
      if (current(original)) {
        retire();
        state = { state: 'unavailable', notice: unavailable };
      }
    } finally {
      if (generation === original) busy = false;
      notify();
    }
  }
  reviews.set(token, {
    snapshot() {
      observe();
      return state.state === 'ready'
        ? {
            ...state,
            publicRows: state.publicRows.map((row) => ({ ...row })),
            received: state.received.map((row) => ({ ...row })),
            outbox: { ...state.outbox },
            preview: state.preview && { ...state.preview }
          }
        : { ...state };
    },
    review,
    async preview(category, serialized) {
      observe();
      if (closed || busy || state.state !== 'ready' || !ownership?.current())
        return false;
      const ids = selection(
        serialized,
        category === 'public'
          ? LOCAL_PERSISTENCE_BUDGETS.publicDrafts +
              LOCAL_PERSISTENCE_BUDGETS.publicOperations
          : LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes
      );
      cancelPreview();
      if (!ids) {
        notify();
        return false;
      }
      const original = generation;
      if (category === 'public' && publicRepository && inventory) {
        const captured = reviewPublicCleanup(publicRepository, inventory, ids);
        if (!captured.ok) {
          notify();
          return false;
        }
        publicCleanup = captured.value;
        state = {
          ...state,
          preview: {
            category,
            selected: ids.length,
            consequences: PUBLIC_CLEANUP_CONSEQUENCES
          },
          notice: undefined
        };
        notify();
        return true;
      }
      if (
        category !== 'received' ||
        !privateRepository ||
        !state.receivedRemoval ||
        !ids.every(
          (id) =>
            state.state === 'ready' &&
            state.received.some((row) => row.id === id)
        )
      ) {
        notify();
        return false;
      }
      const identity = identityViewSession(context);
      if (!identity) return false;
      busy = true;
      state = { ...state, busy: true };
      notify();
      try {
        const session = await createPrivateSession(
          identity,
          'reviewed_private_session'
        );
        if (!session || !current(original)) return false;
        const cleanup = await captureReceivedCleanup(
          privateRepository,
          session,
          ids,
          'reviewed_received_cleanup_selection'
        );
        if (!current(original)) {
          if (cleanup) stopReceivedCleanup(cleanup);
          return false;
        }
        if (!cleanup || state.state !== 'ready') return false;
        receivedCleanup = cleanup;
        state = {
          ...state,
          preview: {
            category: 'received',
            selected: ids.length,
            consequences: receivedConsequences
          },
          notice: undefined
        };
        return true;
      } catch {
        return false;
      } finally {
        if (generation === original) {
          busy = false;
          if (state.state === 'ready') state = { ...state, busy: false };
        }
        notify();
      }
    },
    async confirm(consent) {
      observe();
      if (
        closed ||
        busy ||
        consent !== 'reviewed_local_removal' ||
        state.state !== 'ready' ||
        !state.preview ||
        !ownership?.current() ||
        !cancellation
      )
        return false;
      const original = generation,
        preview = state.preview,
        owner = ownership.owner;
      const pub = publicCleanup,
        received = receivedCleanup,
        repo = publicRepository,
        signal = cancellation.signal;
      publicCleanup = undefined;
      receivedCleanup = undefined;
      busy = true;
      state = { ...state, busy: true, preview: undefined };
      notify();
      let confirmed = false;
      try {
        if (
          preview.category === 'public' &&
          pub &&
          repo &&
          navigator.locks?.request
        ) {
          confirmed = await navigator.locks.request(
            'harvestcircle:owner:' + owner,
            { mode: 'exclusive', ifAvailable: true },
            async (lock) => {
              if (!lock || !current(original)) return false;
              const committed = await commitPublicCleanup(repo, pub, signal);
              if (!committed.ok || !current(original)) return false;
              const checked = await inspectPublicStorage(repo);
              if (!checked.ok || !current(original)) return false;
              const remaining = publicInventorySnapshot(repo, checked.value);
              if (
                !remaining ||
                committed.value.some((key) =>
                  remaining.rows.some((row) => row.key === key)
                )
              )
                return false;
              inventory = checked.value;
              if (state.state === 'ready')
                state = { ...state, publicRows: remaining.rows };
              return true;
            }
          );
        } else if (preview.category === 'received' && received) {
          // Keep the owned action available to synchronous retirement while running.
          receivedCleanup = received;
          const result = await deleteReviewedReceivedCopies(
            received,
            'reviewed_delete_local_received_copies'
          );
          if (
            result.status === 'removed' &&
            current(original) &&
            privateRepository
          ) {
            const scan = await listPrivateReceivedRecords(privateRepository);
            if (scan.ok && current(original) && state.state === 'ready') {
              state = {
                ...state,
                received: state.received.filter((row) =>
                  scan.value.some(
                    (handle) => privateRecordIdentity(handle)?.id === row.id
                  )
                )
              };
              confirmed = true;
            }
          }
        }
      } catch {
        confirmed = false;
      } finally {
        if (received) stopReceivedCleanup(received);
        if (receivedCleanup === received) receivedCleanup = undefined;
        if (generation === original) {
          busy = false;
          if (state.state === 'ready')
            state = {
              ...state,
              busy: false,
              notice: confirmed
                ? 'Removed ' +
                  preview.selected +
                  (preview.category === 'public'
                    ? ' selected public record'
                    : ' selected received encrypted copy') +
                  (preview.selected === 1 ? '.' : ' entries.') +
                  ' Other local categories and remote copies are unchanged.'
                : unavailable
            };
        }
        notify();
      }
      return confirmed;
    },
    cancel() {
      if (busy) return;
      cancelPreview();
      notify();
    },
    close() {
      if (closed) return;
      closed = true;
      retire();
      offView();
      notify();
      listeners.clear();
    },
    subscribe(listener) {
      if (closed) return () => {};
      const id = Symbol();
      listeners.set(id, listener);
      return () => {
        listeners.delete(id);
      };
    }
  });
  return token;
}
export function localDataReviewSnapshot(
  token: LocalDataReview
): LocalDataReviewSnapshot {
  return reviews.get(token)?.snapshot() ?? { state: 'closed' };
}
export async function reviewLocalData(
  token: LocalDataReview,
  consent: unknown
): Promise<void> {
  await reviews.get(token)?.review(consent);
}
export function previewLocalDataRemoval(
  token: LocalDataReview,
  category: unknown,
  selection: unknown
): Promise<boolean> {
  return (
    reviews.get(token)?.preview(category, selection) ?? Promise.resolve(false)
  );
}
export function confirmLocalDataRemoval(
  token: LocalDataReview,
  consent: unknown
): Promise<boolean> {
  return reviews.get(token)?.confirm(consent) ?? Promise.resolve(false);
}
export function cancelLocalDataRemoval(token: LocalDataReview): void {
  reviews.get(token)?.cancel();
}
export function closeLocalDataReview(token: LocalDataReview): void {
  reviews.get(token)?.close();
}
export function subscribeLocalDataReview(
  token: LocalDataReview,
  listener: () => void
): () => void {
  return reviews.get(token)?.subscribe(listener) ?? (() => {});
}
