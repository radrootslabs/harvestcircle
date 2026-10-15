import { makeQualificationFixture } from './inbox-access-qualification.ts';
import type { openRecoveryContext } from './inbox-probe-recovery.ts';
import {
  admitInboxEnvelope,
  inboxEnvelopeSnapshot,
  retainInboxEnvelope
} from '../../../src/lib/persistence/inbox-envelope-repository.ts';
import type { PrivateAccessProbe } from '../../../src/lib/persistence/private-records.ts';
import { observeQualifiedInboxAccess } from '../../../src/lib/messaging/inbox-access-qualification.ts';
import {
  captureInboxIngress,
  startInboxIngress,
  stopInboxIngress,
  inboxIngressSnapshot,
  type InboxIngress
} from '../../../src/lib/nostr/inbox-ingress.ts';
import {
  captureInboxSync,
  startInboxSync,
  stopInboxSync,
  inboxSyncSnapshot,
  type InboxSync
} from '../../../src/lib/messaging/inbox-sync.ts';
import {
  captureOlderInbox,
  loadOlderInbox,
  olderInboxSnapshot,
  stopOlderInbox,
  type OlderInbox
} from '../../../src/lib/messaging/history-pagination.ts';
import {
  captureUnlockedSession,
  type UnlockedSession
} from '../../../src/lib/messaging/unlocked-session.ts';
import {
  captureDecryptionQueue,
  runDecryptionBatch,
  stopDecryptionQueue,
  decryptionQueueSnapshot,
  decryptionQueueConversations,
  type DecryptionQueue
} from '../../../src/lib/messaging/decryption-queue.ts';
import {
  createPublicView,
  beginPublicViewRun,
  disposePublicView
} from '../../../src/lib/runtime/public-runtime.ts';
import {
  resolveInboxPreference,
  settleInboxResolver
} from '../../../src/lib/messaging/resolve-inbox.ts';
import {
  openBrowserDatabase,
  browserDatabaseTransaction,
  closeBrowserDatabase
} from '../../../src/lib/persistence/database.ts';
import { inboxProbeRumorWire } from '../../../src/lib/nostr/inbox-access.ts';
import { recoveredInboxProbeRumorWire } from '../../../src/lib/nostr/self-recovery-reader.ts';
import {
  generateSecretKey,
  getPublicKey,
  getEventHash,
  finalizeEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
export {
  observeInboxProbeDelivery,
  consumeInboxProbeDelivery
} from '../../../src/lib/persistence/inbox-probes.ts';
export * from './inbox-access-qualification.ts';
export { admitInboxEnvelope, retainInboxEnvelope };
export { openBrowserDatabase, closeBrowserDatabase };
// Observe real native transactions and mutation calls without changing results.
export function watchExclusionNative() {
  const transaction = Object.getOwnPropertyDescriptor(
      IDBDatabase.prototype,
      'transaction'
    )!.value as IDBDatabase['transaction'],
    put = Object.getOwnPropertyDescriptor(IDBObjectStore.prototype, 'put')!
      .value as IDBObjectStore['put'],
    remove = Object.getOwnPropertyDescriptor(
      IDBObjectStore.prototype,
      'delete'
    )!.value as IDBObjectStore['delete'];
  let readonly = 0,
    readwrite = 0,
    puts = 0,
    deletes = 0;
  IDBDatabase.prototype.transaction = function (
    ...args: Parameters<IDBDatabase['transaction']>
  ) {
    if (args[1] === 'readwrite') readwrite++;
    else readonly++;
    return transaction.apply(this, args);
  };
  IDBObjectStore.prototype.put = function (
    ...args: Parameters<IDBObjectStore['put']>
  ) {
    puts++;
    return put.apply(this, args);
  };
  IDBObjectStore.prototype.delete = function (
    ...args: Parameters<IDBObjectStore['delete']>
  ) {
    deletes++;
    return remove.apply(this, args);
  };
  return {
    counts: () => ({ readonly, readwrite, puts, deletes }),
    restore() {
      IDBDatabase.prototype.transaction = transaction;
      IDBObjectStore.prototype.put = put;
      IDBObjectStore.prototype.delete = remove;
    }
  };
}
// Delay delivery of the FIRST actual native complete notification. The real
// cursor, result and trusted completion event still run; no success is fabricated.
export function holdExclusionNativeCompletion() {
  const transaction = Object.getOwnPropertyDescriptor(
    IDBDatabase.prototype,
    'transaction'
  )!.value as IDBDatabase['transaction'];
  let selected = false,
    waiting = false,
    released = false;
  const deliveries: (() => void)[] = [];
  IDBDatabase.prototype.transaction = function (
    ...args: Parameters<IDBDatabase['transaction']>
  ) {
    const tx = transaction.apply(this, args);
    if (
      !selected &&
      tx.mode === 'readonly' &&
      tx.objectStoreNames.contains('private_sends')
    ) {
      selected = true;
      const add = tx.addEventListener.bind(tx);
      Object.defineProperty(tx, 'addEventListener', {
        configurable: true,
        value(
          type: string,
          listener: EventListenerOrEventListenerObject | null,
          options?: boolean | AddEventListenerOptions
        ) {
          if (!listener) return;
          if (type !== 'complete') return add(type, listener, options);
          add(
            type,
            (event) => {
              const deliver = () => {
                if (typeof listener === 'function') listener.call(tx, event);
                else listener.handleEvent(event);
              };
              if (released) deliver();
              else {
                waiting = true;
                deliveries.push(deliver);
              }
            },
            options
          );
        }
      });
    }
    return tx;
  };
  return {
    waiting: () => waiting,
    release() {
      released = true;
      waiting = false;
      IDBDatabase.prototype.transaction = transaction;
      for (const deliver of deliveries.splice(0)) deliver();
    }
  };
}
// Real child documents use the surviving extension host while all product
// actors and their private provenance belong to the child's fresh module realm.
export async function retainInRecoveryContext(
  context: Awaited<ReturnType<typeof openRecoveryContext>>,
  wire: string
) {
  const envelope = admitInboxEnvelope(
    wire,
    context.owner,
    'wss://inbox.example.org',
    Date.now()
  );
  return envelope
    ? (await retainInboxEnvelope(context.repository, context.session, envelope))
        .status
    : 'invalid';
}
export async function decryptInRecoveryContext(
  context: Awaited<ReturnType<typeof openRecoveryContext>>
) {
  const unlocked = captureUnlockedSession(
    context.session,
    'reviewed_messages_unlock'
  );
  if (!unlocked) throw Error('actual child explicit unlock');
  const queue = captureDecryptionQueue(
    context.repository,
    context.session,
    context.identity,
    unlocked,
    'reviewed_inbox_unlock'
  );
  if (!queue) throw Error('actual child decryption queue');
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
}
export async function makeExclusionFixture() {
  const base = await makeQualificationFixture();
  const views: ReturnType<typeof createPublicView>[] = [];
  let finite: InboxIngress | undefined,
    sync: InboxSync | undefined,
    older: OlderInbox | undefined,
    unlocked: UnlockedSession | undefined,
    queue: DecryptionQueue | undefined;
  async function selfWire() {
    const row = (await base.host.stored()).find(
      (value) => value.id === base.command()
    );
    if (!row) throw Error('actual source-admitted original probe row');
    const record = JSON.parse(row.wire) as PrivateAccessProbe;
    if (!record.self) throw Error('actual original self ciphertext');
    return record.self.wire;
  }
  return {
    ...base,
    selfWire,
    async seedReceived(count = 1999) {
      if (!Number.isSafeInteger(count) || count < 0 || count > 2000)
        throw Error('bounded native quota fixture');
      const probe = JSON.parse(await selfWire()) as { created_at: number };
      const secret = generateSecretKey(),
        key = nip44.v2.utils.getConversationKey(secret, base.host.owner);
      const rows: { owner: string; id: string; wire: string }[] = [];
      try {
        const content = nip44.v2.encrypt(
          'bounded native outer-only quota fixture',
          key
        );
        for (let n = 0; n < count; n++) {
          const event = finalizeEvent(
            {
              kind: 1059,
              created_at: probe.created_at + n + 1,
              tags: [['p', base.host.owner]],
              content
            },
            secret
          );
          const envelope = admitInboxEnvelope(
            JSON.stringify(event),
            base.host.owner,
            'wss://inbox.example.org',
            Date.now()
          );
          const record = envelope && inboxEnvelopeSnapshot(envelope);
          if (!record)
            throw Error('actual stock signed outer fixture admission');
          rows.push({
            owner: base.host.owner,
            id: event.id,
            wire: JSON.stringify(record)
          });
        }
      } finally {
        key.fill(0);
        secret.fill(0);
      }
      const opened = await openBrowserDatabase();
      if (opened.state !== 'ready')
        throw Error('actual received quota database');
      try {
        const tx = browserDatabaseTransaction(
          opened.owner,
          ['received_envelopes'],
          'readwrite'
        );
        for (const row of rows) tx.objectStore('received_envelopes').put(row);
        await new Promise<void>((resolve, reject) => {
          tx.addEventListener('complete', () => resolve());
          tx.addEventListener('abort', () =>
            reject(Error('actual received quota seed abort'))
          );
        });
      } finally {
        closeBrowserDatabase(opened.owner);
      }
      return probe.created_at + 1;
    },
    async deleteReceived(id: string) {
      const opened = await openBrowserDatabase();
      if (opened.state !== 'ready')
        throw Error('actual received fixture deletion');
      try {
        const tx = browserDatabaseTransaction(
          opened.owner,
          ['received_envelopes'],
          'readwrite'
        );
        tx.objectStore('received_envelopes').delete([base.host.owner, id]);
        await new Promise<void>((resolve, reject) => {
          tx.addEventListener('complete', () => resolve());
          tx.addEventListener('abort', () =>
            reject(Error('actual received fixture delete abort'))
          );
        });
      } finally {
        closeBrowserDatabase(opened.owner);
      }
    },
    async rowWire() {
      const row = (await base.host.stored()).find(
        (value) => value.id === base.command()
      );
      if (!row) throw Error('actual source journal row');
      return row.wire;
    },
    async replaceRow(
      wire: string,
      id = base.command()!,
      owner = base.host.owner
    ) {
      const opened = await openBrowserDatabase();
      if (opened.state !== 'ready')
        throw Error('actual fixture native database');
      try {
        const tx = browserDatabaseTransaction(
          opened.owner,
          ['private_sends'],
          'readwrite'
        );
        tx.objectStore('private_sends').put({ owner, id, wire });
        await new Promise<void>((resolve, reject) => {
          tx.addEventListener('complete', () => resolve());
          tx.addEventListener('abort', () =>
            reject(Error('actual fixture native write abort'))
          );
        });
      } finally {
        closeBrowserDatabase(opened.owner);
      }
    },
    ordinaryWire(recovered: boolean, time?: number) {
      const a = base.actors(),
        original = base.originalIdentity(),
        proof = base.proof();
      const rumorWire =
        recovered && proof
          ? recoveredInboxProbeRumorWire(
              proof,
              base.repository,
              a.session,
              a.access,
              base.host.policy,
              base.host.qualification
            )
          : original && inboxProbeRumorWire(original, a.session);
      if (!rumorWire) throw Error('actual original diagnostic rumor source');
      const diagnostic = JSON.parse(rumorWire) as {
        content: string;
        created_at: number;
      };
      const peer = generateSecretKey(),
        outer = generateSecretKey();
      function encrypt(secret: Uint8Array, plain: string) {
        const key = nip44.v2.utils.getConversationKey(secret, base.host.owner);
        try {
          return nip44.v2.encrypt(plain, key);
        } finally {
          key.fill(0);
        }
      }
      try {
        const template = {
          pubkey: getPublicKey(peer),
          kind: 14,
          created_at: diagnostic.created_at,
          tags: [['p', base.host.owner]],
          content: diagnostic.content
        };
        const rumor = { id: getEventHash(template), ...template };
        const seal = finalizeEvent(
          {
            kind: 13,
            created_at: diagnostic.created_at,
            tags: [],
            content: encrypt(peer, JSON.stringify(rumor))
          },
          peer
        );
        return JSON.stringify(
          finalizeEvent(
            {
              kind: 1059,
              created_at: time ?? diagnostic.created_at,
              tags: [['p', base.host.owner]],
              content: encrypt(outer, JSON.stringify(seal))
            },
            outer
          )
        );
      } finally {
        peer.fill(0);
        outer.fill(0);
      }
    },
    async startReceive(lane: 'finite' | 'sync') {
      const qualification = base.qualification();
      if (!qualification)
        throw Error('actual genuine access qualification missing');
      const a = base.actors(),
        view = createPublicView(base.sharedPublicRuntime());
      views.push(view);
      const own = resolveInboxPreference(
        view,
        beginPublicViewRun(view),
        base.host.owner
      );
      await settleInboxResolver(own);
      const observe = observeQualifiedInboxAccess(qualification);
      if (lane === 'finite') {
        finite = captureInboxIngress(
          base.repository,
          a.session,
          base.host.policy,
          own,
          observe,
          'reviewed_private_inbox_receive'
        );
        return (
          !!finite &&
          startInboxIngress(finite, 'reviewed_private_inbox_receive')
        );
      }
      unlocked = captureUnlockedSession(a.session, 'reviewed_messages_unlock');
      if (!unlocked) throw Error('actual owner unlocked scope');
      sync = captureInboxSync(
        base.repository,
        a.session,
        base.host.policy,
        own,
        unlocked,
        observe,
        'reviewed_foreground_inbox'
      );
      return !!sync && startInboxSync(sync, 'reviewed_foreground_inbox');
    },
    receiveSnapshot: () =>
      finite ? inboxIngressSnapshot(finite) : sync && inboxSyncSnapshot(sync),
    async older() {
      if (!sync) throw Error('actual foreground sync');
      older ??= captureOlderInbox(sync, 'reviewed_load_older');
      return !!older && loadOlderInbox(older, 'reviewed_load_older');
    },
    olderSnapshot: () => older && olderInboxSnapshot(older),
    stopReceive() {
      if (older) stopOlderInbox(older);
      if (finite) stopInboxIngress(finite);
      if (sync) stopInboxSync(sync);
    },
    async decryptStored() {
      const a = base.actors();
      unlocked ??= captureUnlockedSession(
        a.session,
        'reviewed_messages_unlock'
      );
      if (!unlocked) throw Error('actual explicit unlock');
      queue ??= captureDecryptionQueue(
        base.repository,
        a.session,
        a.identity,
        unlocked,
        'reviewed_inbox_unlock'
      );
      if (!queue) throw Error('actual explicit decryption queue');
      const before = base.host.counts();
      const result = await runDecryptionBatch(
        queue,
        'reviewed_decrypt_batch',
        []
      );
      return {
        result,
        snapshot: decryptionQueueSnapshot(queue),
        rooms: decryptionQueueConversations(queue).length,
        before,
        after: base.host.counts()
      };
    },
    async retain(raw?: string, cancellation?: AbortSignal) {
      const a = base.actors();
      const envelope = admitInboxEnvelope(
        raw ?? (await selfWire()),
        base.host.owner,
        'wss://inbox.example.org',
        Date.now()
      );
      return envelope
        ? (
            await retainInboxEnvelope(
              base.repository,
              a.session,
              envelope,
              cancellation
            )
          ).status
        : 'invalid';
    },
    close() {
      if (queue) stopDecryptionQueue(queue);
      if (older) stopOlderInbox(older);
      if (finite) stopInboxIngress(finite);
      if (sync) stopInboxSync(sync);
      for (const view of views) disposePublicView(view);
      base.close();
    }
  };
}
