import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  generateSecretKey,
  getPublicKey,
  getEventHash,
  finalizeEvent,
  verifyEvent,
  verifiedSymbol,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { verifyReceivedLayerData } from '../../src/lib/nostr/unwrap-admission.ts';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import {
  getPublicStore,
  insertPublicEnvelope,
  publicStoreEnvelope,
  publicStoreRetention,
  closePublicStore
} from '../../src/lib/nostr/public-store.ts';
function fixture() {
  const keys = [generateSecretKey(), generateSecretKey(), generateSecretKey()];
  const sender = getPublicKey(keys[0]),
    owner = getPublicKey(keys[1]),
    other = getPublicKey(keys[2]);
  const template: UnsignedEvent = {
    pubkey: sender,
    kind: 14,
    created_at: 1700000000,
    tags: [['p', owner]],
    content: 'Private hostile-input qualification canary'
  };
  const rumor = { id: getEventHash(template), ...template };
  function encrypt(key: Uint8Array, target: string, plain: string) {
    const conversation = nip44.v2.utils.getConversationKey(key, target);
    try {
      return nip44.v2.encrypt(plain, conversation);
    } finally {
      conversation.fill(0);
    }
  }
  const seal = finalizeEvent(
    {
      kind: 13,
      created_at: 1699999900,
      tags: [],
      content: encrypt(keys[0], owner, JSON.stringify(rumor))
    },
    keys[0]
  );
  const outer = finalizeEvent(
    {
      kind: 1059,
      created_at: 1699999800,
      tags: [['p', owner]],
      content: encrypt(keys[2], owner, JSON.stringify(seal))
    },
    keys[2]
  );
  const wire = [outer, seal, rumor].map((value) => JSON.stringify(value));
  const check = (values: readonly unknown[], expected: unknown = owner) =>
    verifyReceivedLayerData(values[0], values[1], values[2], expected);
  return {
    keys,
    sender,
    owner,
    other,
    rumor,
    seal,
    outer,
    wire,
    check,
    close: () => keys.forEach((key) => key.fill(0))
  };
}
await test('finite adversarial nested matrix refuses altered IDs, signatures, author, target, schema and numeric spelling while preserving the next valid candidate', () => {
  const f = fixture();
  try {
    assert.equal(f.check(f.wire), true);
    const candidates: { label: string; layer: number; value: string }[] = [];
    for (const [index, original] of [f.outer, f.seal, f.rumor].entries()) {
      const raw = JSON.stringify(original);
      for (const [field, value] of [
        ['id', '0'.repeat(64)],
        ['pubkey', f.owner],
        ['kind', 15],
        ['content', 'altered plaintext or ciphertext'],
        ['created_at', 1700000001],
        ['verified', true]
      ] as const)
        candidates.push({
          label: `${index}:${field}`,
          layer: index,
          value: JSON.stringify({ ...original, [field]: value })
        });
      candidates.push({
        label: `${index}:duplicate`,
        layer: index,
        value: raw.replace('"kind":', '"kind":1,"kind":')
      });
      const escapedDuplicate = raw.replace('"kind":', '"k\\u0069nd":1,"kind":');
      assert.notEqual(escapedDuplicate, raw);
      assert.deepEqual(JSON.parse(escapedDuplicate), JSON.parse(raw));
      candidates.push({
        label: `${index}:escaped-duplicate`,
        layer: index,
        value: escapedDuplicate
      });
      candidates.push({
        label: `${index}:numeric-spelling`,
        layer: index,
        value: raw.replace(/"created_at":([0-9]+)/u, '"created_at":$1.0')
      });
      candidates.push({
        label: `${index}:invalid-json`,
        layer: index,
        value: raw.slice(0, -1)
      });
    }
    for (const [index, original] of [f.outer, f.seal].entries())
      candidates.push({
        label: `${index}:signature`,
        layer: index,
        value: JSON.stringify({ ...original, sig: '0'.repeat(128) })
      });
    candidates.push({
      label: 'outer:valid-wrong-destination',
      layer: 0,
      value: JSON.stringify(
        finalizeEvent({ ...f.outer, tags: [['p', f.sender]] }, f.keys[2])
      )
    });
    candidates.push({
      label: 'seal:valid-other-signer',
      layer: 1,
      value: JSON.stringify(finalizeEvent({ ...f.seal }, f.keys[1]))
    });
    candidates.push({
      label: 'seal:valid-metadata',
      layer: 1,
      value: JSON.stringify(
        finalizeEvent({ ...f.seal, tags: [['subject', 'private']] }, f.keys[0])
      )
    });
    for (const [label, tags, kind] of [
      [
        'group',
        [
          ['p', f.owner],
          ['p', f.other]
        ],
        14
      ],
      [
        'duplicate-peer',
        [
          ['p', f.owner],
          ['p', f.owner]
        ],
        14
      ],
      ['self-request', [['p', f.sender]], 14],
      [
        'file',
        [
          ['p', f.owner],
          ['file', 'https://example.org/a']
        ],
        15
      ],
      [
        'unsupported-expiration',
        [
          ['p', f.owner],
          ['expiration', '1700000001']
        ],
        14
      ]
    ] as const) {
      const changed: UnsignedEvent = {
        ...f.rumor,
        kind,
        tags: tags.map((row) => [...row])
      };
      candidates.push({
        label,
        layer: 2,
        value: JSON.stringify({ ...changed, id: getEventHash(changed) })
      });
    }
    assert.equal(candidates.length, 40);
    // D09 accepts this former negative as inert incoming metadata. Detached
    // parsing proves no ciphertext correspondence, order flow or effect authority.
    const inert: UnsignedEvent = {
      ...f.rumor,
      tags: [
        ['p', f.owner],
        ['order', 'unsupported']
      ]
    };
    assert.equal(
      f.check([
        f.wire[0],
        f.wire[1],
        JSON.stringify({ ...inert, id: getEventHash(inert) })
      ]),
      true
    );
    for (const row of candidates) {
      const values = [...f.wire];
      values[row.layer] = row.value;
      assert.equal(f.check(values), false, row.label);
      assert.equal(f.check(f.wire), true, `valid after ${row.label}`);
    }
    assert.equal(f.check(f.wire, f.other), false);
    assert.equal(f.check(f.wire, { toString: () => f.owner }), false);
  } finally {
    f.close();
  }
});
await test('actual SDK cached verified symbols and caller objects cannot replace fresh private layer parsing', () => {
  const f = fixture();
  try {
    for (const [index, original] of [f.outer, f.seal].entries()) {
      const cached = {
        ...original,
        tags: original.tags.map((tag) => [...tag])
      };
      assert.equal(verifyEvent(cached), true);
      assert.equal(
        (cached as unknown as Record<symbol, unknown>)[verifiedSymbol],
        true
      );
      cached.content += 'A';
      assert.equal(verifyEvent(cached), true);
      const values: unknown[] = [...f.wire];
      values[index] = JSON.stringify(cached);
      assert.equal(f.check(values), false);
      values[index] = cached;
      assert.equal(f.check(values), false);
    }
    let calls = 0;
    const value = {
      toJSON() {
        calls++;
        return f.rumor;
      },
      toString() {
        calls++;
        return f.wire[2];
      }
    };
    assert.equal(f.check([f.wire[0], f.wire[1], value]), false);
    assert.equal(calls, 0);
    assert.equal(f.check(f.wire), true);
  } finally {
    f.close();
  }
});
await test('actual owned public SDK store refuses signed private kinds before retention and remains usable for public inventory', () => {
  const f = fixture(),
    oldWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {}
  });
  let store: ReturnType<typeof getPublicStore> = undefined;
  try {
    store = getPublicStore();
    assert.ok(store);
    const before = publicStoreRetention(store);
    const events = [f.outer, f.seal, finalizeEvent({ ...f.rumor }, f.keys[0])];
    for (const event of events) {
      const proof = verifyEnvelope(JSON.stringify(event));
      assert.equal(proof.ok, true);
      if (!proof.ok) throw new Error('invalid real signed control');
      assert.equal(insertPublicEnvelope(store, proof.value), 'not_public');
      assert.equal(publicStoreEnvelope(store, event.id), undefined);
      assert.deepEqual(publicStoreRetention(store), before);
    }
    const publicEvent = finalizeEvent(
      {
        kind: 10050,
        created_at: 1700000000,
        tags: [['relay', 'wss://inbox.example.org']],
        content: ''
      },
      f.keys[0]
    );
    const publicProof = verifyEnvelope(JSON.stringify(publicEvent));
    assert.equal(publicProof.ok, true);
    if (!publicProof.ok) throw new Error('invalid public control');
    assert.equal(insertPublicEnvelope(store, publicProof.value), 'accepted');
    assert.ok(publicStoreEnvelope(store, publicEvent.id));
    assert.equal(f.check(f.wire), true);
  } finally {
    if (store) closePublicStore(store);
    if (oldWindow) Object.defineProperty(globalThis, 'window', oldWindow);
    else Reflect.deleteProperty(globalThis, 'window');
    f.close();
  }
});
