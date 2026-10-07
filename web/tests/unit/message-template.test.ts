import { expect, it } from 'vitest';
import {
  messageFromWireParts,
  messageToWireParts
} from '../../src/lib/contracts/message-v1/index.ts';
import { buildUnsignedMessageTemplate } from '../../src/lib/nostr/message-template.ts';
import { inspectLocalTemplate } from '../../src/lib/nostr/local-template.ts';
const peer = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
const parent = '1'.repeat(64);
const input = (extra: Record<string, unknown> = {}) =>
  JSON.stringify({
    recipients: [{ public_key: peer, relay_url: null }],
    content: 'Could I buy carrots?\nPlease reply here.',
    reply_to: null,
    subject: null,
    ...extra
  });
it('first enquiry is readable unsigned14 with no reply parent or new order schema', () => {
  const value = buildUnsignedMessageTemplate(input());
  expect(value).toEqual({
    kind: 14,
    tags: [['p', peer]],
    content: 'Could I buy carrots?\nPlease reply here.'
  });
  expect(value).not.toHaveProperty('sig');
  expect(value).not.toHaveProperty('pubkey');
  expect(value).not.toHaveProperty('id');
  expect(
    inspectLocalTemplate(
      JSON.stringify({ ...value, pubkey: peer, created_at: 100 }),
      peer,
      14
    )
  ).toBeUndefined();
  expect(
    inspectLocalTemplate(
      JSON.stringify({ ...value, kind: 13, pubkey: peer, created_at: 100 }),
      peer,
      13
    )
  ).toBeUndefined();
});
it('reply parent and optional subject use the exact shared p/e/subject fields', () => {
  const raw = input({
    reply_to: { id: parent, relays: 'wss://hint.example.org' },
    subject: 'Carrots'
  });
  const parts = messageToWireParts(raw);
  expect(parts?.tags).toEqual([
    ['p', peer],
    ['e', parent, 'wss://hint.example.org'],
    ['subject', 'Carrots']
  ]);
  expect(messageFromWireParts(JSON.stringify(parts))).toEqual(JSON.parse(raw));
});
it('canonical public keys and parent hashes go beyond generic nonempty codec checks', () => {
  for (const public_key of ['alice', peer.toUpperCase(), 'f'.repeat(64)])
    expect(
      messageToWireParts(input({ recipients: [{ public_key }] }))
    ).toBeUndefined();
  for (const id of [
    'product',
    parent.toUpperCase().replace('1', 'A'),
    '0'.repeat(63)
  ])
    expect(messageToWireParts(input({ reply_to: { id } }))).toBeUndefined();
});
it('body limits count UTF8 without trimming or truncating the shared text', () => {
  for (const content of [
    'a'.repeat(4096),
    'é'.repeat(2048),
    '\ufeff',
    '\u001c'
  ])
    expect(messageToWireParts(input({ content }))?.content).toBe(content);
  for (const content of [
    'a'.repeat(4097),
    'é'.repeat(2049),
    '',
    ' \n\u0085',
    '\ud800'
  ])
    expect(messageToWireParts(input({ content }))).toBeUndefined();
});
it('encoded parts limits apply in addition to the four KiB body limit', () => {
  expect(
    messageToWireParts(input({ content: '"'.repeat(4096) }))
  ).toBeUndefined();
  expect(
    messageToWireParts(
      input({ content: 'a'.repeat(4096), subject: 'b'.repeat(4096) })
    )
  ).toBeUndefined();
  expect(
    messageFromWireParts(
      JSON.stringify({
        kind: 14,
        tags: [['p', peer]],
        content: 'a'.repeat(8193)
      })
    )
  ).toBeUndefined();
});
it('authoring cannot add an order tag or replace plain text with a JSON body object', () => {
  expect(messageToWireParts(input({ order: 'new-protocol' }))).toBeUndefined();
  expect(messageToWireParts(input({ tags: [['order', 'x']] }))).toBeUndefined();
  expect(
    messageToWireParts(input({ content: { order: 'x' } }))
  ).toBeUndefined();
  const value = messageToWireParts(
    JSON.stringify({
      recipients: [{ public_key: peer }],
      content: 'ordinary text'
    })
  );
  expect(value?.tags).toEqual([['p', peer]]);
  expect(messageFromWireParts(JSON.stringify(value))?.reply_to).toBeNull();
});
it('reader matches first optional-tag semantics and ignores unknown tags without granting context', () => {
  const value = messageFromWireParts(
    JSON.stringify({
      kind: 14,
      content: 'hello',
      tags: [
        ['order', 'x'],
        ['p', peer],
        ['e', parent],
        ['e'],
        ['subject', 'first'],
        ['subject', 'second']
      ]
    })
  );
  expect(value?.reply_to).toEqual({ id: parent, relays: null });
  expect(value?.subject).toBe('first');
  expect(value).not.toHaveProperty('order');
  expect(
    messageFromWireParts(
      JSON.stringify({ kind: 14, content: 'hello', tags: [['p', peer], ['p']] })
    )
  ).toBeUndefined();
  expect(
    messageFromWireParts(
      JSON.stringify({ kind: 15, content: 'hello', tags: [['p', peer]] })
    )
  ).toBeUndefined();
});
