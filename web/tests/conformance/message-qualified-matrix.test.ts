import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import {
  qualifiedMessages,
  qualifiedMessageReplay
} from './message-qualified-matrix.ts';
import {
  messageFromWireParts,
  messageToWireParts,
  type MessageV1
} from '../../src/lib/contracts/message-v1/index.ts';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import type { NostrEvent } from 'applesauce-core/helpers';
const digest = (bytes: string | Uint8Array) =>
  createHash('sha256').update(bytes).digest('hex');
const receipt = JSON.parse(
  readFileSync(
    new URL('./message-qualified-writer-rust.v1.json', import.meta.url),
    'utf8'
  )
) as {
  schema_version: number;
  revision: string;
  recipe_sha256: string;
  inputs: Record<string, string>;
  consumed: { id: string; message: MessageV1 }[];
  qualification: string;
};
void test('separate sixteen Message recipes replay actual pinned Rust consumption without changing original shared fixtures', () => {
  assert.equal(
    receipt.recipe_sha256,
    digest(JSON.stringify(qualifiedMessageReplay))
  );
  assert.equal(receipt.schema_version, 1);
  assert.equal(receipt.revision, '189c49b74b4bafc142b00b76b296477931139e72');
  assert.equal(qualifiedMessageReplay.length, 16);
  assert.deepEqual(
    receipt.consumed,
    qualifiedMessageReplay.map((row) => ({ id: row.id, message: row.expected }))
  );
  const inputs = {
    'message-qualified-matrix.ts': new URL(
      './message-qualified-matrix.ts',
      import.meta.url
    ),
    'source_profile.v1.json': new URL(
      '../../../contracts/interop/message/source_profile.v1.json',
      import.meta.url
    ),
    'corpus.v1.json': new URL(
      '../../../contracts/interop/message/corpus.v1.json',
      import.meta.url
    ),
    'oracle/src/main.rs': new URL(
      '../../../contracts/interop/message/oracle/src/main.rs',
      import.meta.url
    )
  };
  assert.deepEqual(
    Object.keys(receipt.inputs).sort(),
    Object.keys(inputs).sort()
  );
  for (const [name, url] of Object.entries(inputs))
    assert.equal(receipt.inputs[name], digest(readFileSync(url)), name);
  assert.match(receipt.qualification, /unsigned kind 14/);
  assert.match(receipt.qualification, /not independent-client receive\/reply/);
});
void test('pure Message parity preserves exact p/e/subject, bounded raw text and ordinary citations without orders', () => {
  for (const [index, row] of qualifiedMessageReplay.entries()) {
    assert.deepEqual(
      messageFromWireParts(JSON.stringify(row.wire_parts)),
      row.expected,
      row.id
    );
    assert.equal(row.wire_parts.kind, 14);
    assert.deepEqual(
      row.wire_parts.tags.map((tag) => tag[0]),
      [
        'p',
        ...(row.expected.reply_to ? ['e'] : []),
        ...(row.expected.subject !== null ? ['subject'] : [])
      ]
    );
    assert.equal(Object.hasOwn(row.wire_parts, 'sig'), false);
    assert.equal(Object.hasOwn(row.wire_parts, 'id'), false);
    assert.equal(Object.hasOwn(row.wire_parts, 'pubkey'), false);
    assert.deepEqual(row.expected, qualifiedMessages[index]);
    assert.equal(
      messageToWireParts(
        JSON.stringify({ ...row.expected, order: 'unauthorized' })
      ),
      undefined
    );
  }
  assert.equal(
    new TextEncoder().encode(qualifiedMessages[9].content).length,
    4096
  );
  assert.notEqual(qualifiedMessages[6].content, qualifiedMessages[7].content);
  assert.equal(
    qualifiedMessages[6].content.normalize('NFC'),
    qualifiedMessages[7].content.normalize('NFC')
  );
});
interface PublishedWires {
  schema_version: number;
  source: {
    url: string;
    revision: string;
    sha256: string;
    bytes: number;
    scope: string;
  };
  cases: {
    role: 'peer' | 'self';
    fence_start_line: number;
    event: NostrEvent;
  }[];
}
const published = JSON.parse(
  readFileSync(
    new URL(
      '../fixtures/independent-nip17/public-outer-wires.v1.json',
      import.meta.url
    ),
    'utf8'
  )
) as PublishedWires;
void test('frozen official NIP17 peer and self outer vectors verify fresh public signatures and exact destinations without imported keys or plaintext claims', () => {
  assert.equal(published.schema_version, 1);
  assert.equal(
    published.source.revision,
    '0046368a747c5c25ae2bec28bae0e537744c8f10'
  );
  assert.equal(
    published.source.sha256,
    '7aa9c3a9c0458a9552c4be2f3bf17f7ec60a4afe88bce7977f03500a7873a64c'
  );
  assert.equal(published.source.bytes, 12439);
  assert.equal(
    published.source.url,
    'https://raw.githubusercontent.com/nostr-protocol/nips/0046368a747c5c25ae2bec28bae0e537744c8f10/17.md'
  );
  assert.match(published.source.scope, /outer ciphertext only/);
  assert.deepEqual(
    published.cases.map((row) => [row.role, row.fence_start_line]),
    [
      ['peer', 190],
      ['self', 204]
    ]
  );
  assert.deepEqual(
    published.cases.map((row) => row.event.id),
    [
      '2886780f7349afc1344047524540ee716f7bdc1b64191699855662330bf235d8',
      '162b0611a1911cfcb30f8a5502792b346e535a45658b3a31ae5c178465509721'
    ]
  );
  const destinations = [
    '918e2da906df4ccd12c8ac672d8335add131a4cf9d27ce42b3bb3625755f0788',
    '44900586091b284416a0c001f677f9c49f7639a55c3f1e2ec130a8e1a7998e1b'
  ];
  for (const [index, row] of published.cases.entries()) {
    assert.deepEqual(
      Object.keys(row.event).sort(),
      ['id', 'pubkey', 'created_at', 'kind', 'tags', 'content', 'sig'].sort()
    );
    assert.equal(row.event.kind, 1059);
    assert.deepEqual(row.event.tags, [['p', destinations[index]]]);
    assert.equal(verifyEnvelope(JSON.stringify(row.event)).ok, true);
    assert.equal(messageFromWireParts(JSON.stringify(row.event)), undefined);
    assert.equal(JSON.stringify(row.event).includes('nsec'), false);
  }
  assert.notEqual(
    published.cases[0].event.pubkey,
    published.cases[1].event.pubkey
  );
  assert.notEqual(published.cases[0].event.id, published.cases[1].event.id);
});
void test('altered official outer target, ciphertext and signature each fail fresh verification; public examples grant no decrypted rumor custody', () => {
  for (const row of published.cases) {
    for (const changed of [
      { ...row.event, tags: [['p', '0'.repeat(64)]] },
      { ...row.event, content: row.event.content + 'A' },
      { ...row.event, sig: '0'.repeat(128) }
    ])
      assert.equal(verifyEnvelope(JSON.stringify(changed)).ok, false);
  }
});
