import {
  captureDecryptionQueue,
  refreshDecryptionQueue,
  runDecryptionBatch,
  decryptionQueueSnapshot,
  stopDecryptionQueue,
  expireDecryptionQueueWait,
  type DecryptionQueue
} from '../../../src/lib/messaging/decryption-queue.ts';
import { makeFixture as makeRecoveryFixture } from './resume-preparation.ts';
import {
  createPrivateSession,
  closePrivateSession
} from '../../../src/lib/runtime/private-session.ts';
import {
  captureUnlockedSession,
  closeUnlockedSession,
  readUnlockedMessages,
  unlockedSessionSnapshot
} from '../../../src/lib/messaging/unlocked-session.ts';
import {
  admitInboxEnvelope,
  retainInboxEnvelope
} from '../../../src/lib/persistence/inbox-envelope-repository.ts';
import { listPrivateReceivedRecords } from '../../../src/lib/persistence/private-storage.ts';
import {
  privateRecordWire,
  privateRecordIdentity
} from '../../../src/lib/persistence/private-records.ts';
import {
  finalizeEvent,
  getPublicKey,
  getEventHash
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import {
  browserExtensionScheduler,
  extensionSchedulerSnapshot,
  runExtensionAction
} from '../../../src/lib/nostr/extension-scheduler.ts';
import {
  identityMessagingOwnership,
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity,
  identitySessionSnapshot
} from '../../../src/lib/runtime/identity-session.ts';
import {
  getPublicStore,
  publicStoreEnvelope
} from '../../../src/lib/nostr/public-store.ts';
export async function makeFixture(count = 21, unknownSender = false) {
  const f = await makeRecoveryFixture();
  const session = await createPrivateSession(
    f.identity,
    'reviewed_private_session'
  );
  if (!session) throw Error('missing genuine original private session');
  const unlocked = captureUnlockedSession(session, 'reviewed_messages_unlock');
  if (!unlocked) throw Error('missing genuine original unlock');
  const initial = (await f.row())!;
  const originalOutbox = JSON.stringify(initial.record);
  const ownerSecret = new Uint8Array(32).fill(83),
    outerSecret = new Uint8Array(32).fill(86),
    strangerSecret = new Uint8Array(32).fill(85);
  const stranger = getPublicKey(strangerSecret),
    disposable = getPublicKey(outerSecret);
  function encrypt(secret: Uint8Array, target: string, plain: string) {
    const key = nip44.v2.utils.getConversationKey(secret, target);
    try {
      return nip44.v2.encrypt(plain, key);
    } finally {
      key.fill(0);
    }
  }
  let sealWire: string;
  try {
    if (unknownSender) {
      const template = {
        pubkey: stranger,
        kind: 14,
        created_at: 1700000000,
        tags: [['p', f.owner]],
        content: 'Controlled unknown authenticated sender'
      };
      const rumor = { id: getEventHash(template), ...template };
      sealWire = JSON.stringify(
        finalizeEvent(
          {
            kind: 13,
            created_at: 1699999900,
            tags: [],
            content: encrypt(strangerSecret, f.owner, JSON.stringify(rumor))
          },
          strangerSecret
        )
      );
    } else {
      const outer = JSON.parse(initial.record.self.wire) as {
        pubkey: string;
        content: string;
      };
      const key = nip44.v2.utils.getConversationKey(ownerSecret, outer.pubkey);
      try {
        sealWire = nip44.v2.decrypt(outer.content, key);
      } finally {
        key.fill(0);
      }
    }
    for (let i = 0; i < count; i++) {
      const outer = finalizeEvent(
        {
          kind: 1059,
          created_at: 1700000000 + i,
          tags: [['p', f.owner]],
          content: encrypt(outerSecret, f.owner, sealWire)
        },
        outerSecret
      );
      const admitted = admitInboxEnvelope(
        JSON.stringify(outer),
        f.owner,
        'wss://archive.example.org',
        100 + i
      );
      if (
        !admitted ||
        (await retainInboxEnvelope(f.repository, session, admitted)).status !==
          'retained'
      )
        throw Error('missing actual retained ciphertext');
    }
  } finally {
    ownerSecret.fill(0);
    outerSecret.fill(0);
    strangerSecret.fill(0);
  }
  const publicStore = getPublicStore()!;
  async function retained() {
    const rows = await listPrivateReceivedRecords(f.repository);
    if (!rows.ok) throw Error('actual received scan failed');
    return rows.value.map((row) =>
      privateRecordWire(row, f.owner, privateRecordIdentity(row)?.id)
    );
  }
  const originalReceived = JSON.stringify(await retained()),
    baseline = f.counts();
  let queue: DecryptionQueue | undefined;
  return {
    ...f,
    stranger,
    disposable,
    capture(review: unknown = 'reviewed_inbox_unlock') {
      queue = captureDecryptionQueue(
        f.repository,
        session,
        f.identity,
        unlocked,
        review
      );
      return !!queue;
    },
    refresh: async () => queue && (await refreshDecryptionQueue(queue)),
    batch: async (
      review: unknown = 'reviewed_decrypt_batch',
      blocked: readonly string[] = []
    ) => queue && (await runDecryptionBatch(queue, review, blocked)),
    snapshot: () => queue && decryptionQueueSnapshot(queue),
    delta: () => ({
      decrypts: f.counts().decrypts - baseline.decrypts,
      keys: f.counts().keys - baseline.keys,
      signs: f.counts().signs - baseline.signs,
      encrypts: f.counts().encrypts - baseline.encrypts
    }),
    cache: () => unlockedSessionSnapshot(unlocked),
    messages: () => readUnlockedMessages(unlocked),
    encryptedUnchanged: async () =>
      JSON.stringify(await retained()) === originalReceived &&
      JSON.stringify((await f.row())!.record) === originalOutbox,
    count: async () => (await retained()).length,
    publicContains: async () => {
      const rows = await listPrivateReceivedRecords(f.repository);
      return (
        rows.ok &&
        rows.value.some((row) => {
          const wire = privateRecordWire(
            row,
            f.owner,
            privateRecordIdentity(row)?.id
          );
          return (
            wire &&
            publicStoreEnvelope(
              publicStore,
              (JSON.parse(wire) as { id: string }).id
            ) !== undefined
          );
        })
      );
    },
    forged: () =>
      runDecryptionBatch({} as DecryptionQueue, 'reviewed_decrypt_batch'),
    copied: () =>
      queue && runDecryptionBatch({ ...queue }, 'reviewed_decrypt_batch'),
    stop: () => {
      if (queue) stopDecryptionQueue(queue);
    },
    expire: () => {
      if (queue) expireDecryptionQueueWait(queue);
    },
    closeUnlock: () => closeUnlockedSession(unlocked),
    scheduler: () => {
      const sdk = browserExtensionScheduler();
      return sdk && extensionSchedulerSnapshot(sdk);
    },
    identitySnapshot: () => identitySessionSnapshot(f.identity),
    async foreignCapture() {
      const other = createIdentitySession();
      await connectIdentity(other);
      await probeIdentityMessaging(other, 'reviewed_self_copy');
      const before = f.counts();
      const candidate = captureDecryptionQueue(
        f.repository,
        session,
        other,
        unlocked,
        'reviewed_inbox_unlock'
      );
      const after = f.counts();
      disconnectIdentity(other);
      return {
        captured: !!candidate,
        keys: after.keys - before.keys,
        decrypts: after.decrypts - before.decrypts,
        signs: after.signs - before.signs
      };
    },
    competing: async () => {
      const sdk = browserExtensionScheduler(),
        ownership = identityMessagingOwnership(f.identity);
      return (
        sdk &&
        ownership &&
        (await runExtensionAction(
          sdk,
          {
            owner: ownership.owner,
            session: ownership.session,
            operation: Symbol()
          },
          ownership.current,
          () => Promise.resolve(true)
        ))
      );
    },
    close() {
      if (queue) stopDecryptionQueue(queue);
      closePrivateSession(session);
      f.close();
    }
  };
}
