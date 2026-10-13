import { expect, it } from 'vitest';
import { getEventHash, type UnsignedEvent } from 'applesauce-core/helpers';
import {
  incomingGiftWrapTags,
  parseIncomingLayer,
  readIncomingPairRumor
} from '../../src/lib/nostr/incoming-wire.ts';
import { inspectConversationData } from '../../src/lib/messaging/admit-conversation.ts';
import { messageToWireParts } from '../../src/lib/contracts/message-v1/index.ts';
const sender =
    '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
  owner = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
function rumor(tags: string[][], content = '  Original readable question\n') {
  const template: UnsignedEvent = {
    kind: 14,
    pubkey: sender,
    created_at: 1700000000,
    tags,
    content
  };
  return { id: getEventHash(template), ...template };
}
it('accepts bounded intended-recipient hints and inert outer metadata without routing authority', () => {
  for (const tags of [
    [['p', owner]],
    [['p', owner, 'wss://untrusted.example.org']],
    [
      ['nonce', '7', '8'],
      ['p', owner, ''],
      ['vendor', 'untrusted inert text']
    ]
  ])
    expect(incomingGiftWrapTags(tags, owner)).toBe(true);
  for (const tags of [
    [],
    [['p', sender]],
    [
      ['p', owner],
      ['p', owner]
    ],
    [
      ['p', owner],
      ['p', sender]
    ],
    [['p', owner, 'hint', 'extra']],
    [
      ['p', owner],
      ['expiration', '1900000000']
    ],
    [['p', owner], []],
    [
      ['p', owner],
      ['vendor', 'x'.repeat(4097)]
    ],
    [['p', owner], ...Array.from({ length: 1024 }, () => ['vendor'])],
    [
      ['p', owner],
      ['vendor', '\ud800']
    ]
  ])
    expect(incomingGiftWrapTags(tags, owner)).toBe(false);
});
it('hashes original tag order and preserves bounded q/unknown metadata while projecting semantics', () => {
  const tags = [
    ['subject', 'Question'],
    ['q', 'b'.repeat(64), 'wss://untrusted.example.org', sender],
    ['vendor', 'untrusted inert text'],
    ['e', 'a'.repeat(64), 'wss://parent.example.org'],
    ['p', owner, 'wss://recipient.example.org']
  ];
  const value = rumor(tags),
    raw = '\n' + JSON.stringify(value, null, 2) + '\n';
  const incoming = readIncomingPairRumor(raw, sender);
  expect(incoming?.original).toEqual(value);
  expect(incoming?.message).toEqual({
    recipients: [
      { public_key: owner, relay_url: 'wss://recipient.example.org' }
    ],
    content: value.content,
    reply_to: { id: 'a'.repeat(64), relays: 'wss://parent.example.org' },
    subject: 'Question'
  });
  expect(inspectConversationData(raw, owner, 'inbound')).toMatchObject({
    owner,
    peer: sender,
    sender,
    recipient: owner,
    rumorId: value.id,
    content: value.content,
    subject: 'Question',
    replyTo: 'a'.repeat(64)
  });
  expect(inspectConversationData(raw, sender, 'self_archive')).toMatchObject({
    peer: owner,
    rumorId: value.id,
    content: value.content
  });
  expect(inspectConversationData(raw, owner, 'self_archive')).toBeUndefined();
  expect(
    inspectConversationData(raw, '0'.repeat(64), 'inbound')
  ).toBeUndefined();
  // The receive profile does not rewrite the strict Lib189 outgoing writer.
  expect(messageToWireParts(JSON.stringify(incoming?.message))?.tags).toEqual([
    ['p', owner, 'wss://recipient.example.org'],
    ['e', 'a'.repeat(64), 'wss://parent.example.org'],
    ['subject', 'Question']
  ]);
});
it('rejects ambiguous recipients/parents/subjects and unsupported retention/file semantics', () => {
  const badTags = [
    [],
    [['p', sender]],
    [
      ['p', owner],
      ['p', owner]
    ],
    [
      ['p', owner],
      ['p', sender]
    ],
    [['p', owner, 'hint', 'extra']],
    [
      ['p', owner],
      ['subject', 'A'],
      ['subject', 'B']
    ],
    [
      ['p', owner],
      ['subject', 'A', 'extra']
    ],
    [
      ['p', owner],
      ['e', 'a'.repeat(64)],
      ['e', 'b'.repeat(64)]
    ],
    [
      ['p', owner],
      ['e', 'a'.repeat(64), 'hint', 'marker']
    ],
    [
      ['p', owner],
      ['e', 'malformed']
    ],
    [
      ['p', owner],
      ['expiration', '1900000000']
    ],
    [
      ['p', owner],
      ['file', 'untrusted']
    ],
    [
      ['p', owner],
      ['imeta', 'untrusted']
    ],
    [
      ['p', owner],
      ['vendor', '\ud800']
    ]
  ];
  for (const tags of badTags)
    expect(readIncomingPairRumor(JSON.stringify(rumor(tags)))).toBeUndefined();
});
it('altered original tags/hash/author/kind and noncanonical numeric spellings never grant admission', () => {
  const value = rumor([
    ['subject', 'Question'],
    ['p', owner]
  ]);
  const raw = JSON.stringify(value);
  for (const modified of [
    { ...value, id: '0'.repeat(64) },
    { ...value, tags: [...value.tags].reverse() },
    { ...value, pubkey: owner },
    { ...value, kind: 15 },
    { ...value, sig: '0'.repeat(128) },
    { ...value, verified: true },
    { ...value, content: 'x'.repeat(4097) }
  ])
    expect(readIncomingPairRumor(JSON.stringify(modified))).toBeUndefined();
  expect(readIncomingPairRumor(raw, owner)).toBeUndefined();
  for (const modified of [
    raw.replace('"kind":14', '"kind":14.0'),
    raw.replace('1700000000', '1700000000.0'),
    raw.replace('1700000000', '1.7e9'),
    '{"kind":14,' + raw.slice(1),
    '{"\\u006bkind":14,' + raw.slice(1),
    '{"\\u006b\\u0069\\u006e\\u0064":14,' + raw.slice(1)
  ]) {
    expect(readIncomingPairRumor(modified)).toBeUndefined();
    expect(inspectConversationData(modified, owner, 'inbound')).toBeUndefined();
  }
});
it('accepts exact body/tag/element limits and rejects their immediate excess', () => {
  expect(
    readIncomingPairRumor(
      JSON.stringify(rumor([['p', owner]], 'x'.repeat(4096)))
    )
  ).toBeDefined();
  expect(
    readIncomingPairRumor(
      JSON.stringify(rumor([['p', owner]], 'x'.repeat(4097)))
    )
  ).toBeUndefined();
  const count = [['p', owner], ...Array.from({ length: 1023 }, () => ['x'])];
  expect(readIncomingPairRumor(JSON.stringify(rumor(count)))).toBeDefined();
  expect(incomingGiftWrapTags(count, owner)).toBe(true);
  expect(incomingGiftWrapTags([...count, ['x']], owner)).toBe(false);
  const elements = [
    ['p', owner, ''],
    ['vendor', '', '', '', ''],
    ...Array.from({ length: 1022 }, () => ['vendor', '', '', ''])
  ];
  expect(incomingGiftWrapTags(elements, owner)).toBe(true);
  expect(
    incomingGiftWrapTags(
      elements.map((row, i) => (i === 1 ? [...row, ''] : row)),
      owner
    )
  ).toBe(false);
  expect(
    incomingGiftWrapTags(
      [
        ['p', owner],
        ['vendor', 'x'.repeat(4096)]
      ],
      owner
    )
  ).toBe(true);
  expect(
    incomingGiftWrapTags(
      [
        ['p', owner],
        ['vendor', 'x'.repeat(4097)]
      ],
      owner
    )
  ).toBe(false);
});
it('raw objects cannot invoke conversion and layer/key/UTF8 limits remain defensive', () => {
  let calls = 0;
  const input = {
    toJSON() {
      calls++;
      return rumor([['p', owner]]);
    }
  };
  expect(readIncomingPairRumor(input)).toBeUndefined();
  expect(parseIncomingLayer(input, 8192)).toBeUndefined();
  expect(calls).toBe(0);
  expect(parseIncomingLayer('x'.repeat(8193), 8192)).toBeUndefined();
  expect(parseIncomingLayer('{"a":1,"a":2}', 8192)).toBeUndefined();
  expect(parseIncomingLayer('{"a":"\\ud800"}', 8192)).toBeUndefined();
});
