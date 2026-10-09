import { test, expect } from 'vitest';
import { getPublicKey } from 'applesauce-core/helpers';
import { groupInboxMessages } from '../../src/lib/messaging/inbox-list.ts';
import {
  inboxListSnapshot,
  openInboxConversation,
  loadOlderInboxView,
  type InboxView
} from '../../src/lib/messaging/inbox-view.ts';
import type { ConversationData } from '../../src/lib/messaging/admit-conversation.ts';
const owner = getPublicKey(new Uint8Array(32).fill(83)),
  peer = getPublicKey(new Uint8Array(32).fill(84)),
  other = getPublicKey(new Uint8Array(32).fill(85));
const message = (
  id = '1'.repeat(64),
  role: 'inbound' | 'self_archive' = 'inbound',
  time = 1700000000,
  content = 'Plain private fixture'
): ConversationData => ({
  owner,
  peer,
  sender: role === 'inbound' ? peer : owner,
  recipient: role === 'inbound' ? owner : peer,
  role,
  rumorId: id,
  createdAt: time,
  content,
  subject: null,
  replyTo: null
});
test('authenticated incoming projection groups a new peer under local requests', () => {
  const view = groupInboxMessages(owner, [message()], []);
  expect(view.status).toBe('ready');
  expect(view.requests).toHaveLength(1);
  expect(view.conversations).toEqual([]);
  expect(view.requests[0]).toMatchObject({ peer, count: 1, unread: 1 });
});
test('actual self-archive history makes one known pair without a required acceptance task', () => {
  const view = groupInboxMessages(
    owner,
    [message(), message('2'.repeat(64), 'self_archive', 1700000001)],
    []
  );
  expect(view.requests).toEqual([]);
  expect(view.conversations).toHaveLength(1);
  expect(view.conversations[0]).toMatchObject({ peer, count: 2, unread: 1 });
  expect(view.conversations[0].latest.role).toBe('self_archive');
});
test('repeated wraps of the same verified rumor do not duplicate local New or rows', () => {
  const view = groupInboxMessages(owner, [message(), message()], []);
  expect(view.requests).toHaveLength(1);
  expect(view.requests[0]).toMatchObject({ count: 1, unread: 1 });
});
test('read state is by verified rumor rather than signed timestamp or outgoing archive', () => {
  const view = groupInboxMessages(
    owner,
    [
      message('1'.repeat(64), 'inbound', 1),
      message('2'.repeat(64), 'self_archive', 1700000002)
    ],
    ['1'.repeat(64)]
  );
  expect(view.conversations[0].unread).toBe(0);
  expect(
    groupInboxMessages(owner, [message('3'.repeat(64), 'inbound', 1)], [])
      .requests[0].unread
  ).toBe(1);
});
test('latest sender-time-text selection is deterministic and does not parse a body link', () => {
  const body = '<script>private</script> https://untrusted.example.org';
  const input = [
    message('2'.repeat(64), 'inbound', 1700000000, body),
    message('1'.repeat(64))
  ];
  const a = groupInboxMessages(owner, input, []),
    b = groupInboxMessages(owner, input.slice().reverse(), []);
  expect(a).toEqual(b);
  expect(a.requests[0].latest.content).toBe(body);
  expect(a.requests[0].latest.rumorId).toBe('2'.repeat(64));
});
test('invalid or foreign detached rows cannot enter an owner projection', () => {
  for (const row of [
    { ...message(), owner: other },
    { ...message(), peer: 'invalid' },
    { ...message(), createdAt: NaN },
    { ...message(), role: 'group' }
  ])
    expect(groupInboxMessages(owner, [row as ConversationData], [])).toEqual({
      status: 'unavailable',
      requests: [],
      conversations: []
    });
});
test('conflicting bytes of one rumor fail closed instead of substituting a preview', () =>
  expect(
    groupInboxMessages(
      owner,
      [message(), message('1'.repeat(64), 'inbound', 1700000000, 'different')],
      []
    )
  ).toEqual({ status: 'unavailable', requests: [], conversations: [] }));
test('SSR/copied scalar page tokens cannot inspect rows, map peers or request older history', async () => {
  const fake = { owner, unlocked: true, ready: true } as unknown as InboxView;
  expect(inboxListSnapshot(fake)).toEqual({
    status: 'unavailable',
    requests: [],
    conversations: []
  });
  expect(
    await openInboxConversation(
      fake,
      peer,
      'reviewed_admitted_conversation_navigation'
    )
  ).toBeUndefined();
  expect(await loadOlderInboxView(fake, 'reviewed_load_older')).toBe(false);
});
