import assert from 'node:assert/strict';
import test from 'node:test';
import { finalizeEvent } from 'applesauce-core/helpers';
import {
  readInboxPreference,
  inboxPreferenceSnapshot,
  inboxPreferenceWire
} from '../../src/lib/nostr/inbox-preferences.ts';
import { inboxProfileSnapshot } from '../../src/lib/messaging/inbox-profile.ts';
const origin = 'wss://one.example.org';
// Test-only ephemeral SDK signing; no literal secret, persistence, logs or product import.
function fixture(
  tags: string[][] = [['relay', origin]],
  kind = 10050,
  created_at = 100,
  content = ''
) {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    const event = finalizeEvent({ kind, created_at, tags, content }, key);
    return { event, wire: JSON.stringify(event) };
  } finally {
    key.fill(0);
  }
}
void test('10050 relay is independently signed preference, distinct from10002/r and Food', () => {
  const f = fixture();
  const result = readInboxPreference(f.wire, f.event.pubkey);
  assert.equal(result.status, 'supported');
  if (result.status !== 'supported') throw new Error('not admitted');
  assert.deepEqual(inboxPreferenceSnapshot(result.value), {
    author: f.event.pubkey,
    id: f.event.id,
    createdAt: 100,
    status: 'supported',
    relays: [origin]
  });
  assert.deepEqual(inboxProfileSnapshot(result.value), {
    author: f.event.pubkey,
    id: f.event.id,
    createdAt: 100,
    status: 'compatible',
    advertised: [origin]
  });
  for (const kind of [10002, 30402, 0, 1059]) {
    const other = fixture([['r', origin]], kind);
    assert.equal(
      readInboxPreference(other.wire, other.event.pubkey).status,
      'unrelated'
    );
  }
});
void test('exact raw original includes unknown entries, content and unauthenticated extra fields without truncation', () => {
  const f = fixture(
    [
      ['relay', origin],
      ['unknown', 'extra', 'values'],
      ['r', 'wss://other.example.org'],
      ['relay', origin]
    ],
    10050,
    100,
    'preserved content'
  );
  const wire = JSON.stringify(
    {
      ...f.event,
      extension: { kind: 1.0, created_at: 2, items: ['preserve'] },
      note: 'untrusted raw extension'
    },
    null,
    2
  );
  const result = readInboxPreference(wire, f.event.pubkey);
  assert.equal(result.status, 'supported');
  if (result.status !== 'supported') throw new Error('not admitted');
  assert.equal(inboxPreferenceWire(result.value), wire);
  const preserved: unknown = JSON.parse(inboxPreferenceWire(result.value)!);
  assert.ok(preserved && typeof preserved === 'object' && 'tags' in preserved);
  assert.deepEqual(preserved.tags, f.event.tags);
  assert.deepEqual(inboxPreferenceSnapshot(result.value)?.relays, [origin]);
});
void test('more than three advertised relays remain intact; no routing or allowlist selection is inferred', () => {
  const relays = ['one', 'two', 'three', 'four'].map(
    (n) => `wss://${n}.example.org`
  );
  const f = fixture(relays.map((r) => ['relay', r]));
  const result = readInboxPreference(f.wire, f.event.pubkey);
  assert.equal(result.status, 'supported');
  if (result.status !== 'supported') throw new Error('not admitted');
  assert.deepEqual(inboxPreferenceSnapshot(result.value)?.relays, relays);
  assert.equal(inboxPreferenceWire(result.value), f.wire);
});
for (const tags of [
  [],
  [['r', origin]],
  [['relay']],
  [['relay', 'ws://one.example.org']],
  [['relay', origin, 'read']],
  [['relay', 'wss://127.0.0.1']],
  [['relay', origin + '/path']],
  [
    ['relay', origin],
    ['relay', 'not a relay']
  ]
]) {
  void test(
    'signed malformed/unsupported preference is a retained head, never absence: ' +
      JSON.stringify(tags),
    () => {
      const f = fixture(tags, 10050, 200);
      const result = readInboxPreference(f.wire, f.event.pubkey);
      assert.equal(result.status, 'unsupported');
      if (result.status !== 'unsupported') throw new Error('not unsupported');
      const saved = inboxPreferenceSnapshot(result.value)!;
      assert.equal(saved.id, f.event.id);
      assert.equal(saved.createdAt, 200);
      assert.equal(saved.author, f.event.pubkey);
      assert.deepEqual(saved.relays, []);
      assert.equal(inboxPreferenceWire(result.value), f.wire);
      assert.equal(inboxProfileSnapshot(result.value)?.status, 'unsupported');
    }
  );
}
void test('invalid intended keys and mismatched authenticated author reject without preference authority', () => {
  const f = fixture();
  for (const owner of [
    '',
    'f'.repeat(64),
    f.event.pubkey.toUpperCase(),
    1,
    null
  ])
    assert.equal(readInboxPreference(f.wire, owner).status, 'rejected');
  const another = fixture();
  assert.notEqual(another.event.pubkey, f.event.pubkey);
  assert.equal(
    readInboxPreference(f.wire, another.event.pubkey).status,
    'rejected'
  );
});
void test('invalid id, signature, key and altered signed fields cannot supply a newer supported or unsupported head', () => {
  const f = fixture();
  for (const altered of [
    { ...f.event, id: '0'.repeat(64) },
    { ...f.event, sig: '0'.repeat(128) },
    { ...f.event, pubkey: 'f'.repeat(64) },
    { ...f.event, created_at: 201 },
    { ...f.event, tags: [['relay']] }
  ])
    assert.equal(
      readInboxPreference(JSON.stringify(altered), f.event.pubkey).status,
      'rejected'
    );
});
void test('bounded raw envelope, generic tags and unknown field budgets reject before preference admission', () => {
  const f = fixture();
  assert.equal(
    readInboxPreference(f.wire + ' '.repeat(262145), f.event.pubkey).status,
    'rejected'
  );
  const tooMany = fixture(Array.from({ length: 1025 }, () => ['unknown', 'x']));
  assert.equal(
    readInboxPreference(tooMany.wire, tooMany.event.pubkey).status,
    'rejected'
  );
  const extra: Record<string, unknown> = { ...f.event };
  for (let i = 0; i < 65; i++) extra['extra' + i] = i;
  assert.equal(
    readInboxPreference(JSON.stringify(extra), f.event.pubkey).status,
    'rejected'
  );
  assert.equal(
    readInboxPreference(
      JSON.stringify({ ...f.event, extra: 'x'.repeat(65537) }),
      f.event.pubkey
    ).status,
    'rejected'
  );
  const oversizedTag = fixture([
    ['relay', origin],
    ['unknown', 'x'.repeat(4097)]
  ]);
  assert.equal(
    readInboxPreference(oversizedTag.wire, oversizedTag.event.pubkey).status,
    'rejected'
  );
});
void test('numeric spelling, object/getter/cache authority and invalid JSON cannot bypass fresh SDK verification', () => {
  const f = fixture();
  assert.equal(
    readInboxPreference(
      f.wire.replace('"kind":10050', '"kind":10050.0'),
      f.event.pubkey
    ).status,
    'rejected'
  );
  let reads = 0;
  const hostile = {
    get content() {
      reads++;
      throw new Error('must not run');
    }
  };
  assert.equal(readInboxPreference(hostile, f.event.pubkey).status, 'rejected');
  assert.equal(reads, 0);
  assert.equal(readInboxPreference('{', f.event.pubkey).status, 'rejected');
  assert.equal(readInboxPreference(f.event, f.event.pubkey).status, 'rejected');
});
