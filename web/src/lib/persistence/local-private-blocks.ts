import { canonicalPublicKey } from '../contracts/public-key.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  conversationSnapshot,
  type AdmittedConversation
} from '../messaging/admit-conversation.ts';
import {
  unlockedConversationCurrent,
  unlockedSessionOwnership,
  type UnlockedSession
} from '../messaging/unlocked-session.ts';
import {
  identityLocalOwnership,
  subscribeIdentityInvalidation,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  identityViewSession,
  subscribeIdentityView,
  type IdentityViewContext
} from '../runtime/view-context.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseState,
  browserDatabaseTransaction,
  type BrowserDatabase
} from './database.ts';

declare const blocksBrand: unique symbol;
export type LocalPrivateBlocks = Readonly<{ [blocksBrand]: true }>;
export const LOCAL_PRIVATE_BLOCKS_CONTEXT =
  'harvestcircle_local_private_blocks';
export const PRIVATE_BLOCK_DISCLOSURE =
  'Blocking a person is local to this account in this browser. It hides their private messages and previews, stops future automatic continuation, and may discard unsaved private text. It does not delete encrypted evidence, remote copies or delivery receipts, reject an order, notify the sender or send a public report. Undo restores eligible local views; another explicit bounded decryption action may be needed. Browser storage is not a backup. Private messages have no forward secrecy or post-compromise security.';
type Failure =
  | 'invalid'
  | 'stopped'
  | 'busy'
  | 'unavailable'
  | 'aborted'
  | 'unknown_completion'
  | 'corrupt_record'
  | 'conflict'
  | 'capacity';
export type PrivateBlocksSnapshot =
  | Readonly<{ state: 'guest' | 'loading' }>
  | Readonly<{ state: 'unavailable'; reason?: Failure }>
  | Readonly<{
      state: 'ready';
      owner: string;
      revision: number;
      peers: readonly string[];
    }>;
type Result<T> =
  Readonly<{ ok: true; value: T }> | Readonly<{ ok: false; reason: Failure }>;
type Row = {
  schema: 1;
  family: 'private_blocks';
  owner: string;
  id: 'private_blocks';
  revision: number;
  peers: string[];
};
type Stored = { row: Row; wire: string | null };
type Scope = { owner: string; database: BrowserDatabase; current(): boolean };
export type PrivateBlockResult = Readonly<{
  status: 'saved' | 'existing' | Failure;
}>;
type Controller = {
  snapshot(): PrivateBlocksSnapshot;
  mount(): Promise<void>;
  refresh(): Promise<void>;
  block(
    room: AdmittedConversation,
    unlocked: UnlockedSession,
    review: unknown
  ): Promise<PrivateBlockResult>;
  current(): boolean;
  undo(peer: unknown, review: unknown): Promise<PrivateBlockResult>;
  subscribe(listener: () => void): () => void;
  close(): void;
};
const controllers = new WeakMap<LocalPrivateBlocks, Controller>();
const failed = (reason: Failure): Result<never> => ({ ok: false, reason });
function decode(value: unknown, owner: string): Stored | undefined {
  if (value === undefined)
    return {
      row: {
        schema: 1,
        family: 'private_blocks',
        owner,
        id: 'private_blocks',
        revision: 0,
        peers: []
      },
      wire: null
    };
  if (
    !exactLocalFields(value, ['owner', 'id', 'wire']) ||
    value.owner !== owner ||
    value.id !== 'private_blocks' ||
    typeof value.wire !== 'string' ||
    new TextEncoder().encode(value.wire).length >
      LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes
  )
    return;
  let row: unknown;
  try {
    row = JSON.parse(value.wire);
  } catch {
    return;
  }
  if (
    !exactLocalFields(row, [
      'schema',
      'family',
      'owner',
      'id',
      'revision',
      'peers'
    ]) ||
    row.schema !== 1 ||
    row.family !== 'private_blocks' ||
    row.owner !== owner ||
    row.id !== 'private_blocks' ||
    !Number.isSafeInteger(row.revision) ||
    typeof row.revision !== 'number' ||
    row.revision < 1 ||
    !Array.isArray(row.peers) ||
    row.peers.length > LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes ||
    row.peers.some(
      (key: unknown) =>
        typeof key !== 'string' || canonicalPublicKey(key) !== key
    ) ||
    new Set(row.peers).size !== row.peers.length ||
    row.peers.some(
      (key: string, index: number, peers: string[]) =>
        index > 0 && key <= peers[index - 1]
    )
  )
    return;
  const admitted: Row = {
    schema: 1,
    family: 'private_blocks',
    owner,
    id: 'private_blocks',
    revision: row.revision,
    peers: row.peers.slice() as string[]
  };
  return JSON.stringify(admitted) === value.wire
    ? { row: admitted, wire: value.wire }
    : undefined;
}
// One fixed owner/key record, with bounded peer entries. This avoids
// scanning or adopting the independent public-hide preferences in the same store.
function transaction<T>(
  scope: Scope,
  mode: 'readonly' | 'readwrite',
  apply: (stored: Stored, tx: IDBTransaction) => Result<T>
): Promise<Result<T>> {
  if (!scope.current()) return Promise.resolve(failed('stopped'));
  let tx: IDBTransaction;
  try {
    tx = browserDatabaseTransaction(
      scope.database,
      ['local_preferences'],
      mode
    );
  } catch {
    return Promise.resolve(failed('unavailable'));
  }
  return new Promise((resolve) => {
    let result: Result<T> | undefined;
    const refuse = (reason: Failure) => {
      result = failed(reason);
      try {
        tx.abort();
      } catch {
        result = failed('unknown_completion');
      }
    };
    tx.addEventListener('complete', () =>
      resolve(
        scope.current()
          ? (result ?? failed('unknown_completion'))
          : failed('stopped')
      )
    );
    tx.addEventListener('abort', () =>
      resolve(
        !scope.current()
          ? failed('stopped')
          : result && !result.ok && result.reason !== 'unknown_completion'
            ? result
            : failed('aborted')
      )
    );
    try {
      const request = tx
        .objectStore('local_preferences')
        .get([scope.owner, 'private_blocks']);
      request.addEventListener('success', () => {
        if (!scope.current()) {
          refuse('stopped');
          return;
        }
        try {
          const row = decode(request.result, scope.owner);
          if (!row) {
            refuse('corrupt_record');
            return;
          }
          result = apply(row, tx);
          if (!result.ok) refuse(result.reason);
        } catch {
          refuse('aborted');
        }
      });
    } catch {
      refuse('aborted');
    }
  });
}
// Construction has no browser effect and holds no server-global account state.
function capturePrivateBlocks(
  identity: IdentitySession
): LocalPrivateBlocks | undefined {
  const observed = identityLocalOwnership(identity);
  if (!observed?.current()) return;
  const capture = observed;
  const owned = () => !closed && capture.current();
  const token = Object.freeze({}) as LocalPrivateBlocks;
  const listeners = new Map<symbol, () => void>();
  let state: PrivateBlocksSnapshot,
    mounted = false,
    closed = false;
  state = { state: 'loading' };
  let scope: Scope | undefined,
    generation = Symbol(),
    cancel: AbortController | undefined;
  let offInvalidation = () => {};
  function notify() {
    for (const listener of listeners.values()) {
      try {
        listener();
      } catch {
        /* Isolate presentation. */
      }
    }
  }
  function retire() {
    generation = Symbol();
    cancel?.abort();
    cancel = undefined;
    offInvalidation();
    offInvalidation = () => {};
    if (scope) closeBrowserDatabase(scope.database);
    scope = undefined;
    state = { state: 'unavailable', reason: 'stopped' };
  }
  function snapshot(): PrivateBlocksSnapshot {
    if (closed) return { state: 'unavailable', reason: 'stopped' };
    if (!owned() || (scope && !scope.current()))
      return { state: 'unavailable', reason: 'stopped' };
    return state.state === 'ready'
      ? { ...state, peers: state.peers.slice() }
      : { ...state };
  }
  async function refresh() {
    retire();
    notify();
    if (!mounted || closed) return;
    if (!owned()) return;
    const original = generation;
    const current = () =>
      !closed && mounted && original === generation && capture.current();
    state = { state: 'loading' };
    notify();
    cancel = new AbortController();
    offInvalidation = subscribeIdentityInvalidation(identity, () => {
      // Observations also fan out after a successful same-owner recheck.
      // Retire only actual ownership loss; another explicit private-session
      // acquisition must not close this still-current preference generation.
      if (!capture.current()) {
        retire();
        notify();
      }
    });
    const opened = await openBrowserDatabase(cancel.signal);
    if (!current()) {
      if (opened.state === 'ready') closeBrowserDatabase(opened.owner);
      return;
    }
    if (opened.state !== 'ready') {
      state = { state: 'unavailable', reason: 'unavailable' };
      notify();
      return;
    }
    const acquired: Scope = {
      owner: capture.owner,
      database: opened.owner,
      current: () =>
        current() && browserDatabaseState(opened.owner).state === 'ready'
    };
    scope = acquired;
    const read = await transaction(acquired, 'readonly', (stored) => ({
      ok: true,
      value: stored
    }));
    if (!acquired.current()) return;
    state = read.ok
      ? {
          state: 'ready',
          owner: acquired.owner,
          revision: read.value.row.revision,
          peers: read.value.row.peers.slice()
        }
      : { state: 'unavailable', reason: read.reason };
    notify();
  }
  async function change(
    peer: string,
    hidden: boolean,
    original: Scope
  ): Promise<PrivateBlockResult> {
    if (!original.current()) return { status: 'stopped' };
    if (!navigator.locks?.request) return { status: 'unavailable' };
    try {
      return await navigator.locks.request(
        'harvestcircle:owner:' + original.owner,
        { mode: 'exclusive', ifAvailable: true },
        async (lock) => {
          if (!lock) return { status: 'busy' as const };
          if (!original.current()) return { status: 'stopped' as const };
          const loaded = await transaction(original, 'readonly', (stored) => ({
            ok: true,
            value: stored
          }));
          if (!original.current()) return { status: 'stopped' as const };
          if (!loaded.ok) {
            state = { state: 'unavailable', reason: loaded.reason };
            notify();
            return { status: loaded.reason };
          }
          const before = loaded.value;
          if (before.row.peers.includes(peer) === hidden) {
            // Another controller may have acknowledged this complete native
            // row. Reconcile all preferences without another write/revision.
            state = {
              state: 'ready',
              owner: original.owner,
              revision: before.row.revision,
              peers: before.row.peers.slice()
            };
            notify();
            return { status: 'existing' as const };
          }
          if (
            before.row.revision >= Number.MAX_SAFE_INTEGER ||
            (hidden &&
              before.row.peers.length >=
                LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes)
          )
            return { status: 'capacity' as const };
          const next: Row = {
            ...before.row,
            revision: before.row.revision + 1,
            peers: hidden
              ? before.row.peers.concat(peer).sort()
              : before.row.peers.filter((key) => key !== peer)
          };
          const wire = JSON.stringify(next);
          const written = await transaction<boolean>(
            original,
            'readwrite',
            (stored, tx) => {
              if (stored.wire !== before.wire) return failed('conflict');
              tx.objectStore('local_preferences').put({
                owner: original.owner,
                id: 'private_blocks',
                wire
              });
              return { ok: true, value: true };
            }
          );
          if (!original.current()) return { status: 'stopped' as const };
          if (!written.ok) {
            if (
              written.reason === 'conflict' ||
              written.reason === 'unknown_completion'
            ) {
              state = { state: 'unavailable', reason: written.reason };
              notify();
            }
            return { status: written.reason };
          }
          const readback = await transaction(
            original,
            'readonly',
            (stored) => ({ ok: true, value: stored })
          );
          if (!original.current()) return { status: 'stopped' as const };
          if (!readback.ok) {
            state = { state: 'unavailable', reason: 'unknown_completion' };
            notify();
            return { status: 'unknown_completion' as const };
          }
          if (readback.value.wire !== wire) {
            state = { state: 'unavailable', reason: 'conflict' };
            notify();
            return { status: 'conflict' as const };
          }
          state = {
            state: 'ready',
            owner: original.owner,
            revision: next.revision,
            peers: next.peers.slice()
          };
          notify();
          return { status: 'saved' as const };
        }
      );
    } catch {
      if (!original.current()) return { status: 'stopped' };
      state = { state: 'unavailable', reason: 'unknown_completion' };
      notify();
      return { status: 'unknown_completion' };
    }
  }
  controllers.set(token, {
    current: owned,
    snapshot,
    async mount() {
      if (closed || mounted || typeof window === 'undefined') return;
      mounted = true;
      await refresh();
    },
    refresh,
    async block(room, unlocked, review) {
      const message = conversationSnapshot(room);
      const cache = unlockedSessionOwnership(unlocked);
      const original = scope;
      if (
        !message ||
        !cache?.current() ||
        cache.owner !== capture.owner ||
        cache.session !== capture.session ||
        message.owner !== capture.owner ||
        !canonicalPublicKey(message.peer) ||
        review !== 'reviewed_local_private_block' ||
        !unlockedConversationCurrent(unlocked, room)
      )
        return { status: 'invalid' };
      if (!original || state.state !== 'ready')
        return { status: 'unavailable' };
      return change(message.peer, true, {
        ...original,
        current: () =>
          original.current() &&
          cache.current() &&
          unlockedConversationCurrent(unlocked, room)
      });
    },
    async undo(peer, review) {
      const key = canonicalPublicKey(peer);
      if (!key || review !== 'reviewed_local_private_undo')
        return { status: 'invalid' };
      const original = scope;
      if (!original || state.state !== 'ready')
        return { status: 'unavailable' };
      if (!state.peers.includes(key)) return { status: 'invalid' };
      return change(key, false, original);
    },
    subscribe(listener) {
      if (closed) return () => {};
      const id = Symbol();
      listeners.set(id, listener);
      return () => {
        listeners.delete(id);
      };
    },
    close() {
      if (closed) return;
      closed = true;
      retire();
      notify();
      listeners.clear();
    }
  });
  return token;
}
export function localPrivateBlocksSnapshot(
  token: LocalPrivateBlocks
): PrivateBlocksSnapshot {
  return controllers.get(token)?.snapshot() ?? { state: 'unavailable' };
}
export function localPrivatePeerAllowed(
  token: LocalPrivateBlocks,
  peer: string
): boolean {
  const owned = controllers.get(token);
  if (!owned) return false;
  const state = owned.snapshot();
  return state.state === 'ready' && !state.peers.includes(peer);
}
export function mountLocalPrivateBlocks(
  token: LocalPrivateBlocks
): Promise<void> {
  return controllers.get(token)?.mount() ?? Promise.resolve();
}
export function refreshLocalPrivateBlocks(
  token: LocalPrivateBlocks
): Promise<void> {
  return controllers.get(token)?.refresh() ?? Promise.resolve();
}
export function subscribeLocalPrivateBlocks(
  token: LocalPrivateBlocks,
  listener: () => void
): () => void {
  return controllers.get(token)?.subscribe(listener) ?? (() => {});
}
export function blockLocalPrivatePeer(
  token: LocalPrivateBlocks,
  room: AdmittedConversation,
  unlocked: UnlockedSession,
  review: unknown
): Promise<PrivateBlockResult> {
  return (
    controllers.get(token)?.block(room, unlocked, review) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function undoLocalPrivatePeer(
  token: LocalPrivateBlocks,
  peer: unknown,
  review: unknown
): Promise<PrivateBlockResult> {
  return (
    controllers.get(token)?.undo(peer, review) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function closeLocalPrivateBlocks(token: LocalPrivateBlocks): void {
  controllers.get(token)?.close();
}

const identityControllers = new WeakMap<IdentitySession, LocalPrivateBlocks>();
export function privateBlocksForIdentity(
  identity: IdentitySession
): LocalPrivateBlocks | undefined {
  const prior = identityControllers.get(identity);
  if (prior && controllers.get(prior)?.current()) return prior;
  if (prior) closeLocalPrivateBlocks(prior);
  const token = capturePrivateBlocks(identity);
  if (token) identityControllers.set(identity, token);
  return token;
}
declare const viewBrand: unique symbol;
export type LocalPrivateBlocksView = Readonly<{ [viewBrand]: true }>;
const viewControllers = new WeakMap<
  LocalPrivateBlocksView,
  {
    snapshot(): PrivateBlocksSnapshot;
    mount(): Promise<void>;
    subscribe(listener: () => void): () => void;
    undo(peer: unknown, review: unknown): Promise<PrivateBlockResult>;
    close(): void;
  }
>();
// Root construction stays SSR-pure. Original identity sessions share the same
// preference owner with Inbox controllers; route disposal cannot close another view.
export function createLocalPrivateBlocks(
  context: IdentityViewContext
): LocalPrivateBlocksView {
  return createPrivateBlocksView(
    () => identityViewSession(context),
    (listener) => {
      let initial = true;
      return subscribeIdentityView(context, () => {
        if (initial) {
          initial = false;
          return;
        }
        listener();
      });
    }
  );
}
// Original identity-owned consumers use the same registry as the root context;
// a copied session still cannot acquire an account or preference controller.
export function createIdentityLocalPrivateBlocksView(
  identity: IdentitySession
): LocalPrivateBlocksView {
  return createPrivateBlocksView(
    () => identity,
    (listener) => subscribeIdentityInvalidation(identity, listener)
  );
}
function createPrivateBlocksView(
  sessionOf: () => IdentitySession | undefined,
  observe: (listener: () => void) => () => void
): LocalPrivateBlocksView {
  const view = Object.freeze({}) as LocalPrivateBlocksView;
  const listeners = new Map<symbol, () => void>();
  let token: LocalPrivateBlocks | undefined,
    mounted = false,
    closed = false;
  let off = () => {},
    offToken = () => {};
  const notify = () => {
    for (const listener of listeners.values()) {
      try {
        listener();
      } catch {
        /* Isolate presentation. */
      }
    }
  };
  async function bind() {
    offToken();
    offToken = () => {};
    const session = sessionOf();
    token = session ? privateBlocksForIdentity(session) : undefined;
    if (token) {
      const original = token;
      offToken = subscribeLocalPrivateBlocks(original, notify);
      await mountLocalPrivateBlocks(original);
    }
    if (!closed) notify();
  }
  viewControllers.set(view, {
    snapshot: () =>
      closed
        ? { state: 'unavailable', reason: 'stopped' }
        : token
          ? localPrivateBlocksSnapshot(token)
          : { state: 'guest' },
    async mount() {
      if (closed || mounted || typeof window === 'undefined') return;
      mounted = true;
      off = observe(() => {
        void bind();
      });
      await bind();
    },
    subscribe(listener) {
      const key = Symbol();
      listeners.set(key, listener);
      return () => {
        listeners.delete(key);
      };
    },
    undo: (peer, review) =>
      token
        ? undoLocalPrivatePeer(token, peer, review)
        : Promise.resolve({ status: 'invalid' }),
    close() {
      if (closed) return;
      closed = true;
      off();
      offToken();
      token = undefined;
      notify();
      listeners.clear();
    }
  });
  return view;
}
export function mountLocalPrivateBlocksView(
  view: LocalPrivateBlocksView
): Promise<void> {
  return viewControllers.get(view)?.mount() ?? Promise.resolve();
}
export function localPrivateBlocksViewSnapshot(
  view: LocalPrivateBlocksView
): PrivateBlocksSnapshot {
  return viewControllers.get(view)?.snapshot() ?? { state: 'unavailable' };
}
export function subscribeLocalPrivateBlocksView(
  view: LocalPrivateBlocksView,
  listener: () => void
): () => void {
  return viewControllers.get(view)?.subscribe(listener) ?? (() => {});
}
export function undoLocalPrivateBlocksView(
  view: LocalPrivateBlocksView,
  peer: unknown,
  review: unknown
): Promise<PrivateBlockResult> {
  return (
    viewControllers.get(view)?.undo(peer, review) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function closeLocalPrivateBlocksView(
  view: LocalPrivateBlocksView
): void {
  viewControllers.get(view)?.close();
}
