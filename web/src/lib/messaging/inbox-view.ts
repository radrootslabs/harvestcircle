import {
  identityMessagingOwnership,
  identitySessionSnapshot,
  subscribeIdentityInvalidation,
  type IdentitySession,
  type IdentitySnapshot
} from '../runtime/identity-session.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  closePrivateSession,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  inboxSetupViewOwnership,
  inboxSetupViewSnapshot,
  subscribeInboxSetupView,
  closeInboxSetupView,
  unlockInboxSetupView,
  observeInboxSetupView,
  type InboxSetupView
} from './inbox-setup-view.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  type BrowserDatabase
} from '../persistence/database.ts';
import {
  createPrivateStorageRepository,
  type PrivateStorageRepository
} from '../persistence/private-storage.ts';
import {
  captureUnlockedSession,
  closeUnlockedSession,
  readUnlockedMessages,
  type UnlockedSession
} from './unlocked-session.ts';
import {
  captureDecryptionQueue,
  refreshDecryptionQueue,
  runDecryptionBatch,
  decryptionQueueSnapshot,
  stopDecryptionQueue,
  type DecryptionQueue
} from './decryption-queue.ts';
import {
  captureInboxSync,
  startInboxSync,
  refreshInboxSync,
  inboxSyncSnapshot,
  inboxSyncHistoryOwnership,
  stopInboxSync,
  type InboxSync
} from './inbox-sync.ts';
import { messagingReadinessSnapshot } from './readiness.ts';
declare const inboxViewBrand: unique symbol;
export type InboxView = Readonly<{ [inboxViewBrand]: true }>;
export type InboxViewSnapshot = Readonly<{
  state: string;
  reason: string | null;
  busy: boolean;
  owner?: string;
  identity: IdentitySnapshot;
  queued: number;
  count: number;
  lastCheckedAt: number | null;
  historyComplete: false;
  cleanupRequired: boolean;
}>;
type Inputs = Readonly<{ identity: IdentitySession; setup: InboxSetupView }>;
type Controller = {
  snapshot(): InboxViewSnapshot;
  setup(): InboxSetupView | undefined;
  subscribe(listener: (view: InboxViewSnapshot) => void): () => void;
  unlock(review: unknown): Promise<boolean>;
  check(review: unknown): Promise<boolean>;
  read(review: unknown): Promise<boolean>;
  stop(): void;
  close(): boolean;
};
const views = new WeakMap<InboxView, Controller>();
const unavailable = (): InboxViewSnapshot => ({
  state: 'unavailable',
  reason: null,
  busy: false,
  identity: { state: 'guest', reason: 'unavailable' },
  queued: 0,
  count: 0,
  lastCheckedAt: null,
  historyComplete: false,
  cleanupRequired: false
});
// The original setup/identity objects, not their copied readiness projections,
// own this per-page orchestration. Mounting/subscribing never prompts or sends.
export function createInboxView(input: Inputs): InboxView | undefined {
  if (typeof window === 'undefined') return;
  const original = identityMessagingOwnership(input.identity),
    setup = inboxSetupViewOwnership(input.setup);
  if (
    !original?.current() ||
    !setup?.current() ||
    setup.identity !== input.identity
  )
    return;
  const capture = original,
    token = Object.freeze({}) as InboxView;
  let closed = false,
    busy = false,
    epoch = 0,
    reason: string | null = null,
    lastCheckedAt: number | null = null,
    checkPending = false,
    sampling = false,
    refreshedRetained = -1;
  let session: PrivateSession | undefined,
    database: BrowserDatabase | undefined,
    repository: PrivateStorageRepository | undefined,
    unlocked: UnlockedSession | undefined,
    queue: DecryptionQueue | undefined,
    sync: InboxSync | undefined;
  let off = () => {},
    privateOff = () => {},
    setupOff = () => {},
    pendingCleanup = Array.from<() => void>([]);
  const listeners = new Map<symbol, (view: InboxViewSnapshot) => void>();
  function current(generation = epoch) {
    const owner = inboxSetupViewOwnership(input.setup);
    return (
      !closed &&
      generation === epoch &&
      capture.current() &&
      owner?.identity === input.identity &&
      owner.current()
    );
  }
  function privateCurrent() {
    const owner = session && privateSessionOwnership(session);
    return (
      !!owner?.current() &&
      owner.owner === capture.owner &&
      owner.session === capture.session &&
      current()
    );
  }
  function snapshot(): InboxViewSnapshot {
    if (!current())
      return { ...unavailable(), cleanupRequired: pendingCleanup.length > 0 };
    const identity = identitySessionSnapshot(input.identity),
      setupState = inboxSetupViewSnapshot(input.setup);
    if (!privateCurrent() || !unlocked || !queue)
      return {
        ...unavailable(),
        owner: capture.owner,
        identity,
        state: busy
          ? 'unlocking'
          : [
                'compatible',
                'compatible_existing',
                'access_unavailable',
                'verified'
              ].includes(setupState.status)
            ? 'locked'
            : 'setup',
        reason,
        busy: busy || setupState.busy
      };
    const cached = readUnlockedMessages(unlocked),
      decrypt = decryptionQueueSnapshot(queue),
      transport = sync && inboxSyncSnapshot(sync);
    const scope = inboxSetupViewOwnership(input.setup);
    const exercised =
      !!scope?.resolver &&
      messagingReadinessSnapshot(
        input.identity,
        session,
        scope.policy,
        scope.resolver,
        undefined,
        scope.observeAccess
      ).ownAccess === 'qualified_exercised';
    if (!privateCurrent()) return unavailable();
    const partial =
      reason !== null ||
      !exercised ||
      cached.status !== 'ready' ||
      decrypt.state === 'paused' ||
      transport?.state === 'needs_action' ||
      transport?.state === 'stopped';
    const checked =
      lastCheckedAt !== null && !!sync && !!inboxSyncHistoryOwnership(sync);
    return {
      state:
        busy || decrypt.busy
          ? 'unlocking'
          : partial
            ? 'partial'
            : checked && decrypt.queued === 0 && cached.messages.length === 0
              ? 'empty'
              : 'ready',
      reason:
        reason ??
        decrypt.reason ??
        transport?.reason ??
        (!exercised ? 'access_unavailable' : null),
      busy: busy || decrypt.busy || setupState.busy,
      owner: capture.owner,
      identity,
      queued: decrypt.queued,
      count: cached.messages.length,
      lastCheckedAt,
      historyComplete: false,
      cleanupRequired: pendingCleanup.length > 0
    };
  }
  function notify() {
    for (const listener of listeners.values())
      try {
        listener(snapshot());
      } catch {
        /* Isolate presentation. */
      }
  }
  function releasePrivate() {
    privateOff();
    privateOff = () => {};
    const originalQueue = queue,
      originalSync = sync,
      originalUnlocked = unlocked,
      originalSession = session,
      originalDatabase = database;
    const tasks = pendingCleanup.concat([
      () => {
        if (originalQueue) stopDecryptionQueue(originalQueue);
      },
      () => {
        if (originalSync) stopInboxSync(originalSync);
      },
      () => {
        if (originalUnlocked) closeUnlockedSession(originalUnlocked);
      },
      () => {
        if (originalSession && !closePrivateSession(originalSession))
          throw Error('private_cleanup_required');
      },
      () => {
        if (originalDatabase) closeBrowserDatabase(originalDatabase);
      }
    ]);
    pendingCleanup = [];
    for (const cleanup of tasks)
      try {
        cleanup();
      } catch {
        pendingCleanup = pendingCleanup.concat(cleanup);
      }
    if (pendingCleanup.length > 0) reason = 'cleanup_required';
    queue = undefined;
    sync = undefined;
    unlocked = undefined;
    session = undefined;
    repository = undefined;
    database = undefined;
    lastCheckedAt = null;
    checkPending = false;
  }
  function stop() {
    epoch++;
    reason = 'stopped';
    releasePrivate();
    busy = false;
    notify();
  }
  async function sample() {
    if (sampling || closed) return;
    if (!current()) {
      notify();
      return;
    }
    observeInboxSetupView(input.setup);
    if (!queue || !privateCurrent()) {
      notify();
      return;
    }
    sampling = true;
    const generation = epoch;
    try {
      const transport = sync && inboxSyncSnapshot(sync);
      // Read native ciphertext only after actual ingress changes, never on
      // every idle presentation tick and never as automatic SDK decryption.
      if (
        transport &&
        transport.pending === 0 &&
        transport.retained !== refreshedRetained &&
        !decryptionQueueSnapshot(queue).busy
      ) {
        const retained = transport.retained;
        const refreshed = await refreshDecryptionQueue(queue);
        if (!current(generation) || !privateCurrent()) return;
        refreshedRetained = retained;
        if (!refreshed) {
          notify();
          return;
        }
      }
      if (!current(generation) || !privateCurrent()) return;
      if (
        checkPending &&
        sync &&
        transport?.backfill === 'complete' &&
        transport.pending === 0 &&
        inboxSyncHistoryOwnership(sync)?.current()
      ) {
        const checked = Date.now();
        if (
          current(generation) &&
          privateCurrent() &&
          inboxSyncHistoryOwnership(sync)?.current()
        ) {
          lastCheckedAt = checked;
          checkPending = false;
        }
      }
      notify();
    } catch {
      if (current(generation)) {
        reason = 'unavailable';
        notify();
      }
    } finally {
      sampling = false;
    }
  }
  async function read(reviewed: unknown) {
    if (
      reviewed !== 'reviewed_decrypt_batch' ||
      busy ||
      !queue ||
      !privateCurrent()
    )
      return false;
    busy = true;
    reason = null;
    const generation = epoch;
    notify();
    try {
      const result = await runDecryptionBatch(queue, reviewed);
      if (!current(generation) || !privateCurrent()) return false;
      return result;
    } finally {
      if (current(generation)) {
        busy = false;
        notify();
      }
    }
  }
  async function unlock(reviewed: unknown) {
    if (reviewed !== 'reviewed_messages_unlock' || busy || !current())
      return false;
    if (queue && privateCurrent()) return await read('reviewed_decrypt_batch');
    const setupState = inboxSetupViewSnapshot(input.setup);
    if (
      ![
        'compatible',
        'compatible_existing',
        'access_unavailable',
        'verified'
      ].includes(setupState.status)
    )
      return false;
    busy = true;
    reason = null;
    const generation = epoch;
    notify();
    try {
      await unlockInboxSetupView(input.setup);
      if (!current(generation)) return false;
      const original = inboxSetupViewOwnership(input.setup),
        acquired = original?.session,
        owner = acquired && privateSessionOwnership(acquired);
      if (
        !original?.resolver ||
        !acquired ||
        !owner?.current() ||
        owner.owner !== capture.owner ||
        owner.session !== capture.session ||
        !original.current()
      )
        return false;
      session = acquired;
      const opened = await openBrowserDatabase();
      if (!current(generation) || !privateCurrent()) {
        if (opened.state === 'ready') closeBrowserDatabase(opened.owner);
        return false;
      }
      if (opened.state !== 'ready') {
        reason = 'storage_unavailable';
        return false;
      }
      database = opened.owner;
      repository = createPrivateStorageRepository(database, capture.owner);
      unlocked = captureUnlockedSession(session, reviewed);
      queue =
        repository &&
        unlocked &&
        captureDecryptionQueue(
          repository,
          session,
          input.identity,
          unlocked,
          'reviewed_inbox_unlock'
        );
      if (!queue) {
        reason = 'unavailable';
        return false;
      }
      privateOff = subscribePrivateSessionClose(session, () => {
        epoch++;
        releasePrivate();
        busy = false;
        notify();
        if (pendingCleanup.length > 0)
          queueMicrotask(() => {
            releasePrivate();
            notify();
          });
      });
      if (!privateCurrent()) return false;
      const result = await runDecryptionBatch(queue, 'reviewed_decrypt_batch');
      if (!current(generation) || !privateCurrent()) return false;
      return result;
    } catch {
      if (current(generation)) reason = 'unavailable';
      return false;
    } finally {
      if (current(generation)) {
        busy = false;
        notify();
      }
    }
  }
  async function check(reviewed: unknown) {
    if (
      reviewed !== 'reviewed_foreground_inbox' ||
      busy ||
      !privateCurrent() ||
      !session ||
      !repository ||
      !unlocked
    )
      return false;
    const scope = inboxSetupViewOwnership(input.setup);
    if (!scope?.resolver || !scope.current() || !scope.observeAccess) {
      reason = 'access_unavailable';
      notify();
      return false;
    }
    reason = null;
    if (sync) {
      if (
        !refreshInboxSync(
          sync,
          reviewed,
          lastCheckedAt === null ? undefined : Math.floor(lastCheckedAt / 1000)
        )
      ) {
        reason = inboxSyncSnapshot(sync)?.reason ?? 'check_incomplete';
        notify();
        return false;
      }
    } else {
      sync = captureInboxSync(
        repository,
        session,
        scope.policy,
        scope.resolver,
        unlocked,
        scope.observeAccess,
        reviewed
      );
      if (!sync || !startInboxSync(sync, reviewed)) {
        reason = 'access_unavailable';
        notify();
        return false;
      }
    }
    checkPending = true;
    notify();
    await sample();
    return privateCurrent();
  }
  views.set(token, {
    snapshot,
    setup: () => (current() ? input.setup : undefined),
    subscribe(listener) {
      const key = Symbol();
      listeners.set(key, listener);
      listener(snapshot());
      return () => {
        listeners.delete(key);
      };
    },
    unlock,
    check,
    read,
    stop,
    close() {
      if (!closed) {
        closed = true;
        epoch++;
        clearInterval(timer);
        off();
        setupOff();
      }
      releasePrivate();
      // Cleanup retains the original genuine setup independently of current
      // admission. Logout hides effect access, while disposal still closes
      // its identity observer, database and runtime public-view owner.
      try {
        closeInboxSetupView(input.setup);
      } catch {
        pendingCleanup = pendingCleanup.concat(() =>
          closeInboxSetupView(input.setup)
        );
      }
      listeners.clear();
      return pendingCleanup.length === 0;
    }
  });
  off = subscribeIdentityInvalidation(input.identity, () => {
    if (!capture.current()) {
      epoch++;
      releasePrivate();
      busy = false;
      notify();
    }
  });
  setupOff = subscribeInboxSetupView(input.setup, notify);
  const timer = setInterval(() => {
    void sample();
  }, 200);
  if (!current()) {
    views.get(token)?.close();
    return;
  }
  return token;
}
export function inboxViewSnapshot(
  token: InboxView | undefined
): InboxViewSnapshot {
  return (token && views.get(token)?.snapshot()) || unavailable();
}
export function inboxViewSetup(
  token: InboxView | undefined
): InboxSetupView | undefined {
  return token && views.get(token)?.setup();
}
export function subscribeInboxView(
  token: InboxView | undefined,
  listener: (view: InboxViewSnapshot) => void
): () => void {
  return (token && views.get(token)?.subscribe(listener)) || (() => {});
}
export function unlockInboxView(
  token: InboxView,
  review: unknown
): Promise<boolean> {
  return views.get(token)?.unlock(review) ?? Promise.resolve(false);
}
export function checkInboxView(
  token: InboxView,
  review: unknown
): Promise<boolean> {
  return views.get(token)?.check(review) ?? Promise.resolve(false);
}
export function readNewInboxView(
  token: InboxView,
  review: unknown
): Promise<boolean> {
  return views.get(token)?.read(review) ?? Promise.resolve(false);
}
export function stopInboxView(token: InboxView): void {
  views.get(token)?.stop();
}
export function closeInboxView(token: InboxView): boolean {
  return views.get(token)?.close() ?? true;
}
