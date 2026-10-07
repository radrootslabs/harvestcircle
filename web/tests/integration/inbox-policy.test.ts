import test from 'node:test';
import assert from 'node:assert/strict';
import { PrivateKeySigner } from 'applesauce-signers';
import { createInboxPolicy, fixtureRelay } from './harness/inbox-policy.ts';

const now = 1800000000;
async function inputs() {
  const owner = new PrivateKeySigner(),
    outer = new PrivateKeySigner(),
    peer = new PrivateKeySigner();
  const author = await owner.getPublicKey(),
    recipient = await peer.getPublicKey();
  const auth = await owner.signEvent({
    kind: 22242,
    created_at: now,
    tags: [
      ['relay', fixtureRelay],
      ['challenge', 'challenge']
    ],
    content: ''
  });
  const wrap = await outer.signEvent({
    kind: 1059,
    created_at: now - 48 * 3600 - 300,
    tags: [['p', recipient]],
    content: 'opaque policy fixture ciphertext'
  });
  return {
    owner,
    outer,
    peer,
    author,
    recipient,
    auth,
    wrap,
    close() {
      owner.key.fill(0);
      outer.key.fill(0);
      peer.key.fill(0);
    }
  };
}
void test('expected policy independently verifies AUTH and disposable outer author', async () => {
  const x = await inputs();
  try {
    const policy = createInboxPolicy({ now, retentionSeconds: 30 * 86400 }),
      connection = policy.connect('challenge');
    assert.deepEqual(connection.frame(JSON.stringify(['AUTH', x.auth])), [
      ['OK', x.auth.id, true, '']
    ]);
    assert.notEqual(x.wrap.pubkey, x.author);
    assert.deepEqual(connection.frame(JSON.stringify(['EVENT', x.wrap])), [
      ['OK', x.wrap.id, true, '']
    ]);
    assert.equal(policy.size(), 1);
    assert.deepEqual(
      connection.frame(
        JSON.stringify([
          'REQ',
          'wrong',
          { kinds: [1059], '#p': [x.recipient], limit: 200 }
        ])
      ),
      [['CLOSED', 'wrong', 'restricted: recipient-only']]
    );
    connection.close();
    assert.deepEqual(connection.frame(JSON.stringify(['EVENT', x.wrap])), [
      ['NOTICE', 'closed: fixture connection']
    ]);
  } finally {
    x.close();
  }
});
void test('invalid signature, wrong AUTH binding and plaintext kinds fail closed without admission', async () => {
  const x = await inputs();
  try {
    const policy = createInboxPolicy({ now, retentionSeconds: 30 * 86400 }),
      c = policy.connect('challenge');
    assert.equal(
      c.frame(
        JSON.stringify(['AUTH', { ...x.auth, sig: '0'.repeat(128) }])
      )[0][2],
      false
    );
    const wrong = await x.owner.signEvent({
      ...x.auth,
      tags: [
        ['relay', 'wss://wrong.example.org/'],
        ['challenge', 'challenge']
      ]
    });
    assert.equal(c.frame(JSON.stringify(['AUTH', wrong]))[0][2], false);
    assert.equal(c.frame(JSON.stringify(['EVENT', x.wrap]))[0][2], false);
    const fresh = policy.connect('challenge');
    fresh.frame(JSON.stringify(['AUTH', x.auth]));
    for (const kind of [13, 14]) {
      const plaintext = await x.owner.signEvent({
        kind,
        created_at: now,
        tags: [],
        content: 'unsupported plaintext fixture'
      });
      assert.equal(
        fresh.frame(JSON.stringify(['EVENT', plaintext]))[0][2],
        false
      );
    }
    assert.equal(
      fresh.frame(
        JSON.stringify(['EVENT', { ...x.wrap, sig: '0'.repeat(128) }])
      )[0][2],
      false
    );
    assert.deepEqual(fresh.frame('{'), [['NOTICE', 'invalid: fixture frame']]);
    assert.equal(policy.size(), 0);
  } finally {
    x.close();
  }
});
void test('modeled restrictive author policy exposes disposable-wrap refusal', async () => {
  const x = await inputs();
  try {
    const p = createInboxPolicy({
        now,
        retentionSeconds: 30 * 86400,
        requireOuterAuthor: true
      }),
      c = p.connect('challenge');
    c.frame(JSON.stringify(['AUTH', x.auth]));
    assert.equal(c.frame(JSON.stringify(['EVENT', x.wrap]))[0][2], false);
    assert.equal(p.size(), 0);
  } finally {
    x.close();
  }
});
