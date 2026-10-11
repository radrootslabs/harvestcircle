import { canonicalPublicKey } from '../contracts/public-key.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../nostr/verified-envelope.ts';
import {
  identityLocalOwnership,
  subscribeIdentityInvalidation
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

declare const hidesBrand: unique symbol;
export type LocalPublicHides = Readonly<{ [hidesBrand]: true }>;
export const LOCAL_PUBLIC_HIDES_CONTEXT = 'harvestcircle_local_public_hides';
export const PUBLIC_HIDE_DISCLOSURE =
  'Hiding a publisher is local to this account in this browser. It does not delete listings, remote copies or your publication receipts, and sends no public report. Undo restores eligible listings; operator policy still applies. Browser storage is not a backup.';
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
export type PublicHidesSnapshot =
  | Readonly<{ state: 'guest' | 'loading' }>
  | Readonly<{ state: 'unavailable'; reason?: Failure }>
  | Readonly<{
      state: 'ready';
      owner: string;
      revision: number;
      publishers: readonly string[];
    }>;
type Result<T> =
  Readonly<{ ok: true; value: T }> | Readonly<{ ok: false; reason: Failure }>;
type Row = {
  schema: 1;
  family: 'public_hides';
  owner: string;
  id: 'public_hides';
  revision: number;
  publishers: string[];
};
type Stored = { row: Row; wire: string | null };
type Scope = { owner: string; database: BrowserDatabase; current(): boolean };
export type PublicHideResult = Readonly<{
  status: 'saved' | 'existing' | Failure;
}>;
type Controller = {
  snapshot(): PublicHidesSnapshot;
  mount(): Promise<void>;
  refresh(): Promise<void>;
  hide(proof: VerifiedEnvelope, review: unknown): Promise<PublicHideResult>;
  undo(publisher: unknown, review: unknown): Promise<PublicHideResult>;
  subscribe(listener: () => void): () => void;
  close(): void;
};
const controllers = new WeakMap<LocalPublicHides, Controller>();
const failed = (reason: Failure): Result<never> => ({ ok: false, reason });
function decode(value: unknown, owner: string): Stored | undefined {
  if (value === undefined)
    return {
      row: {
        schema: 1,
        family: 'public_hides',
        owner,
        id: 'public_hides',
        revision: 0,
        publishers: []
      },
      wire: null
    };
  if (
    !exactLocalFields(value, ['owner', 'id', 'wire']) ||
    value.owner !== owner ||
    value.id !== 'public_hides' ||
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
      'publishers'
    ]) ||
    row.schema !== 1 ||
    row.family !== 'public_hides' ||
    row.owner !== owner ||
    row.id !== 'public_hides' ||
    !Number.isSafeInteger(row.revision) ||
    typeof row.revision !== 'number' ||
    row.revision < 1 ||
    !Array.isArray(row.publishers) ||
    row.publishers.length > LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes ||
    row.publishers.some(
      (key: unknown) =>
        typeof key !== 'string' || canonicalPublicKey(key) !== key
    ) ||
    new Set(row.publishers).size !== row.publishers.length ||
    row.publishers.some(
      (key: string, index: number, publishers: string[]) =>
        index > 0 && key <= publishers[index - 1]
    )
  )
    return;
  const admitted: Row = {
    schema: 1,
    family: 'public_hides',
    owner,
    id: 'public_hides',
    revision: row.revision,
    publishers: row.publishers.slice() as string[]
  };
  return JSON.stringify(admitted) === value.wire
    ? { row: admitted, wire: value.wire }
    : undefined;
}
// One fixed owner/key record, with bounded publisher entries. This avoids
// scanning or adopting future private-block preferences in the same store.
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
        .get([scope.owner, 'public_hides']);
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
export function createLocalPublicHides(
  context: IdentityViewContext
): LocalPublicHides {
  const token = Object.freeze({}) as LocalPublicHides;
  const listeners = new Map<symbol, () => void>();
  let state: PublicHidesSnapshot,
    mounted = false,
    closed = false;
  state = { state: 'guest' };
  let scope: Scope | undefined,
    generation = Symbol(),
    cancel: AbortController | undefined;
  let offIdentity = () => {},
    offInvalidation = () => {};
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
    state = { state: 'guest' };
  }
  function snapshot(): PublicHidesSnapshot {
    if (closed) return { state: 'unavailable', reason: 'stopped' };
    if (scope && !scope.current()) {
      const session = identityViewSession(context);
      const connected = session && identityLocalOwnership(session)?.current();
      retire();
      if (connected) state = { state: 'unavailable', reason: 'unavailable' };
      notify();
    }
    return state.state === 'ready'
      ? { ...state, publishers: state.publishers.slice() }
      : { ...state };
  }
  async function refresh() {
    retire();
    notify();
    if (!mounted || closed) return;
    const session = identityViewSession(context),
      capture = session && identityLocalOwnership(session);
    if (!session || !capture?.current()) return;
    const original = generation;
    const current = () =>
      !closed &&
      mounted &&
      original === generation &&
      identityViewSession(context) === session &&
      capture.current();
    state = { state: 'loading' };
    notify();
    cancel = new AbortController();
    offInvalidation = subscribeIdentityInvalidation(session, () => {
      retire();
      notify();
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
          publishers: read.value.row.publishers.slice()
        }
      : { state: 'unavailable', reason: read.reason };
    notify();
  }
  async function change(
    publisher: string,
    hidden: boolean,
    original: Scope
  ): Promise<PublicHideResult> {
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
          if (!loaded.ok) return { status: loaded.reason };
          const before = loaded.value;
          if (before.row.publishers.includes(publisher) === hidden) {
            // Another controller may have acknowledged this complete native
            // row. Reconcile all preferences without another write/revision.
            state = {
              state: 'ready',
              owner: original.owner,
              revision: before.row.revision,
              publishers: before.row.publishers.slice()
            };
            notify();
            return { status: 'existing' as const };
          }
          if (
            before.row.revision >= Number.MAX_SAFE_INTEGER ||
            (hidden &&
              before.row.publishers.length >=
                LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes)
          )
            return { status: 'capacity' as const };
          const next: Row = {
            ...before.row,
            revision: before.row.revision + 1,
            publishers: hidden
              ? before.row.publishers.concat(publisher).sort()
              : before.row.publishers.filter((key) => key !== publisher)
          };
          const wire = JSON.stringify(next);
          const written = await transaction<boolean>(
            original,
            'readwrite',
            (stored, tx) => {
              if (stored.wire !== before.wire) return failed('conflict');
              tx.objectStore('local_preferences').put({
                owner: original.owner,
                id: 'public_hides',
                wire
              });
              return { ok: true, value: true };
            }
          );
          if (!original.current()) return { status: 'stopped' as const };
          if (!written.ok) return { status: written.reason };
          const readback = await transaction(
            original,
            'readonly',
            (stored) => ({ ok: true, value: stored })
          );
          if (!original.current()) return { status: 'stopped' as const };
          if (!readback.ok) return { status: 'unknown_completion' as const };
          if (readback.value.wire !== wire)
            return { status: 'conflict' as const };
          state = {
            state: 'ready',
            owner: original.owner,
            revision: next.revision,
            publishers: next.publishers.slice()
          };
          notify();
          return { status: 'saved' as const };
        }
      );
    } catch {
      return { status: original.current() ? 'unknown_completion' : 'stopped' };
    }
  }
  controllers.set(token, {
    snapshot,
    async mount() {
      if (closed || mounted || typeof window === 'undefined') return;
      mounted = true;
      // Subscribe before initial load; pending/guest transitions revoke the
      // previous generation before an asynchronous open can expose its rows.
      let initial = true;
      offIdentity = subscribeIdentityView(context, () => {
        if (initial) {
          initial = false;
          return;
        }
        void refresh();
      });
      await refresh();
    },
    refresh,
    async hide(proof, review) {
      const event = verifiedEnvelopeSnapshot(proof);
      if (
        !event ||
        event.kind !== 30402 ||
        !canonicalPublicKey(event.pubkey) ||
        review !== 'reviewed_local_public_hide'
      )
        return { status: 'invalid' };
      const original = scope;
      if (!original || state.state !== 'ready')
        return { status: 'unavailable' };
      return change(event.pubkey, true, original);
    },
    async undo(publisher, review) {
      const key = canonicalPublicKey(publisher);
      if (!key || review !== 'reviewed_local_public_undo')
        return { status: 'invalid' };
      const original = scope;
      if (!original || state.state !== 'ready')
        return { status: 'unavailable' };
      if (!state.publishers.includes(key)) return { status: 'invalid' };
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
      offIdentity();
      retire();
      notify();
      listeners.clear();
    }
  });
  return token;
}
export function localPublicHidesSnapshot(
  token: LocalPublicHides
): PublicHidesSnapshot {
  return controllers.get(token)?.snapshot() ?? { state: 'unavailable' };
}
export function localPublicPublisherAllowed(
  token: LocalPublicHides,
  publisher: string
): boolean {
  const owned = controllers.get(token);
  if (!owned) return false;
  const state = owned.snapshot();
  return state.state === 'loading' ||
    (state.state === 'unavailable' && state.reason === 'stopped')
    ? false
    : state.state !== 'ready' || !state.publishers.includes(publisher);
}
export function mountLocalPublicHides(token: LocalPublicHides): Promise<void> {
  return controllers.get(token)?.mount() ?? Promise.resolve();
}
export function refreshLocalPublicHides(
  token: LocalPublicHides
): Promise<void> {
  return controllers.get(token)?.refresh() ?? Promise.resolve();
}
export function subscribeLocalPublicHides(
  token: LocalPublicHides,
  listener: () => void
): () => void {
  return controllers.get(token)?.subscribe(listener) ?? (() => {});
}
export function hideLocalPublicPublisher(
  token: LocalPublicHides,
  proof: VerifiedEnvelope,
  review: unknown
): Promise<PublicHideResult> {
  return (
    controllers.get(token)?.hide(proof, review) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function undoLocalPublicPublisher(
  token: LocalPublicHides,
  publisher: unknown,
  review: unknown
): Promise<PublicHideResult> {
  return (
    controllers.get(token)?.undo(publisher, review) ??
    Promise.resolve({ status: 'invalid' })
  );
}
export function closeLocalPublicHides(token: LocalPublicHides): void {
  controllers.get(token)?.close();
}
