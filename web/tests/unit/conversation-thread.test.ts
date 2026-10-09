import { describe, expect, it } from 'vitest';
import { getPublicKey, finalizeEvent } from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { projectConversationThread } from '../../src/lib/messaging/conversation-thread.ts';
import type { ConversationData } from '../../src/lib/messaging/admit-conversation.ts';
import { encodeProductReference } from '../../src/lib/nostr/references.ts';
import { decodePrivateRecord } from '../../src/lib/persistence/private-records.ts';
import {
  resolveInboxConversation,
  inboxThreadSnapshot,
  inboxMessageSnapshot,
  markInboxConversationDisplayed,
  type InboxView
} from '../../src/lib/messaging/inbox-view.ts';

const owner = getPublicKey(new Uint8Array(32).fill(83)),
  peer = getPublicKey(new Uint8Array(32).fill(84)),
  other = getPublicKey(new Uint8Array(32).fill(85)),
  command = '12345678-1234-4234-8234-123456789abc';
function message(overrides: Partial<ConversationData> = {}): ConversationData {
  return {
    owner,
    peer,
    sender: peer,
    recipient: owner,
    role: 'inbound',
    rumorId: 'a'.repeat(64),
    createdAt: 1700000000,
    content: 'Actual message text',
    subject: null,
    replyTo: null,
    ...overrides
  };
}
function archive(overrides: Partial<ConversationData> = {}): ConversationData {
  return message({
    role: 'self_archive',
    sender: owner,
    recipient: peer,
    ...overrides
  });
}
// Structural local receipt projection fixture; no factory, native commit or
// real target delivery authority is claimed by this pure unit fixture.
function receipt(role: 'peer' | 'self_archive', rumorHash = 'a'.repeat(64)) {
  const key = new Uint8Array(32).fill(83);
  try {
    function artifact(destination: string) {
      const conversation = nip44.v2.utils.getConversationKey(key, destination);
      try {
        const event = finalizeEvent(
          {
            kind: 1059,
            created_at: 1700000000,
            tags: [['p', destination]],
            content: nip44.v2.encrypt(
              'Structural codec fixture only',
              conversation
            )
          },
          key
        );
        return { eventId: event.id, wire: JSON.stringify(event) };
      } finally {
        conversation.fill(0);
      }
    }
    function route(
      author: string,
      kind: 'peer' | 'self_archive',
      origin: string
    ) {
      return {
        author,
        role: kind,
        targets: [origin],
        knownBase: { author, id: 'b'.repeat(64), createdAt: 1 },
        sources: [{ source: 'wss://discovery.example.org', state: 'eose' }]
      };
    }
    const self = artifact(owner),
      peerArtifact = artifact(peer),
      row = {
        schema: 1,
        family: 'private_send_operation',
        owner,
        id: command,
        revision: 3,
        peer,
        rumorHash,
        createdAt: 1700000000,
        self,
        peerArtifact,
        deliveryPlan: {
          state: 'prepared',
          routes: {
            peer: route(peer, 'peer', 'wss://peer.example.org'),
            archive: route(owner, 'self_archive', 'wss://archive.example.org')
          }
        },
        receipts: [
          {
            actionId: '12345678-1234-4234-8234-123456789abd',
            role,
            origin:
              role === 'peer'
                ? 'wss://peer.example.org'
                : 'wss://archive.example.org',
            attempt: 1,
            eventId: role === 'peer' ? peerArtifact.eventId : self.eventId,
            status: 'accepted',
            observedAtMilliseconds: 100,
            readbackWire: null
          }
        ]
      },
      decoded = decodePrivateRecord(JSON.stringify(row), owner, command);
    if (!decoded.ok) throw Error('invalid structural codec fixture');
    return decoded.value;
  } finally {
    key.fill(0);
  }
}
describe('detached plain chronological pair presentation grants no custody', () => {
  it('orders by actual inner time then stable rumor identity without room-level product state', () => {
    const first = message({
        rumorId: 'b'.repeat(64),
        createdAt: 2,
        content: 'About apples'
      }),
      second = archive({ createdAt: 1, content: 'About carrots' }),
      view = projectConversationThread(owner, peer, [first, second], []);
    expect(view.status).toBe('ready');
    expect(view.messages.map((row) => row.data.content)).toEqual([
      'About carrots',
      'About apples'
    ]);
    expect(view.messages.map((row) => row.data.role)).toEqual([
      'self_archive',
      'inbound'
    ]);
  });
  it('deduplicates the same verified rumor and returns detached independent rows', () => {
    const original = message(),
      view = projectConversationThread(
        owner,
        peer,
        [original, { ...original }],
        []
      );
    expect(view.messages).toHaveLength(1);
    expect(view.messages[0].data).not.toBe(original);
    expect(original.content).toBe('Actual message text');
  });
  it('rejects conflicting same-rumor data instead of substituting later bytes', () => {
    expect(
      projectConversationThread(
        owner,
        peer,
        [message(), message({ content: 'Changed' })],
        []
      ).status
    ).toBe('unavailable');
  });
  it('rejects foreign owner, different pair and false sender roles', () => {
    for (const row of [
      message({ owner: other }),
      message({ peer: other }),
      message({ sender: owner })
    ])
      expect(projectConversationThread(owner, peer, [row], []).status).toBe(
        'unavailable'
      );
  });
  it('keeps canonical public citations on their own message without promoting a listing claim', () => {
    const carrots = encodeProductReference({
        kind: 30402,
        pubkey: peer,
        identifier: 'carrots'
      })!,
      apples = encodeProductReference({
        kind: 30402,
        pubkey: peer,
        identifier: 'apples'
      })!,
      view = projectConversationThread(
        owner,
        peer,
        [
          message({ content: 'Product: nostr:' + carrots }),
          message({
            rumorId: 'b'.repeat(64),
            content: 'Product: nostr:' + apples
          })
        ],
        []
      );
    expect(view.messages[0].citations.map((row) => row.naddr)).toEqual([
      carrots
    ]);
    expect(view.messages[1].citations.map((row) => row.naddr)).toEqual([
      apples
    ]);
  });
  it('preserves quoted price and hostile text as text without HTML or external URL projection', () => {
    const content =
        '<img src=x> Quoted price 4 GBP https://untrusted.example.org nostr:naddr1invalid',
      view = projectConversationThread(owner, peer, [message({ content })], []);
    expect(view.messages[0].data.content).toBe(content);
    expect(view.messages[0].citations).toEqual([]);
  });
  it('recognizes only a present parent in the same admitted pair without an external private-ID lookup', () => {
    const parent = message(),
      reply = archive({ rumorId: 'b'.repeat(64), replyTo: parent.rumorId }),
      missing = message({ rumorId: 'c'.repeat(64), replyTo: 'd'.repeat(64) }),
      view = projectConversationThread(
        owner,
        peer,
        [parent, reply, missing],
        []
      );
    expect(
      view.messages.find((row) => row.data.rumorId === reply.rumorId)?.parent
    ).toBe('present');
    expect(
      view.messages.find((row) => row.data.rumorId === missing.rumorId)?.parent
    ).toBe('missing');
  });
  it('never upgrades sender archive acceptance to recipient acceptance or reports incoming remote reads', () => {
    const view = projectConversationThread(
      owner,
      peer,
      [archive()],
      [receipt('self_archive')]
    );
    expect(view.messages[0].delivery).toContain('Recipient delivery unknown');
    expect(view.messages[0].delivery).not.toContain(
      'Accepted by recipient inbox relay'
    );
    expect(
      projectConversationThread(owner, peer, [message()], [receipt('peer')])
        .messages[0].delivery
    ).toBeNull();
  });
  it('uses only an exact local owner peer rumor receipt projection, never a different message fact', () => {
    expect(
      projectConversationThread(owner, peer, [archive()], [receipt('peer')])
        .messages[0].delivery
    ).toContain('Accepted by recipient inbox relay');
    expect(
      projectConversationThread(
        owner,
        peer,
        [archive()],
        [receipt('peer', 'b'.repeat(64))]
      ).messages[0].delivery
    ).toContain('Recipient delivery unknown');
  });
  it('unavailable copied and SSR controllers cannot import thread bodies or native display custody', async () => {
    const fake = {} as InboxView;
    expect(inboxThreadSnapshot(fake, command).status).toBe('unavailable');
    expect(inboxMessageSnapshot(fake, command, 'a'.repeat(64))).toBeUndefined();
    expect(
      await resolveInboxConversation(
        fake,
        command,
        'reviewed_conversation_navigation'
      )
    ).toBe(false);
    expect(
      await markInboxConversationDisplayed(
        fake,
        command,
        ['a'.repeat(64)],
        'reviewed_displayed_conversation'
      )
    ).toBe(false);
  });
});
