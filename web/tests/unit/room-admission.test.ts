import { describe, it, expect } from 'vitest';
import {
  getPublicKey,
  getEventHash,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import {
  inspectConversationData,
  admitReceivedConversation,
  conversationSnapshot
} from '../../src/lib/messaging/admit-conversation.ts';
import type { ReceivedNestedEnvelope } from '../../src/lib/nostr/unwrap-admission.ts';
const owner = getPublicKey(new Uint8Array(32).fill(83)),
  peer = getPublicKey(new Uint8Array(32).fill(84)),
  other = getPublicKey(new Uint8Array(32).fill(85));
function rumor(
  sender = peer,
  recipient = owner,
  content = 'Untrusted plain text',
  extra: string[][] = []
) {
  const template: UnsignedEvent = {
    pubkey: sender,
    kind: 14,
    created_at: 1700000000,
    tags: [['p', recipient], ...extra],
    content
  };
  return JSON.stringify({ id: getEventHash(template), ...template });
}
describe('detached exact room inspection grants no authenticated custody', () => {
  it('inbound owner membership selects authenticated sender as peer', () =>
    expect(inspectConversationData(rumor(), owner, 'inbound')).toMatchObject({
      owner,
      peer,
      sender: peer,
      recipient: owner,
      role: 'inbound'
    }));
  it('self archive retains original recipient instead of changing rumor room', () =>
    expect(
      inspectConversationData(rumor(owner, peer), owner, 'self_archive')
    ).toMatchObject({
      owner,
      peer,
      sender: owner,
      recipient: peer,
      role: 'self_archive'
    }));
  it('third party rumor excludes connected account', () =>
    expect(
      inspectConversationData(rumor(peer, other), owner, 'inbound')
    ).toBeUndefined());
  it('group and duplicate-recipient fanout are rejected', () => {
    expect(
      inspectConversationData(
        rumor(peer, owner, 'text', [['p', other]]),
        owner,
        'inbound'
      )
    ).toBeUndefined();
    expect(
      inspectConversationData(
        rumor(peer, owner, 'text', [['p', owner]]),
        owner,
        'inbound'
      )
    ).toBeUndefined();
  });
  it('self request has no other participant', () =>
    expect(
      inspectConversationData(rumor(owner, owner), owner, 'self_archive')
    ).toBeUndefined());
  it('expected inbound/archive role must match actual authenticated sender', () => {
    expect(
      inspectConversationData(rumor(), owner, 'self_archive')
    ).toBeUndefined();
    expect(
      inspectConversationData(rumor(owner, peer), owner, 'inbound')
    ).toBeUndefined();
    expect(
      inspectConversationData(rumor(), owner, 'automatic')
    ).toBeUndefined();
  });
  it('subject URLs and body contact strings cannot select another peer', () =>
    expect(
      inspectConversationData(
        rumor(peer, owner, `Contact ${other} https://untrusted.example.org`, [
          ['subject', other]
        ]),
        owner,
        'inbound'
      )
    ).toMatchObject({ peer, sender: peer }));
  it('unknown parent remains a detached private hash without public lookup', () =>
    expect(
      inspectConversationData(
        rumor(peer, owner, 'text', [
          ['e', 'a'.repeat(64), 'wss://untrusted.example.org']
        ]),
        owner,
        'inbound'
      )
    ).toMatchObject({ peer, replyTo: 'a'.repeat(64) }));
  it('fresh rumor hash is required before room inspection', () => {
    const row = JSON.parse(rumor()) as Record<string, unknown>;
    expect(
      inspectConversationData(
        JSON.stringify({ ...row, id: '0'.repeat(64) }),
        owner,
        'inbound'
      )
    ).toBeUndefined();
  });
  it('malformed and caller object values are isolated', () => {
    expect(inspectConversationData('{}', owner, 'inbound')).toBeUndefined();
    expect(
      inspectConversationData(JSON.parse(rumor()) as unknown, owner, 'inbound')
    ).toBeUndefined();
    expect(inspectConversationData(rumor(), other, 'inbound')).toBeUndefined();
    expect(inspectConversationData(rumor(), owner, 'inbound')).toBeDefined();
  });
  it('unsupported tags and signed rumor cannot become room data', () => {
    expect(
      inspectConversationData(
        rumor(peer, owner, 'text', [['file', 'https://untrusted.example.org']]),
        owner,
        'inbound'
      )
    ).toBeUndefined();
    const row = JSON.parse(rumor()) as Record<string, unknown>;
    expect(
      inspectConversationData(
        JSON.stringify({ ...row, sig: '0'.repeat(128) }),
        owner,
        'inbound'
      )
    ).toBeUndefined();
  });
  it('structural nested objects and copied detached rooms grant no proof', () => {
    const fake = {} as ReceivedNestedEnvelope;
    expect(admitReceivedConversation(fake, 'inbound')).toBeUndefined();
    expect(
      conversationSnapshot(
        inspectConversationData(
          rumor(),
          owner,
          'inbound'
        ) as unknown as Parameters<typeof conversationSnapshot>[0]
      )
    ).toBeUndefined();
  });
});
