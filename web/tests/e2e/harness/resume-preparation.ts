import {
  finalizeEvent,
  getPublicKey,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { verifyEnvelope } from '../../../src/lib/nostr/verified-envelope.ts';
import {
  createIdentitySession,
  connectIdentity,
  probeIdentityMessaging,
  disconnectIdentity
} from '../../../src/lib/runtime/identity-session.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  createPrivateStorageRepository,
  loadPrivateRecord
} from '../../../src/lib/persistence/private-storage.ts';
import {
  privateRecordSnapshot,
  privateRecordWire,
  decodePrivateRecord
} from '../../../src/lib/persistence/private-records.ts';
import { createPrivateSendReservationRepository } from '../../../src/lib/persistence/private-send-reservations.ts';
import { captureEnquiryContext } from '../../../src/lib/messaging/enquiry-context.ts';
import {
  captureEnquiryRumor,
  stopRumorPlan,
  rumorPlanSnapshot
} from '../../../src/lib/messaging/rumor-plan.ts';
import {
  reserveSendIdentity,
  reservedSendSnapshot
} from '../../../src/lib/messaging/send-identity.ts';
import {
  captureSelfRecoveryPreparation,
  prepareSelfRecovery,
  stopSelfRecoveryPreparation
} from '../../../src/lib/messaging/prepare-send.ts';
import {
  browserExtensionScheduler,
  extensionSchedulerSnapshot
} from '../../../src/lib/nostr/extension-scheduler.ts';
import { validateRelayPolicy } from '../../../src/lib/config/relays.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  closePublicScheduler
} from '../../../src/lib/nostr/request-scope.ts';
import { createInboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';
import { createInboxRoutePlan } from '../../../src/lib/messaging/inbox-routing.ts';
import {
  captureResumePreparation,
  resumeEncryptedPreparation,
  resumePreparationSnapshot,
  resumeRecoveredRumor,
  stopResumePreparation
} from '../../../src/lib/messaging/resume-preparation.ts';
export {
  captureResumePreparation,
  resumeEncryptedPreparation,
  resumePreparationSnapshot,
  resumeRecoveredRumor,
  stopResumePreparation,
  rumorPlanSnapshot
};
// Public disposable fixture keys, never installed operator credentials. Stable
// only so an actual page reload can unlock the same actual stored ciphertext.
const command = '12345678-1234-4234-8234-123456789abc',
  marker = 'HCP083_PRIVATE_RECOVERY_MEMORY_ONLY_SENTINEL';
type Mode =
  | 'normal'
  | 'changed_key'
  | 'hold_decrypt'
  | 'declined_decrypt'
  | 'wrong_plaintext';
export async function makeFixture(initialise = true) {
  const secret = new Uint8Array(32).fill(83),
    peerSecret = new Uint8Array(32).fill(84),
    owner = getPublicKey(secret),
    peer = getPublicKey(peerSecret);
  let mode: Mode = 'normal',
    keys = 0,
    encrypts = 0,
    decrypts = 0,
    signs = 0,
    release: (() => void) | undefined;
  function encrypt(target: string, text: string) {
    const conversation = nip44.v2.utils.getConversationKey(secret, target);
    try {
      return nip44.v2.encrypt(text, conversation);
    } finally {
      conversation.fill(0);
    }
  }
  function decrypt(target: string, wire: string) {
    const conversation = nip44.v2.utils.getConversationKey(secret, target);
    try {
      return nip44.v2.decrypt(wire, conversation);
    } finally {
      conversation.fill(0);
    }
  }
  Object.defineProperty(window, 'nostr', {
    configurable: true,
    value: {
      getPublicKey: () => {
        keys++;
        return Promise.resolve(mode === 'changed_key' ? peer : owner);
      },
      signEvent: (template: UnsignedEvent) => {
        signs++;
        return Promise.resolve(finalizeEvent(template, secret));
      },
      nip44: {
        encrypt: (target: string, text: string) => {
          encrypts++;
          return Promise.resolve(encrypt(target, text));
        },
        decrypt: (target: string, wire: string) => {
          decrypts++;
          if (mode === 'declined_decrypt')
            return Promise.reject(Error('controlled decryption refusal'));
          const work = () =>
            mode === 'wrong_plaintext' ? '{}' : decrypt(target, wire);
          return mode === 'hold_decrypt'
            ? new Promise<string>((resolve) => {
                release = () => resolve(work());
              })
            : Promise.resolve(work());
        }
      }
    }
  });
  const identity = createIdentitySession();
  await connectIdentity(identity);
  await probeIdentityMessaging(identity, 'reviewed_self_copy');
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error('fixture storage unavailable');
  const database = opened.owner,
    repository = createPrivateStorageRepository(database, owner);
  if (!repository) throw Error('fixture namespace unavailable');
  if (initialise) {
    const listing = finalizeEvent(
      {
        kind: 30402,
        created_at: 1700000060,
        content: 'Public carrots',
        tags: [
          ['d', 'carrots'],
          ['title', 'Carrots'],
          ['summary', 'Fresh carrots'],
          ['published_at', '1700000000'],
          ['location', 'Victoria'],
          ['price', '3.5', 'CAD'],
          ['radroots:price_unit', 'lb'],
          ['status', 'active']
        ]
      },
      peerSecret
    );
    const verified = verifyEnvelope(JSON.stringify(listing)),
      context = verified.ok && captureEnquiryContext(verified.value),
      observed = Date.now();
    const plan =
      context &&
      captureEnquiryRumor(
        identity,
        context,
        marker,
        Math.floor(observed / 1000)
      );
    const reservations = createPrivateSendReservationRepository(
      database,
      owner
    );
    if (!plan || !reservations)
      throw Error('fixture original intent unavailable');
    const realNow = Date.now;
    Date.now = () => observed;
    let reserved: Awaited<ReturnType<typeof reserveSendIdentity>>;
    try {
      reserved = await reserveSendIdentity(
        reservations,
        identity,
        command,
        plan,
        'reviewed_private_intent'
      );
    } finally {
      Date.now = realNow;
    }
    if (!('identity' in reserved) || !reservedSendSnapshot(reserved.identity))
      throw Error('fixture original command unavailable');
    const preparation = captureSelfRecoveryPreparation(
      repository,
      identity,
      reserved.identity,
      'reviewed_self_recovery'
    );
    if (
      !preparation ||
      !['saved', 'existing'].includes(
        (await prepareSelfRecovery(preparation, 'reviewed_self_recovery'))
          .status
      )
    )
      throw Error('fixture actual self save unavailable');
    stopSelfRecoveryPreparation(preparation);
    stopRumorPlan(plan);
  }
  const discovery = 'wss://discovery.example.org',
    archive = 'wss://archive.example.org',
    destination = 'wss://peer.example.org';
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [{ origin: discovery, read: true, write: false, nip50: false }],
      inbox: [archive, destination].map((origin) => ({
        origin,
        read: true,
        write: true
      })),
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  );
  if (!policy) throw Error('fixture policy unavailable');
  const schedulers: ReturnType<typeof createPublicScheduler>[] = [],
    tokens: ReturnType<typeof captureResumePreparation>[] = [];
  let current = true;
  function resolve(asPeer: boolean) {
    const event = finalizeEvent(
      {
        kind: 10050,
        created_at: 100,
        tags: [['relay', asPeer ? destination : archive]],
        content: ''
      },
      asPeer ? peerSecret : secret
    );
    const scheduler = createPublicScheduler({
      now: () => 0,
      schedule: () => () => {}
    });
    schedulers.push(scheduler);
    const run = createPublicRun(scheduler, policy!);
    return createInboxResolver(
      run,
      event.pubkey,
      (next) =>
        openInboxRequest(
          run,
          event.pubkey,
          (sink) => {
            sink({ type: 'EVENT', from: discovery, id: 'fixture', event });
            sink({ type: 'EOSE', from: discovery, id: 'fixture' });
            return () => {};
          },
          next
        ),
      () => current
    );
  }
  const own = resolve(false),
    other = resolve(true),
    plan = createInboxRoutePlan(policy, owner, peer, own, other);
  if (!plan) throw Error('fixture genuine routing unavailable');
  async function row() {
    const loaded = await loadPrivateRecord(
      repository!,
      'private_sends',
      command
    );
    if (!loaded.ok) return undefined;
    const record = privateRecordSnapshot(loaded.value, owner, command),
      wire = privateRecordWire(loaded.value, owner, command);
    return record?.family === 'private_send_operation' && wire
      ? { record, wire }
      : undefined;
  }
  async function raw() {
    const tx = browserDatabaseTransaction(
        database,
        ['private_sends'],
        'readonly'
      ),
      request = tx.objectStore('private_sends').getAll();
    return new Promise<string>((resolve, reject) => {
      tx.addEventListener('complete', () =>
        resolve(JSON.stringify(request.result))
      );
      tx.addEventListener('abort', () => reject(Error('fixture read aborted')));
    });
  }
  async function write(kind: 'erase' | 'revision' | 'corrupt') {
    const original = kind === 'revision' ? await row() : undefined;
    const wire =
      original &&
      JSON.stringify({
        ...original.record,
        revision: original.record.revision + 1
      });
    if (
      kind === 'revision' &&
      (!wire || !decodePrivateRecord(wire, owner, command).ok)
    )
      throw Error('fixture successor unavailable');
    const tx = browserDatabaseTransaction(
        database,
        ['private_sends'],
        'readwrite'
      ),
      store = tx.objectStore('private_sends');
    if (kind === 'erase') store.delete([owner, command]);
    else
      store.put({
        owner,
        id: command,
        wire: kind === 'corrupt' ? '{"schema":999}' : wire
      });
    await new Promise<void>((resolve, reject) => {
      tx.addEventListener('complete', () => resolve());
      tx.addEventListener('abort', () =>
        reject(Error('fixture write aborted'))
      );
    });
  }
  return {
    owner,
    peer,
    command,
    marker,
    identity,
    repository,
    context: { plan, policy, own, other },
    row,
    raw,
    write,
    capture() {
      const token = captureResumePreparation(
        repository,
        identity,
        command,
        'reviewed_private_resume'
      );
      if (!token) throw Error('fixture resume unavailable');
      tokens.push(token);
      return token;
    },
    counts: () => ({ keys, encrypts, decrypts, signs }),
    mode: (next: Mode) => {
      mode = next;
    },
    settle: () => release?.(),
    pending: () => {
      const scheduler = browserExtensionScheduler(),
        slot = scheduler && extensionSchedulerSnapshot(scheduler);
      return slot?.state === 'active' && slot.pending === 'decrypt';
    },
    disconnect: () => disconnectIdentity(identity),
    decryptPeer(wire: string) {
      const outer = JSON.parse(wire) as { pubkey: string; content: string };
      const conversation = nip44.v2.utils.getConversationKey(
        peerSecret,
        outer.pubkey
      );
      let seal: { pubkey: string; content: string };
      try {
        seal = JSON.parse(
          nip44.v2.decrypt(outer.content, conversation)
        ) as typeof seal;
      } finally {
        conversation.fill(0);
      }
      const inner = nip44.v2.utils.getConversationKey(peerSecret, seal.pubkey);
      try {
        return JSON.parse(nip44.v2.decrypt(seal.content, inner)) as {
          id: string;
          created_at: number;
          content: string;
        };
      } finally {
        inner.fill(0);
      }
    },
    close() {
      current = false;
      for (const token of tokens) if (token) stopResumePreparation(token);
      for (const scheduler of schedulers) closePublicScheduler(scheduler);
      disconnectIdentity(identity);
      closeBrowserDatabase(database);
      secret.fill(0);
      peerSecret.fill(0);
      delete window.nostr;
    }
  };
}
