import { mount, unmount } from 'svelte';
import Panel from './conversation-thread-panel.svelte';
import { makeFixture as makeRecoveryFixture } from './resume-preparation.ts';
import { makeFixture as makeRetryFixture } from './retry-send.ts';
import {
  getPublicKey,
  getEventHash,
  finalizeEvent,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { encodeProductReference } from '../../../src/lib/nostr/references.ts';
import { createPrivateSession } from '../../../src/lib/runtime/private-session.ts';
import {
  admitInboxEnvelope,
  retainInboxEnvelope
} from '../../../src/lib/persistence/inbox-envelope-repository.ts';
import {
  captureReceivedUnwrap,
  unwrapReceivedEnvelope,
  stopReceivedUnwrap
} from '../../../src/lib/nostr/unwrap-admission.ts';
import { admitReceivedConversation } from '../../../src/lib/messaging/admit-conversation.ts';
import {
  captureUnlockedSession,
  acceptUnlockedConversation,
  closeUnlockedSession
} from '../../../src/lib/messaging/unlocked-session.ts';
import { captureMessageMetadata } from '../../../src/lib/persistence/message-metadata.ts';
import {
  captureConversationDirectory,
  rememberAdmittedConversation,
  closeConversationDirectory
} from '../../../src/lib/messaging/conversation-directory.ts';
import {
  createInboxSetupView,
  checkInboxSetupView,
  closeInboxSetupView
} from '../../../src/lib/messaging/inbox-setup-view.ts';
import {
  readRelayPolicy,
  validateRelayPolicy
} from '../../../src/lib/config/relays.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  browserDatabaseTransaction
} from '../../../src/lib/persistence/database.ts';
import {
  createInboxView,
  inboxViewSnapshot,
  unlockInboxView,
  readNewInboxView,
  inboxThreadSnapshot,
  inboxMessageSnapshot,
  resolveInboxConversation,
  markInboxConversationDisplayed,
  stopInboxView,
  closeInboxView
} from '../../../src/lib/messaging/inbox-view.ts';
import { identityMessagingOwnership } from '../../../src/lib/runtime/identity-session.ts';
import type { RelayPolicy } from '../../../src/lib/config/relays.ts';
import type { InboxResolver } from '../../../src/lib/messaging/resolve-inbox.ts';

// HCP103 does not navigate; HCP104's separate harness exercises actual mounted
// beforeNavigate callbacks and unload/disposal. This alias owns fixture scope.
export function beforeNavigate(): void {}

export function goto(): never {
  throw Error('Thread fixture must not navigate');
}
export type Scenario =
  | 'inbound'
  | 'self_archive'
  | 'contexts'
  | 'duplicate'
  | 'receipt'
  | 'backfill'
  | 'hostile';
export async function renderFixture(
  kind: Scenario = 'inbound',
  selectedId?: string,
  qualifiedReplyFixture = false,
  freshOwnLookup?: (
    owner: string,
    policy: RelayPolicy
  ) => Promise<InboxResolver>
) {
  // Receipt scenario performs actual original SDK retry and receives loopback
  // OKs before mounting the readonly thread. The UI itself never publishes.
  const delivery = kind === 'receipt' ? await makeRetryFixture() : undefined,
    f = delivery ?? (await makeRecoveryFixture()),
    session = await createPrivateSession(
      f.identity,
      'reviewed_private_session'
    );
  if (!session) throw Error('no genuine current private session');
  let actualDelivery: unknown;
  if (delivery) {
    const prepared = await delivery.prepare();
    if (prepared.status !== 'existing' || !delivery.capture())
      throw Error('no actual stored pair retry');
    actualDelivery = await delivery.run();
  }
  const ownerSecret = new Uint8Array(32).fill(83),
    peerSecret = new Uint8Array(32).fill(84);
  function cipher(
    content: string,
    createdAt: number,
    index: number,
    parent?: string
  ) {
    const sender = getPublicKey(peerSecret),
      tags = [['p', f.owner]];
    if (parent) tags.push(['e', parent]);
    const template: UnsignedEvent = {
        pubkey: sender,
        kind: 14,
        created_at: createdAt,
        tags,
        content
      },
      rumor = { id: getEventHash(template), ...template };
    function encrypt(key: Uint8Array, plain: string) {
      const conversation = nip44.v2.utils.getConversationKey(key, f.owner);
      try {
        return nip44.v2.encrypt(plain, conversation);
      } finally {
        conversation.fill(0);
      }
    }
    const seal = finalizeEvent(
        {
          kind: 13,
          created_at: 1699999900,
          tags: [],
          content: encrypt(peerSecret, JSON.stringify(rumor))
        },
        peerSecret
      ),
      outerSecret = new Uint8Array(32).fill(85 + index);
    try {
      const outer = finalizeEvent(
        {
          kind: 1059,
          created_at: 1699999800 + index,
          tags: [['p', f.owner]],
          content: encrypt(outerSecret, JSON.stringify(seal))
        },
        outerSecret
      );
      return {
        wire: JSON.stringify(outer),
        rumorId: rumor.id,
        body: content,
        outerId: outer.id
      };
    } finally {
      outerSecret.fill(0);
    }
  }
  const carrots = encodeProductReference({
      kind: 30402,
      pubkey: f.peer,
      identifier: 'carrots'
    })!,
    apples = encodeProductReference({
      kind: 30402,
      pubkey: f.peer,
      identifier: 'apples'
    })!;
  let first: ReturnType<typeof cipher>,
    wires = Array.from<ReturnType<typeof cipher>>([]);
  try {
    if (kind === 'self_archive' || kind === 'receipt') {
      const row = (await f.row())!.record,
        outer = JSON.parse(row.self.wire) as { id: string };
      first = {
        wire: row.self.wire,
        outerId: outer.id,
        rumorId: row.rumorHash,
        body: 'Original encrypted self archive'
      };
    } else
      first = cipher(
        kind === 'hostile'
          ? '<img src=x> Quoted price 4 GBP https://untrusted.example.org nostr:naddr1invalid'
          : 'Actual encrypted thread fixture',
        1700000000,
        0,
        kind === 'hostile' ? 'f'.repeat(64) : undefined
      );
    wires = wires.concat(first);
    if (kind === 'contexts')
      wires = wires.concat(
        cipher(
          'About carrots\nProduct: nostr:' + carrots + '\nQuoted price: 4 GBP',
          1700000020,
          1
        ),
        cipher(
          'About apples\nProduct: nostr:' + apples + '\nQuoted price: 8 GBP',
          1700000010,
          2,
          first.rumorId
        )
      );
    if (kind === 'duplicate')
      wires = wires.concat(cipher(first.body, 1700000000, 1));
    if (kind === 'backfill')
      for (let i = 1; i <= 20; i++)
        wires = wires.concat(
          cipher('Actual later message ' + i, 1700000000 + i, i)
        );
    for (const wire of wires) {
      const admitted = admitInboxEnvelope(
        wire.wire,
        f.owner,
        'wss://archive.example.org',
        100
      );
      if (
        !admitted ||
        (await retainInboxEnvelope(f.repository, session, admitted)).status !==
          'retained'
      )
        throw Error('actual ciphertext not retained');
    }
  } finally {
    ownerSecret.fill(0);
    peerSecret.fill(0);
  }
  const opened = await openBrowserDatabase();
  if (opened.state !== 'ready') throw Error('no actual native database');
  const database = opened.owner;
  // Establish only a genuine original-room owner UUID through the existing
  // non-read native workflow, then discard the warmup plaintext cache. These
  // preparation SDK calls are distinct from the later explicit UI count.
  const reader = captureReceivedUnwrap(
    f.repository,
    f.identity,
    first.outerId,
    'reviewed_inbox_unlock'
  );
  if (!reader) throw Error('no original nested reader');
  const result = await unwrapReceivedEnvelope(reader);
  if (result.status !== 'authenticated')
    throw Error('no actual authenticated fixture');
  const room = admitReceivedConversation(
      result.envelope,
      kind === 'self_archive' || kind === 'receipt' ? 'self_archive' : 'inbound'
    ),
    cache = captureUnlockedSession(session, 'reviewed_messages_unlock');
  if (!room || !cache || acceptUnlockedConversation(cache, room) !== 'added')
    throw Error('no original cached room');
  const metadata = captureMessageMetadata(
      database,
      session,
      cache,
      'reviewed_local_message_metadata'
    ),
    directory =
      metadata &&
      captureConversationDirectory(
        session,
        metadata,
        'reviewed_conversation_directory'
      );
  if (!directory) throw Error('no genuine directory');
  const remembered = await rememberAdmittedConversation(
    directory,
    room,
    'reviewed_admitted_conversation_navigation'
  );
  if (remembered.status !== 'saved' && remembered.status !== 'existing')
    throw Error('no actual local UUID');
  const conversationId = selectedId ?? remembered.conversationId;
  closeConversationDirectory(directory);
  closeUnlockedSession(cache);
  stopReceivedUnwrap(reader);
  const policy = validateRelayPolicy(
    JSON.stringify({
      ...readRelayPolicy(f.context.policy),
      messagingEnabled: true
    })
  );
  if (!policy) throw Error('no original policy');
  const setup = createInboxSetupView({
    identity: f.identity,
    policy,
    lookup: () =>
      freshOwnLookup
        ? freshOwnLookup(f.owner, policy)
        : Promise.resolve(f.context.own),
    // Controlled qualification projection only, never production relay evidence.
    observeAccess: qualifiedReplyFixture
      ? (context) => ({
          ...context,
          receive: 'qualified_exercised',
          archive: 'qualified_exercised',
          current: () => !!identityMessagingOwnership(f.identity)?.current()
        })
      : undefined
  });
  if (!setup) throw Error('no original setup');
  const controller = createInboxView({
    identity: f.identity,
    setup,
    conversationId
  });
  if (!controller) throw Error('no genuine thread owner');
  const target = document.createElement('div');
  document.body.append(target);
  const mounted = mount(Panel, {
      target,
      props: { controller, conversationId }
    }),
    baseline = f.counts();
  async function native() {
    const tx = browserDatabaseTransaction(
        database,
        ['received_envelopes', 'conversations', 'private_sends'],
        'readonly'
      ),
      received = tx.objectStore('received_envelopes').getAll(),
      pairs = tx.objectStore('conversations').getAll(),
      sends = tx.objectStore('private_sends').getAll();
    return await new Promise<{
      received: unknown[];
      pairs: unknown[];
      sends: unknown[];
    }>((resolve, reject) => {
      tx.addEventListener('complete', () =>
        resolve({
          received: received.result,
          pairs: pairs.result,
          sends: sends.result
        })
      );
      tx.addEventListener('abort', () => reject(Error('native scan aborted')));
    });
  }
  let releaseReadLock: (() => void) | undefined,
    readLockPending: Promise<unknown> | undefined;
  async function holdReadLock() {
    if (readLockPending) throw Error('already held');
    await new Promise<void>((resolve) => {
      readLockPending = navigator.locks.request(
        'harvestcircle:owner:' + f.owner,
        async () => {
          await new Promise<void>((release) => {
            releaseReadLock = release;
            resolve();
          });
        }
      );
    });
  }
  async function releaseRead() {
    releaseReadLock?.();
    await readLockPending;
    releaseReadLock = undefined;
    readLockPending = undefined;
  }
  return {
    holdReadLock,
    releaseRead,
    controller,
    replyLookup(peer: string) {
      if (peer !== f.peer) throw Error('wrong authenticated fixture peer');
      return f.context.other;
    },
    owner: f.owner,
    peer: f.peer,
    conversationId,
    knownId: remembered.conversationId,
    firstRumor: first.rumorId,
    carrots,
    apples,
    actualDelivery,
    preparationCounts: baseline,
    native,
    snapshot: () => inboxViewSnapshot(controller),
    thread: () => inboxThreadSnapshot(controller, conversationId),
    checkSetup: () => checkInboxSetupView(setup),
    unlock: () => unlockInboxView(controller, 'reviewed_messages_unlock'),
    next: () => readNewInboxView(controller, 'reviewed_decrypt_batch'),
    delta: () => ({
      decrypts: f.counts().decrypts - baseline.decrypts,
      signs: f.counts().signs - baseline.signs,
      encrypts: f.counts().encrypts - baseline.encrypts
    }),
    copied: async () => ({
      snapshot: inboxThreadSnapshot({ ...controller }, conversationId),
      message: inboxMessageSnapshot(
        { ...controller },
        conversationId,
        first.rumorId
      ),
      resolve: await resolveInboxConversation(
        { ...controller },
        conversationId,
        'reviewed_conversation_navigation'
      ),
      read: await markInboxConversationDisplayed(
        { ...controller },
        conversationId,
        [first.rumorId],
        'reviewed_displayed_conversation'
      )
    }),
    resolve: (id: string) =>
      resolveInboxConversation(
        controller,
        id,
        'reviewed_conversation_navigation'
      ),
    stop: () => stopInboxView(controller),
    logout: () => f.disconnect(),
    mode: f.mode,
    settle: () => f.settle(),
    pending: () => f.pending(),
    async close() {
      await releaseRead();
      f.settle();
      closeInboxView(controller);
      closeInboxSetupView(setup);
      await unmount(mounted);
      closeBrowserDatabase(database);
      f.close();
      target.remove();
    }
  };
}
