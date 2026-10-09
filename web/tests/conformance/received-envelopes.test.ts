import test from 'node:test';
import assert from 'node:assert/strict';
import {
  generateSecretKey,
  getPublicKey,
  getEventHash,
  finalizeEvent,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { verifyReceivedLayerData } from '../../src/lib/nostr/unwrap-admission.ts';
function fixture() {
  const keys = [generateSecretKey(), generateSecretKey(), generateSecretKey()],
    sender = getPublicKey(keys[0]),
    owner = getPublicKey(keys[1]);
  const template: UnsignedEvent = {
    pubkey: sender,
    kind: 14,
    created_at: 1700000000,
    tags: [['p', owner]],
    content: 'Bounded inbound enquiry'
  };
  const rumor = { id: getEventHash(template), ...template };
  const conversation = nip44.v2.utils.getConversationKey(keys[0], owner);
  let content: string;
  try {
    content = nip44.v2.encrypt(JSON.stringify(rumor), conversation);
  } finally {
    conversation.fill(0);
  }
  const seal = finalizeEvent(
    { kind: 13, created_at: 1699999900, tags: [], content },
    keys[0]
  );
  const wrapConversation = nip44.v2.utils.getConversationKey(keys[2], owner);
  let outerContent: string;
  try {
    outerContent = nip44.v2.encrypt(JSON.stringify(seal), wrapConversation);
  } finally {
    wrapConversation.fill(0);
  }
  const outer = finalizeEvent(
    {
      kind: 1059,
      created_at: 1699999800,
      tags: [['p', owner]],
      content: outerContent
    },
    keys[2]
  );
  const check = (r: unknown = rumor, s: unknown = seal, o: unknown = outer) =>
    verifyReceivedLayerData(
      JSON.stringify(o),
      JSON.stringify(s),
      JSON.stringify(r),
      owner
    );
  return {
    keys,
    sender,
    owner,
    rumor,
    seal,
    outer,
    check,
    close() {
      keys.forEach((k) => k.fill(0));
    }
  };
}
async function vector(
  name: string,
  run: (f: ReturnType<typeof fixture>) => void
) {
  await test(name, () => {
    const f = fixture();
    try {
      run(f);
    } finally {
      f.close();
    }
  });
}
await vector(
  'actual signed layers and freshly hashed unsigned pair rumor pass detached inspection',
  (f) => assert.equal(f.check(), true)
);
await vector(
  'forged inner author fails even after recomputing rumor hash',
  (f) => {
    const r = { ...f.rumor, pubkey: f.owner };
    r.id = getEventHash(r);
    assert.equal(f.check(r), false);
  }
);
await vector('forged seal signature fails before nested rumor admission', (f) =>
  assert.equal(f.check(f.rumor, { ...f.seal, sig: '0'.repeat(128) }), false)
);
await vector(
  'a valid differently signed seal cannot authenticate original rumor author',
  (f) =>
    assert.equal(
      f.check(f.rumor, finalizeEvent({ ...f.seal }, f.keys[1])),
      false
    )
);
await vector('supplied rumor id is never a cache identity substitute', (f) =>
  assert.equal(f.check({ ...f.rumor, id: '0'.repeat(64) }), false)
);
await vector(
  'signed rumor is unsupported rather than silently accepted as unsigned14',
  (f) => assert.equal(f.check(finalizeEvent({ ...f.rumor }, f.keys[0])), false)
);
await vector('wrong kind and file metadata are isolated', (f) => {
  const r = {
    ...f.rumor,
    kind: 15,
    tags: [...f.rumor.tags, ['file', 'https://example.org/a']]
  };
  r.id = getEventHash(r);
  assert.equal(f.check(r), false);
});
await vector('group fanout cannot become an admitted pair rumor', (f) => {
  const r = {
    ...f.rumor,
    tags: [...f.rumor.tags, ['p', getPublicKey(f.keys[2])]]
  };
  r.id = getEventHash(r);
  assert.equal(f.check(r), false);
});
await vector('self-only participants are unsupported', (f) => {
  const r = { ...f.rumor, tags: [['p', f.sender]] };
  r.id = getEventHash(r);
  assert.equal(f.check(r), false);
});
await vector('nonempty seal tags are rejected despite a valid signature', (f) =>
  assert.equal(
    f.check(
      f.rumor,
      finalizeEvent({ ...f.seal, tags: [['client', 'unsupported']] }, f.keys[0])
    ),
    false
  )
);
await vector(
  'wrong outer destination cannot cross owner-scoped admission',
  (f) =>
    assert.equal(
      f.check(
        f.rumor,
        f.seal,
        finalizeEvent({ ...f.outer, tags: [['p', f.sender]] }, f.keys[2])
      ),
      false
    )
);
await vector('body and layer bounds are checked without truncation', (f) => {
  const r = { ...f.rumor, content: 'x'.repeat(4097) };
  r.id = getEventHash(r);
  assert.equal(f.check(r), false);
  assert.equal(
    verifyReceivedLayerData(
      JSON.stringify(f.outer),
      JSON.stringify(f.seal),
      'x'.repeat(8193),
      f.owner
    ),
    false
  );
});
await vector(
  'caller objects and imported or persisted verification markers cannot bypass fresh parsing',
  (f) => {
    let calls = 0;
    const value = {
      ...f.rumor,
      toJSON() {
        calls++;
        return f.rumor;
      }
    };
    assert.equal(
      verifyReceivedLayerData(
        JSON.stringify(f.outer),
        JSON.stringify(f.seal),
        value,
        f.owner
      ),
      false
    );
    assert.equal(calls, 0);
    assert.equal(f.check({ ...f.rumor, verified: true }), false);
  }
);
await vector(
  'harmless JSON whitespace works while duplicate fields and noncanonical numeric lexemes fail',
  (f) => {
    const o = JSON.stringify(f.outer),
      s = JSON.stringify(f.seal),
      r = JSON.stringify(f.rumor);
    assert.equal(
      verifyReceivedLayerData(
        ' ' + o + '\n',
        '\n' + s,
        JSON.stringify(f.rumor, null, 2),
        f.owner
      ),
      true
    );
    assert.equal(
      verifyReceivedLayerData(
        o,
        s,
        r.replace('"kind":14', '"kind":15,"kind":14'),
        f.owner
      ),
      false
    );
    assert.equal(
      verifyReceivedLayerData(o, s, r.replace('1700000000', '17e8'), f.owner),
      false
    );
    assert.deepEqual(
      [
        verifyReceivedLayerData(o, s, '{}', f.owner),
        verifyReceivedLayerData(o, s, r, f.owner)
      ],
      [false, true]
    );
  }
);
