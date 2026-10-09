import { test, expect } from 'vitest';
import { localUnreadRumors } from '../../src/lib/messaging/unread.ts';
import type { ConversationData } from '../../src/lib/messaging/admit-conversation.ts';
const owner = 'a'.repeat(64),
  peer = 'b'.repeat(64);
const message = (id: string, time = 1700000000): ConversationData => ({
  owner,
  peer,
  sender: peer,
  recipient: owner,
  role: 'inbound',
  rumorId: id,
  createdAt: time,
  content: 'Private test only',
  subject: null,
  replyTo: null
});
test('different envelopes of one verified rumor count once locally', () =>
  expect(
    localUnreadRumors(
      [message('1'.repeat(64)), message('1'.repeat(64))],
      owner,
      []
    )
  ).toEqual(['1'.repeat(64)]));
test('late old signed time is still locally unread', () =>
  expect(localUnreadRumors([message('1'.repeat(64), 1)], owner, [])).toEqual([
    '1'.repeat(64)
  ]));
test('read flag suppresses all repetitions independent of signed order', () =>
  expect(
    localUnreadRumors(
      [message('1'.repeat(64), 200), message('1'.repeat(64), 1)],
      owner,
      ['1'.repeat(64)]
    )
  ).toEqual([]));
test('owner isolation does not adopt another local namespace', () =>
  expect(localUnreadRumors([message('1'.repeat(64))], peer, [])).toEqual([]));
test('own archive is not an incoming unread notification', () =>
  expect(
    localUnreadRumors(
      [
        {
          ...message('1'.repeat(64)),
          sender: owner,
          recipient: peer,
          role: 'self_archive'
        }
      ],
      owner,
      []
    )
  ).toEqual([]));
test('reordering never changes the unique locally unread set', () => {
  const a = message('1'.repeat(64), 300),
    b = message('2'.repeat(64), 1);
  expect(new Set(localUnreadRumors([a, b, a], owner, []))).toEqual(
    new Set(localUnreadRumors([b, a, b], owner, []))
  );
});
