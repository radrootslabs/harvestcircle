import { makeRetryFixture } from './inbox-probe-retry.ts';
import { makeFixture as makeCheckCryptoFixture } from './approved-signing.ts';
import {
  withIdlePrivateLocalStorage,
  createPrivateSession,
  closePrivateSession,
  type PrivateSession
} from '../../../src/lib/runtime/private-session.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import {
  installHostedProvider,
  openRecoveryContext,
  validateRelayPolicy,
  validateInboxOperatorQualification
} from './inbox-probe-recovery.ts';
import { inboxOperatorQualificationSnapshot } from '../../../src/lib/config/inbox-operators.ts';
import {
  createPublicView,
  beginPublicViewRun,
  disposePublicView
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  resolveInboxPreference,
  settleInboxResolver
} from '../../../src/lib/messaging/resolve-inbox.ts';
import { captureUnlockedSession } from '../../../src/lib/messaging/unlocked-session.ts';
import {
  captureInboxSync,
  startInboxSync,
  stopInboxSync,
  inboxSyncSnapshot,
  type InboxSync
} from '../../../src/lib/messaging/inbox-sync.ts';
import { observeQualifiedInboxAccess } from '../../../src/lib/messaging/inbox-access-qualification.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  captureDecryptionQueue,
  runDecryptionBatch,
  decryptionQueueSnapshot,
  decryptionQueueConversations,
  stopDecryptionQueue
} from '../../../src/lib/messaging/decryption-queue.ts';
import {
  createInboxAccessCheck,
  inboxAccessCheckSnapshot,
  inboxAccessCheckQualification,
  inboxAccessCheckReviewSnapshot,
  reviewInboxAccessCheck,
  runInboxAccessCheck,
  cleanupInboxAccessCheck,
  reviewInboxAccessCheckCleanup,
  reviewInboxAccessCheckReconciliation,
  reconcileInboxAccessCheck,
  stopInboxAccessCheck,
  retryInboxAccessCheckCleanup,
  type InboxAccessCheck,
  type InboxAccessCheckReview
} from '../../../src/lib/messaging/inbox-access-check.ts';
import {
  createInboxAccessController,
  closeInboxAccessController
} from '../../../src/lib/messaging/inbox-access-controller.ts';
export * from '../../../src/lib/messaging/inbox-access-check.ts';
export * from '../../../src/lib/messaging/inbox-access-controller.ts';
export * from '../../../src/lib/messaging/inbox-access-qualification.ts';
export * from './inbox-probe-retry.ts';
export { decodePrivateRecord } from '../../../src/lib/persistence/private-records.ts';
export function otherCheckOwner() {
  const fixture = makeCheckCryptoFixture();
  try {
    return fixture.owner;
  } finally {
    fixture.close();
  }
}
export async function makeCheckFixture(authRequired = false) {
  // Only inert host/context setup and open() are reused. The new coordinator
  // must perform every preparation, network, nonce and qualification step.
  const base = await makeRetryFixture();
  if (authRequired) {
    const wire = inboxOperatorQualificationSnapshot(base.host.qualification)!;
    const operator = validateInboxOperatorQualification(
      JSON.stringify({
        ...wire,
        relays: wire.relays.map((relay) =>
          relay.origin === 'wss://inbox.example.org'
            ? { ...relay, auth: 'explicit_required' }
            : relay
        )
      })
    );
    if (!operator) throw Error('actual explicit AUTH operator declaration');
    base.host.qualification = operator;
  }
  let check: InboxAccessCheck | undefined;
  let selected: InboxAccessCheckReview | undefined;
  let sync: InboxSync | undefined;
  const views: ReturnType<typeof createPublicView>[] = [];
  const alternates = new Set<() => boolean>();
  return {
    ...base,
    check: () => check,
    review: () => selected,
    async open() {
      await base.open();
      const a = base.actors();
      check = createInboxAccessCheck(
        base.repository,
        a.identity,
        a.session,
        base.host.policy,
        a.access,
        base.host.qualification
      );
      if (!check) throw Error('actual source check capture');
    },
    snapshot: () => check && inboxAccessCheckSnapshot(check),
    qualification: () => check && inboxAccessCheckQualification(check),
    async alternateActors() {
      const identity = createIdentitySession();
      await connectIdentity(identity);
      await probeIdentityMessaging(identity, 'reviewed_self_copy');
      const session = await createPrivateSession(
        identity,
        'reviewed_private_session'
      );
      if (!session) throw Error('actual alternate private session missing');
      const view = createPublicView(base.sharedPublicRuntime());
      const resolver = resolveInboxPreference(
        view,
        beginPublicViewRun(view),
        base.host.owner
      );
      await settleInboxResolver(resolver);
      const access = createInboxAccessController(
        session,
        base.host.policy,
        resolver
      );
      if (!access) throw Error('actual alternate C7 missing');
      function close() {
        const c7 = closeInboxAccessController(access!);
        const actor = closePrivateSession(session!);
        disconnectIdentity(identity);
        disposePublicView(view);
        if (c7 && actor) alternates.delete(close);
        return c7 && actor;
      }
      alternates.add(close);
      return { identity, session, access, close };
    },
    async startOwnInbox() {
      const qualified = check && inboxAccessCheckQualification(check);
      if (!qualified) throw Error('actual coordinator qualification missing');
      const a = base.actors(),
        view = createPublicView(base.sharedPublicRuntime());
      views.push(view);
      const resolver = resolveInboxPreference(
        view,
        beginPublicViewRun(view),
        base.host.owner
      );
      await settleInboxResolver(resolver);
      const unlocked = captureUnlockedSession(
        a.session,
        'reviewed_messages_unlock'
      );
      if (!unlocked) throw Error('actual unlocked scope missing');
      sync = captureInboxSync(
        base.repository,
        a.session,
        base.host.policy,
        resolver,
        unlocked,
        observeQualifiedInboxAccess(qualified),
        'reviewed_foreground_inbox'
      );
      return !!sync && startInboxSync(sync, 'reviewed_foreground_inbox');
    },
    syncSnapshot: () => sync && inboxSyncSnapshot(sync),
    stopOwnInbox: () => {
      if (sync) stopInboxSync(sync);
    },
    async decryptReceived() {
      const a = base.actors(),
        unlocked = captureUnlockedSession(
          a.session,
          'reviewed_messages_unlock'
        );
      if (!unlocked) throw Error('actual explicit unlock missing');
      const queue = captureDecryptionQueue(
        base.repository,
        a.session,
        a.identity,
        unlocked,
        'reviewed_inbox_unlock'
      );
      if (!queue) throw Error('actual decryption queue missing');
      try {
        const result = await runDecryptionBatch(
          queue,
          'reviewed_decrypt_batch',
          []
        );
        return {
          result,
          snapshot: decryptionQueueSnapshot(queue),
          rooms: decryptionQueueConversations(queue).length
        };
      } finally {
        stopDecryptionQueue(queue);
      }
    },
    async replaceCheck(freshSession = false) {
      if (freshSession) await base.freshSession();
      else await base.replaceController();
      const a = base.actors();
      check = createInboxAccessCheck(
        base.repository,
        a.identity,
        a.session,
        base.host.policy,
        a.access,
        base.host.qualification
      );
      if (!check) throw Error('actual replacement check capture');
      selected = undefined;
    },
    select(
      mode: unknown = 'original',
      origin: unknown = 'wss://inbox.example.org',
      id?: unknown
    ) {
      if (!check) throw Error('actual coordinator missing');
      selected = reviewInboxAccessCheck(check, mode, origin, id);
      return selected && inboxAccessCheckReviewSnapshot(selected);
    },
    run(review: unknown = 'reviewed_inbox_access_check') {
      if (!selected) throw Error('actual source review missing');
      return runInboxAccessCheck(selected, review);
    },
    cleanup(review: unknown = 'reviewed_delete_local_inbox_probe') {
      if (!selected) throw Error('actual cleanup review missing');
      return cleanupInboxAccessCheck(selected, review);
    },
    selectCleanup() {
      if (!check) throw Error('actual coordinator missing');
      selected = reviewInboxAccessCheckCleanup(check);
      return selected && inboxAccessCheckReviewSnapshot(selected);
    },
    selectReconciliation() {
      if (!check) throw Error('actual coordinator missing');
      selected = reviewInboxAccessCheckReconciliation(check);
      return selected && inboxAccessCheckReviewSnapshot(selected);
    },
    reconcile(review: unknown = 'reviewed_inbox_access_check_reconciliation') {
      if (!selected) throw Error('actual reconciliation review missing');
      return reconcileInboxAccessCheck(selected, review);
    },
    stop: () => !!check && stopInboxAccessCheck(check),
    retryCleanup: () =>
      !!check &&
      retryInboxAccessCheckCleanup(
        check,
        'reviewed_inbox_access_check_cleanup_retry'
      ),
    close() {
      let failedAlternate = false;
      for (const close of alternates) if (!close()) failedAlternate = true;
      if (failedAlternate) throw Error('private_auth_cleanup_required');
      if (sync) stopInboxSync(sync);
      for (const view of views) disposePublicView(view);
      if (check && !stopInboxAccessCheck(check))
        throw Error('inbox_access_check_cleanup_required');
      base.close();
    }
  };
}

export function checkNativeLoss(
  phase: 'append' | 'nonce' | 'qualification',
  lose: () => void
) {
  const cursor = Object.getOwnPropertyDescriptor(
    IDBIndex.prototype,
    'openCursor'
  )!.value as IDBIndex['openCursor'];
  const put = Object.getOwnPropertyDescriptor(IDBObjectStore.prototype, 'put')!
    .value as IDBObjectStore['put'];
  let hits = 0;
  const nonce =
    phase === 'nonce'
      ? atCheckNonce(async () => {
          armed = true;
          await Promise.resolve();
        })
      : undefined;
  let armed = false;
  const lockRequest = Object.getOwnPropertyDescriptor(
    LockManager.prototype,
    'request'
  )!.value as LockManager['request'];
  if (phase === 'qualification') {
    navigator.locks.request = function (
      this: LockManager,
      ...args: Parameters<LockManager['request']>
    ) {
      // Observe the REAL qualification owner's lock admission; native event
      // callbacks intentionally do not retain their original async call stack.
      if (/qualifyInboxAccess/.test(new Error().stack ?? '')) armed = true;
      return lockRequest.apply(this, args);
    } as LockManager['request'];
  }
  IDBIndex.prototype.openCursor = function (
    ...args: Parameters<IDBIndex['openCursor']>
  ) {
    const request = cursor.apply(this, args);
    if (
      this.objectStore.name === 'private_sends' &&
      this.objectStore.transaction.mode === 'readonly' &&
      ((phase === 'nonce' && armed) || (phase === 'qualification' && armed))
    ) {
      request.addEventListener('success', () => {
        if (!hits && !request.result) {
          hits++;
          lose();
        }
      });
    }
    return request;
  };
  IDBObjectStore.prototype.put = function (
    ...args: Parameters<IDBObjectStore['put']>
  ) {
    const request = put.apply(this, args);
    const value = args[0] as { wire?: unknown };
    if (
      !hits &&
      phase === 'append' &&
      this.name === 'private_sends' &&
      typeof value?.wire === 'string'
    ) {
      const record = JSON.parse(value.wire) as {
        family?: string;
        receipts?: { role: string }[];
      };
      if (
        record.family === 'private_access_probe' &&
        record.receipts?.at(-1)?.role === 'access_archive'
      ) {
        request.addEventListener('success', () => {
          if (!hits) {
            hits++;
            lose();
          }
        });
      }
    }
    return request;
  };
  return {
    hits: () => hits,
    restore() {
      nonce?.restore();
      if (phase === 'qualification') navigator.locks.request = lockRequest;
      IDBIndex.prototype.openCursor = cursor;
      IDBObjectStore.prototype.put = put;
    }
  };
}

export function failCheckPreparationNative(
  phase: 'R0' | 'R1',
  mode: 'abort' | 'readback' | 'uncertain_readback'
) {
  const put = Object.getOwnPropertyDescriptor(IDBObjectStore.prototype, 'put')!
    .value as IDBObjectStore['put'];
  const abort = Object.getOwnPropertyDescriptor(
    IDBTransaction.prototype,
    'abort'
  )!.value as IDBTransaction['abort'];
  const cursor = Object.getOwnPropertyDescriptor(
    IDBIndex.prototype,
    'openCursor'
  )!.value as IDBIndex['openCursor'];
  let selected: IDBTransaction | undefined,
    candidate: { owner: string; id: string; wire: string } | undefined;
  let puts = 0,
    completes = 0,
    aborts = 0,
    abortRefusals = 0,
    readbackAborts = 0;
  IDBObjectStore.prototype.put = function (
    ...args: Parameters<IDBObjectStore['put']>
  ) {
    const request = put.apply(this, args),
      value = args[0] as { owner?: unknown; id?: unknown; wire?: unknown };
    if (
      !selected &&
      this.name === 'private_sends' &&
      typeof value?.wire === 'string'
    ) {
      const record = JSON.parse(value.wire) as {
        family?: string;
        self?: unknown;
        receipts?: unknown[];
      };
      if (
        record.family === 'private_access_probe' &&
        record.receipts?.length === 0 &&
        (phase === 'R0' ? record.self === null : record.self !== null)
      ) {
        if (typeof value.owner !== 'string' || typeof value.id !== 'string')
          throw Error('actual native preparation identity missing');
        candidate = { owner: value.owner, id: value.id, wire: value.wire };
        selected = this.transaction;
        selected.addEventListener('complete', () => {
          completes++;
        });
        selected.addEventListener('abort', () => {
          aborts++;
        });
        request.addEventListener('success', () => {
          puts++;
          if (mode === 'abort') selected!.abort();
        });
        if (mode === 'uncertain_readback')
          throw Error('actual admitted native write uncertainty');
      }
    }
    return request;
  };
  IDBTransaction.prototype.abort = function (
    ...args: Parameters<IDBTransaction['abort']>
  ) {
    if (this === selected && mode === 'uncertain_readback') {
      abortRefusals++;
      throw Error('actual admitted native abort refusal');
    }
    return abort.apply(this, args);
  };
  IDBIndex.prototype.openCursor = function (
    ...args: Parameters<IDBIndex['openCursor']>
  ) {
    const request = cursor.apply(this, args);
    if (
      mode !== 'abort' &&
      puts &&
      !readbackAborts &&
      this.objectStore.name === 'private_sends' &&
      this.objectStore.transaction.mode === 'readonly'
    ) {
      readbackAborts++;
      this.objectStore.transaction.abort();
    }
    return request;
  };
  return {
    counts: () => ({ puts, completes, aborts, abortRefusals, readbackAborts }),
    candidate: () => candidate,
    restore() {
      IDBObjectStore.prototype.put = put;
      IDBTransaction.prototype.abort = abort;
      IDBIndex.prototype.openCursor = cursor;
    }
  };
}

export function atCheckNonce(work: () => Promise<void>) {
  const original = Object.getOwnPropertyDescriptor(
    LockManager.prototype,
    'request'
  )!.value as LockManager['request'];
  let hits = 0;
  navigator.locks.request = async function (
    this: LockManager,
    ...args: Parameters<LockManager['request']>
  ) {
    if (
      !hits &&
      /read(?:Recovered)?InboxProbeNonce/.test(new Error().stack ?? '')
    ) {
      hits++;
      await work();
    }
    return original.apply(this, args);
  } as LockManager['request'];
  return {
    hits: () => hits,
    restore: () => {
      navigator.locks.request = original;
    }
  };
}

export async function changeCheckNativeRow(
  row: { owner: string; id: string; wire: string },
  mode: 'missing' | 'corrupt' | 'alias' | 'foreign'
) {
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready')
    throw Error('actual mutation database unavailable');
  try {
    const tx = browserDatabaseTransaction(
        opened.owner,
        ['private_sends'],
        'readwrite'
      ),
      store = tx.objectStore('private_sends');
    if (mode === 'missing') store.delete([row.owner, row.id]);
    else if (mode === 'corrupt') store.put({ ...row, wire: row.wire + 'x' });
    else if (mode === 'alias') store.put({ ...row, id: crypto.randomUUID() });
    else
      store.put({
        ...row,
        owner: row.owner === 'f'.repeat(64) ? 'e'.repeat(64) : 'f'.repeat(64)
      });
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () =>
        reject(Error('actual fixture mutation aborted'))
      );
    });
  } finally {
    closeBrowserDatabase(opened.owner);
  }
}

export function abortCheckCleanup() {
  const remove = Object.getOwnPropertyDescriptor(
    IDBObjectStore.prototype,
    'delete'
  )!.value as IDBObjectStore['delete'];
  let aborts = 0,
    selected = false;
  IDBObjectStore.prototype.delete = function (
    ...args: Parameters<IDBObjectStore['delete']>
  ) {
    const request = remove.apply(this, args);
    if (!selected && this.name === 'private_sends') {
      selected = true;
      const tx = this.transaction;
      tx.addEventListener('abort', () => {
        aborts++;
      });
      request.addEventListener('success', () => tx.abort());
    }
    return request;
  };
  return {
    aborts: () => aborts,
    restore: () => {
      IDBObjectStore.prototype.delete = remove;
    }
  };
}

// Hold an ACTUAL competing owner WebLock only when the real nonce reader first
// requests admission. No callback receives a fabricated lock or idle result.
export function holdCheckNonceOwnerLock(holds: 1 | 2 = 1) {
  return holdCheckNonceAdmission(undefined, holds);
}
export async function holdCheckOwnerLock(owner: string) {
  let release = () => {},
    enter = () => {};
  const entered = new Promise<void>((resolve) => {
    enter = resolve;
  });
  const held = navigator.locks.request(
    'harvestcircle:owner:' + owner,
    async (lock) => {
      if (!lock) throw Error('actual fixture owner lock missing');
      enter();
      await new Promise<void>((resolve) => {
        release = resolve;
      });
    }
  );
  await entered;
  return async () => {
    release();
    await held;
  };
}
export function holdCheckNonceSdk(session: PrivateSession, holds: 1 | 2 = 1) {
  return holdCheckNonceAdmission(session, holds);
}
function holdCheckNonceAdmission(
  session: PrivateSession | undefined,
  holds: 1 | 2
) {
  const original = Object.getOwnPropertyDescriptor(
    LockManager.prototype,
    'request'
  )!.value as LockManager['request'];
  let release: (() => void) | undefined,
    holder: Promise<unknown> | undefined,
    hits = 0;
  navigator.locks.request = async function (
    this: LockManager,
    ...args: Parameters<LockManager['request']>
  ) {
    if (
      hits < holds &&
      /read(?:Recovered)?InboxProbeNonce/.test(new Error().stack ?? '')
    ) {
      hits++;
      await new Promise<void>((ready) => {
        if (session) {
          holder = withIdlePrivateLocalStorage(session, async () => {
            ready();
            await new Promise<void>((resolve) => {
              release = resolve;
            });
          });
          return;
        }
        holder = original.call(
          this,
          args[0],
          { mode: 'exclusive' },
          async (lock) => {
            if (!lock) throw Error('actual nonce-stage competing lock missing');
            ready();
            await new Promise<void>((resolve) => {
              release = resolve;
            });
          }
        );
      });
    }
    return original.apply(this, args);
  } as LockManager['request'];
  return {
    hits: () => hits,
    async release() {
      release?.();
      await holder;
    },
    restore() {
      navigator.locks.request = original;
    }
  };
}

// A new child realm uses only the parent extension's ephemeral closure and
// genuine native storage. No predecessor preparation or proof is imported.
export async function openHostedCheck(setup: {
  owner: string;
  policyWire: string;
  operatorWire: string;
  command: string;
}) {
  installHostedProvider();
  const policy = validateRelayPolicy(setup.policyWire),
    operator = validateInboxOperatorQualification(setup.operatorWire);
  if (!policy || !operator) throw Error('actual hosted policy and operator');
  const context = await openRecoveryContext(
    setup.owner,
    policy,
    operator,
    setup.command
  );
  const check = createInboxAccessCheck(
    context.repository,
    context.identity,
    context.session,
    policy,
    context.access,
    operator
  );
  if (!check) throw Error('actual hosted coordinator capture');
  return { context, check };
}
