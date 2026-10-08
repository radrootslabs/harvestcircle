import test from 'node:test';
import assert from 'node:assert/strict';
import {
  generateSecretKey,
  getPublicKey,
  finalizeEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { buildCanonicalPairRumor } from '../../src/lib/nostr/rumor-template.ts';
import { verifyOutboundLayerData } from '../../src/lib/nostr/outbound-envelope-layers.ts';
const bytes = (value: string) => new TextEncoder().encode(value).length;
function input(peer: string, content: string) {
  return JSON.stringify({
    recipients: [{ public_key: peer, relay_url: null }],
    content,
    reply_to: null,
    subject: null
  });
}
await test('4096 UTF8 body boundaries and serialization overhead reject unchanged originals, never truncate', () => {
  const secret = generateSecretKey(),
    peerSecret = generateSecretKey();
  try {
    const owner = getPublicKey(secret),
      peer = getPublicKey(peerSecret);
    const cases = [
      ['ASCII', 'x'.repeat(4096)],
      ['BMP', 'é'.repeat(2048)],
      ['astral', '😀'.repeat(1024)]
    ];
    for (const [label, body] of cases) {
      const raw = input(peer, body),
        rumor = buildCanonicalPairRumor(raw, owner, 1700000000);
      assert.equal(bytes(body), 4096);
      assert.ok(rumor, label);
      assert.equal(rumor.content, body);
      assert.ok(bytes(JSON.stringify(rumor)) <= 8192);
      assert.equal(
        buildCanonicalPairRumor(input(peer, body + 'x'), owner, 1700000000),
        undefined
      );
      assert.equal(raw, input(peer, body));
    }
    for (const body of [
      '\ud800',
      '\udc00',
      '"'.repeat(4096),
      '\\'.repeat(4096),
      '\n'.repeat(4096),
      '\u0001'.repeat(4096)
    ]) {
      const raw = input(peer, body);
      assert.equal(buildCanonicalPairRumor(raw, owner, 1700000000), undefined);
      assert.equal(raw, input(peer, body));
    }
  } finally {
    secret.fill(0);
    peerSecret.fill(0);
  }
});
await test('stock SDK padding steps are measured from actual standard payloads and roundtrip without extended mode', () => {
  const secret = generateSecretKey(),
    peerSecret = generateSecretKey();
  let conversation: Uint8Array | undefined;
  try {
    conversation = nip44.v2.utils.getConversationKey(
      secret,
      getPublicKey(peerSecret)
    );
    const rows = [];
    for (const [size, expected] of [
      [32, 132],
      [33, 176],
      [256, 432],
      [257, 516],
      [4096, 5552],
      [4097, 6916],
      [8192, 11012],
      [8193, 13744],
      [16384, 21936],
      [16385, 27396]
    ]) {
      const plain = 'x'.repeat(size),
        cipher = nip44.v2.encrypt(plain, conversation);
      assert.equal(bytes(cipher), expected);
      assert.equal(nip44.v2.decrypt(cipher, conversation), plain);
      rows.push({
        plaintext_utf8: size,
        ciphertext_utf8: bytes(cipher),
        mode: 'stock_standard_v2_primitive_only'
      });
    }
    console.log(
      JSON.stringify({
        fixture: 'HCP079_ACTUAL_STOCK_PADDING_NOT_PRODUCT_ADMISSION',
        rows
      })
    );
  } finally {
    conversation?.fill(0);
    secret.fill(0);
    peerSecret.fill(0);
  }
});
await test('valid signed detached serialized seal and outer exact caps accept, plus one rejects before admission', () => {
  const secret = generateSecretKey(),
    peerSecret = generateSecretKey(),
    outerSecret = generateSecretKey();
  let conversation: Uint8Array | undefined,
    outerConversation: Uint8Array | undefined;
  try {
    const owner = getPublicKey(secret),
      peer = getPublicKey(peerSecret),
      rumor = buildCanonicalPairRumor(
        input(peer, 'bounded fixture'),
        owner,
        1700000000
      );
    assert.ok(rumor);
    const original = JSON.stringify(rumor);
    conversation = nip44.v2.utils.getConversationKey(secret, peer);
    const seal = finalizeEvent(
        {
          kind: 13,
          created_at: 1699999000,
          tags: [],
          content: nip44.v2.encrypt(original, conversation)
        },
        secret
      ),
      sealWire = JSON.stringify(seal);
    outerConversation = nip44.v2.utils.getConversationKey(outerSecret, peer);
    const outer = finalizeEvent(
        {
          kind: 1059,
          created_at: 1699998000,
          tags: [['p', peer]],
          content: nip44.v2.encrypt(sealWire, outerConversation)
        },
        outerSecret
      ),
      outerWire = JSON.stringify(outer);
    assert.equal(nip44.v2.decrypt(seal.content, conversation), original);
    assert.equal(nip44.v2.decrypt(outer.content, outerConversation), sealWire);
    // Valid JSON whitespace changes serialized admission bytes, not signed data.
    // These detached fixtures do NOT claim factory custody or genuine maxima.
    const atSeal = sealWire + ' '.repeat(16384 - bytes(sealWire)),
      atOuter = outerWire + ' '.repeat(32768 - bytes(outerWire));
    assert.equal(bytes(atSeal), 16384);
    assert.equal(bytes(atOuter), 32768);
    assert.equal(
      verifyOutboundLayerData(
        original,
        original,
        atSeal,
        atOuter,
        owner,
        peer,
        'peer'
      ),
      true
    );
    assert.equal(
      verifyOutboundLayerData(
        original,
        original,
        atSeal + ' ',
        atOuter,
        owner,
        peer,
        'peer'
      ),
      false
    );
    assert.equal(
      verifyOutboundLayerData(
        original,
        original,
        atSeal,
        atOuter + ' ',
        owner,
        peer,
        'peer'
      ),
      false
    );
    console.log(
      JSON.stringify({
        fixture:
          'HCP079_DETACHED_SIGNED_RAW_JSON_BOUNDARIES_NOT_FACTORY_MAXIMA',
        seal_utf8: bytes(atSeal),
        outer_utf8: bytes(atOuter),
        seal_plus_one_rejected: true,
        outer_plus_one_rejected: true
      })
    );
  } finally {
    conversation?.fill(0);
    outerConversation?.fill(0);
    secret.fill(0);
    peerSecret.fill(0);
    outerSecret.fill(0);
  }
});
