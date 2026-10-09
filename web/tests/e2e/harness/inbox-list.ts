import { mount, unmount } from 'svelte';
import Panel from './inbox-list-panel.svelte';
import { makeFixture as makeMetadataFixture } from './message-metadata.ts';
import {
  createInboxSetupView,
  checkInboxSetupView,
  closeInboxSetupView
} from '../../../src/lib/messaging/inbox-setup-view.ts';
import {
  createInboxView,
  inboxViewSnapshot,
  inboxListSnapshot,
  unlockInboxView,
  readNewInboxView,
  checkInboxView,
  loadOlderInboxView,
  openInboxConversation,
  stopInboxView,
  closeInboxView
} from '../../../src/lib/messaging/inbox-view.ts';
import { identityMessagingOwnership } from '../../../src/lib/runtime/identity-session.ts';
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
  admitInboxEnvelope,
  retainInboxEnvelope
} from '../../../src/lib/persistence/inbox-envelope-repository.ts';
import { finalizeEvent } from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
let navigations = Array.from<string>([]);
// Harness-only alias for Kit navigation. Production retains actual Kit goto.
export function goto(href: string) {
  navigations.push(href);
  return Promise.resolve();
}
export async function renderFixture(
  kind: 'inbound' | 'self_archive' | 'hostile_text' = 'inbound',
  qualified = false,
  variant: 'single' | 'duplicate' | 'mixed' = 'single'
) {
  const f = await makeMetadataFixture(kind);
  const original = identityMessagingOwnership(f.identity);
  if (!original) throw Error('no original identity');
  if (variant === 'duplicate') await f.anotherWrap(false);
  if (variant === 'mixed') {
    const record = (await f.row())!.record;
    const envelope = admitInboxEnvelope(
      record.self.wire,
      f.owner,
      'wss://archive.example.org',
      102
    );
    const session = f.navigationInputs().privateSession;
    if (
      !envelope ||
      (await retainInboxEnvelope(f.repository, session, envelope)).status !==
        'retained'
    )
      throw Error('no actual self archive');
  }
  async function native() {
    const opened = await openBrowserDatabase();
    if (opened.state !== 'ready') throw Error('no actual DB');
    try {
      const tx = browserDatabaseTransaction(
          opened.owner,
          ['received_envelopes', 'conversations'],
          'readonly'
        ),
        received = tx.objectStore('received_envelopes').getAll(),
        pairs = tx.objectStore('conversations').getAll();
      return await new Promise<{ received: unknown[]; pairs: unknown[] }>(
        (resolve, reject) => {
          tx.addEventListener('complete', () =>
            resolve({ received: received.result, pairs: pairs.result })
          );
          tx.addEventListener('abort', () => reject(Error('scan aborted')));
        }
      );
    } finally {
      closeBrowserDatabase(opened.owner);
    }
  }
  const initial = await native(),
    raw = initial.received[0] as { wire: string },
    record = JSON.parse(raw.wire) as { outer: string };
  // Produce a relay-only valid old outer with the same authenticated signed seal.
  // Both timestamps are set before signing. Nothing signed is edited afterward.
  const outer = JSON.parse(record.outer) as {
    pubkey: string;
    content: string;
    created_at: number;
  };
  const ownerSecret = new Uint8Array(32).fill(83),
    outerSecret = new Uint8Array(32).fill(86);
  let olderWire: string;
  try {
    const decryptKey = nip44.v2.utils.getConversationKey(
      ownerSecret,
      outer.pubkey
    );
    let seal: string;
    try {
      seal = nip44.v2.decrypt(outer.content, decryptKey);
    } finally {
      decryptKey.fill(0);
    }
    const encryptKey = nip44.v2.utils.getConversationKey(outerSecret, f.owner);
    let content: string;
    try {
      content = nip44.v2.encrypt(seal, encryptKey);
    } finally {
      encryptKey.fill(0);
    }
    olderWire = JSON.stringify(
      finalizeEvent(
        {
          kind: 1059,
          created_at: outer.created_at - 120,
          tags: [['p', f.owner]],
          content
        },
        outerSecret
      )
    );
  } finally {
    ownerSecret.fill(0);
    outerSecret.fill(0);
  }
  const policy = validateRelayPolicy(
    JSON.stringify({
      ...readRelayPolicy(f.context.policy),
      messagingEnabled: true
    })
  );
  if (!policy) throw Error('no policy');
  let access = true;
  const setup = createInboxSetupView({
    identity: f.identity,
    policy,
    lookup: () => Promise.resolve(f.context.own),
    observeAccess: qualified
      ? (context) => ({
          ...context,
          receive: 'qualified_exercised',
          archive: 'qualified_exercised',
          current: () => access && original.current()
        })
      : undefined
  });
  if (!setup) throw Error('no setup');
  const controller = createInboxView({ identity: f.identity, setup });
  if (!controller) throw Error('no original page');
  const target = document.createElement('div');
  document.body.append(target);
  const mounted = mount(Panel, { target, props: { controller } });
  navigations = [];
  const baseline = f.counts();
  return {
    controller,
    owner: f.owner,
    peer: f.peer,
    rumorId: f.rumorId,
    olderWire,
    native,
    snapshot: () => inboxViewSnapshot(controller),
    list: () => inboxListSnapshot(controller),
    navigations: () => navigations.slice(),
    checkSetup: () => checkInboxSetupView(setup),
    unlock: () => unlockInboxView(controller, 'reviewed_messages_unlock'),
    next: () => readNewInboxView(controller, 'reviewed_decrypt_batch'),
    check: () => checkInboxView(controller, 'reviewed_foreground_inbox'),
    older: (review: unknown = 'reviewed_load_older') =>
      loadOlderInboxView(controller, review),
    open: (
      peer: unknown = f.peer,
      review: unknown = 'reviewed_admitted_conversation_navigation'
    ) => openInboxConversation(controller, peer, review),
    delta: () => ({
      decrypts: f.counts().decrypts - baseline.decrypts,
      signs: f.counts().signs - baseline.signs,
      encrypts: f.counts().encrypts - baseline.encrypts
    }),
    copied: async () => ({
      list: inboxListSnapshot({ ...controller }),
      open: await openInboxConversation(
        { ...controller },
        f.peer,
        'reviewed_admitted_conversation_navigation'
      ),
      older: await loadOlderInboxView({ ...controller }, 'reviewed_load_older')
    }),
    async markRead() {
      if (
        (await f.unlock()) !== 'authenticated' ||
        !f.admit(kind === 'self_archive' ? 'self_archive' : 'inbound') ||
        !f.captureCache()
      )
        throw Error('no genuine explicit read proof');
      f.cache();
      if (!(await f.captureMetadata())) throw Error('no original metadata');
      return await f.display();
    },
    stop: () => stopInboxView(controller),
    logout: () => f.disconnect(),
    mode: f.mode,
    settle: () => f.settle(),
    pending: () => f.pending(),
    revokeAccess: () => {
      access = false;
    },
    async close() {
      f.settle();
      closeInboxView(controller);
      closeInboxSetupView(setup);
      await unmount(mounted);
      f.close();
      target.remove();
    }
  };
}
