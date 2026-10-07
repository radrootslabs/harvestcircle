import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import test from 'node:test';
import { getEventHash } from 'applesauce-core/helpers';
import { buildCanonicalPairRumor } from '../../src/lib/nostr/rumor-template.ts';
import { messageToWireParts } from '../../src/lib/contracts/message-v1/index.ts';
import { messageCorpus } from './message-cases.ts';
const author =
    '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
  peer = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5',
  time = 1700000100;
const message = (content = 'A plain enquiry') => ({
  recipients: [{ public_key: peer, relay_url: null }],
  content,
  reply_to: null,
  subject: null
});
void test('fresh unsigned rumor uses the qualified hash and canonical NIP01 serialization', () => {
  const value = buildCanonicalPairRumor(
    JSON.stringify(message()),
    author,
    time
  )!;
  assert.equal(value.id, getEventHash(value));
  assert.equal(
    value.id,
    createHash('sha256')
      .update(
        JSON.stringify([0, author, time, 14, [['p', peer]], 'A plain enquiry'])
      )
      .digest('hex')
  );
  assert.equal(value.pubkey, author);
  assert.equal(value.created_at, time);
  assert.equal(value.kind, 14);
  assert.equal('sig' in value, false);
  assert.deepEqual(Object.keys(value).sort(), [
    'content',
    'created_at',
    'id',
    'kind',
    'pubkey',
    'tags'
  ]);
});
void test('body Unicode and generic Message reply/subject semantics survive canonical hashing', () => {
  const input = {
    ...message('Carrots 🥕\nnostr:untrusted-text'),
    reply_to: { id: 'f'.repeat(64), relays: null },
    subject: 'Food question'
  };
  const value = buildCanonicalPairRumor(JSON.stringify(input), author, time)!;
  assert.equal(value.content, input.content);
  assert.deepEqual(value.tags, [
    ['p', peer],
    ['e', input.reply_to.id],
    ['subject', input.subject]
  ]);
  assert.equal(value.id, getEventHash(value));
});
void test('same captured fields have the same ID and changed observed timestamp/body changes it', () => {
  const raw = JSON.stringify(message()),
    first = buildCanonicalPairRumor(raw, author, time)!;
  assert.equal(buildCanonicalPairRumor(raw, author, time)!.id, first.id);
  assert.notEqual(buildCanonicalPairRumor(raw, author, time + 1)!.id, first.id);
  assert.notEqual(
    buildCanonicalPairRumor(
      JSON.stringify(message('A different intention')),
      author,
      time
    )!.id,
    first.id
  );
  // Intentional same-second duplicate admission/clock progression belongs to074.
});
void test('groups, duplicate recipients and self messages are unsupported', () => {
  for (const recipients of [
    [],
    [{ public_key: author }],
    [{ public_key: peer }, { public_key: author }],
    [{ public_key: peer }, { public_key: peer }]
  ])
    assert.equal(
      buildCanonicalPairRumor(
        JSON.stringify({ ...message(), recipients }),
        author,
        time
      ),
      undefined
    );
});
void test('keys and observed numeric timestamps cannot be coerced or rounded', () => {
  for (const owner of [
    author.toUpperCase(),
    '0'.repeat(64),
    '',
    null,
    { toString: () => author }
  ])
    assert.equal(
      buildCanonicalPairRumor(JSON.stringify(message()), owner, time),
      undefined
    );
  for (const timestamp of [
    -1,
    -0,
    1.5,
    Infinity,
    NaN,
    Number.MAX_SAFE_INTEGER + 1,
    String(time),
    null
  ])
    assert.equal(
      buildCanonicalPairRumor(JSON.stringify(message()), author, timestamp),
      undefined
    );
  assert.equal(
    buildCanonicalPairRumor(JSON.stringify(message()), author, 0)!.created_at,
    0
  );
});
void test('body and complete rumor JSON limits are separately enforced without truncation', () => {
  assert.equal(
    buildCanonicalPairRumor(
      JSON.stringify(message('a'.repeat(4096))),
      author,
      time
    )!.content.length,
    4096
  );
  assert.equal(
    buildCanonicalPairRumor(
      JSON.stringify(message('é'.repeat(2048))),
      author,
      time
    )!.content.length,
    2048
  );
  for (const content of ['a'.repeat(4097), 'é'.repeat(2049), '\ud800', ' \n\t'])
    assert.equal(
      buildCanonicalPairRumor(JSON.stringify(message(content)), author, time),
      undefined
    );
  const escaped = JSON.stringify(message('"'.repeat(4000)));
  assert.ok(messageToWireParts(escaped), 'detached parts still fit');
  assert.equal(
    buildCanonicalPairRumor(escaped, author, time),
    undefined,
    'full rumor overhead exceeds8KiB'
  );
});
for (const vector of messageCorpus().vectors.filter((v) => v.mode === 'write'))
  void test(
    'pinned Message writer projection into pair rumor: ' + vector.id,
    () => {
      const raw = JSON.stringify(vector.input),
        parts = messageToWireParts(raw);
      const recipients = parts?.tags.filter((row) => row[0] === 'p');
      const value = buildCanonicalPairRumor(raw, author, time);
      if (!parts || recipients?.length !== 1 || recipients[0]?.[1] === author)
        assert.equal(value, undefined);
      else {
        assert.ok(value);
        assert.deepEqual(value.tags, parts.tags);
        assert.equal(value.content, parts.content);
        assert.equal(value.id, getEventHash(value));
        assert.equal(value.pubkey, author);
      }
    }
  );
