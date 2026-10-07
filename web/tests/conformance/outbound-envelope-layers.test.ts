import test from 'node:test';
import assert from 'node:assert/strict';
import {
  generateSecretKey,
  getPublicKey,
  getEventHash,
  finalizeEvent,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import { verifyOutboundLayerData } from '../../src/lib/nostr/outbound-envelope-layers.ts';

// Detached real SDK signature vectors only; no genuine preparation authority.
function fixture() {
  const keys = [generateSecretKey(), generateSecretKey(), generateSecretKey()];
  const owner = getPublicKey(keys[0]),
    peer = getPublicKey(keys[1]);
  const template: UnsignedEvent = {
    pubkey: owner,
    kind: 14,
    created_at: 1700000000,
    tags: [['p', peer]],
    content: 'Original enquiry'
  };
  const rumor = { id: getEventHash(template), ...template };
  const seal = finalizeEvent(
    {
      kind: 13,
      created_at: 1699999900,
      tags: [],
      content: 'bounded ciphertext fixture'
    },
    keys[0]
  );
  const outer = finalizeEvent(
    {
      kind: 1059,
      created_at: 1699999800,
      tags: [['p', peer]],
      content: 'bounded outer fixture'
    },
    keys[2]
  );
  const original = JSON.stringify(rumor);
  const check = (
    r: unknown = rumor,
    s: unknown = seal,
    w: unknown = outer,
    role: 'peer' | 'self' = 'peer'
  ) =>
    verifyOutboundLayerData(
      original,
      JSON.stringify(r),
      JSON.stringify(s),
      JSON.stringify(w),
      owner,
      peer,
      role
    );
  return {
    keys,
    owner,
    peer,
    rumor,
    seal,
    outer,
    check,
    close: () => keys.forEach((k) => k.fill(0))
  };
}
await test('fresh SDK hashes and both signed layers preserve the exact original pair rumor', () => {
  const f = fixture();
  try {
    assert.equal(f.check(), true);
    const self = finalizeEvent(
      {
        kind: 1059,
        created_at: f.outer.created_at,
        tags: [['p', f.owner]],
        content: f.outer.content
      },
      f.keys[2]
    );
    assert.equal(f.check(f.rumor, f.seal, self, 'self'), true);
  } finally {
    f.close();
  }
});
await test('valid signatures do not admit wrong sender, seal tags or outer target', () => {
  const f = fixture();
  try {
    assert.equal(
      f.check(f.rumor, finalizeEvent({ ...f.seal }, f.keys[1])),
      false
    );
    assert.equal(
      f.check(
        f.rumor,
        finalizeEvent({ ...f.seal, tags: [['client', 'leak']] }, f.keys[0])
      ),
      false
    );
    assert.equal(
      f.check(
        f.rumor,
        f.seal,
        finalizeEvent({ ...f.outer, tags: [['p', f.owner]] }, f.keys[2])
      ),
      false
    );
    assert.equal(
      f.check(
        f.rumor,
        f.seal,
        finalizeEvent(
          {
            ...f.outer,
            tags: [
              ['p', f.peer],
              ['p', f.owner]
            ]
          },
          f.keys[2]
        )
      ),
      false
    );
  } finally {
    f.close();
  }
});
await test('rehashed substitute rumor, third-party fanout and cached wrong ids fail', () => {
  const f = fixture();
  try {
    const substitute = { ...f.rumor, content: 'different enquiry' };
    substitute.id = getEventHash(substitute);
    assert.equal(f.check(substitute), false);
    const fanout = {
      ...f.rumor,
      tags: [
        ['p', f.peer],
        ['p', f.owner]
      ]
    };
    fanout.id = getEventHash(fanout);
    assert.equal(f.check(fanout), false);
    assert.equal(f.check({ ...f.rumor, id: '0'.repeat(64) }), false);
    assert.equal(
      f.check(f.rumor, { ...f.seal, id: '0'.repeat(64), verified: true }),
      false
    );
  } finally {
    f.close();
  }
});
