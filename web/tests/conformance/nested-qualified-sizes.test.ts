import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  generateSecretKey,
  getPublicKey,
  finalizeEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { buildCanonicalPairRumor } from '../../src/lib/nostr/rumor-template.ts';
import { verifyOutboundLayerData } from '../../src/lib/nostr/outbound-envelope-layers.ts';
import { PRIVATE_TRANSPORT_BUDGETS } from '../../src/lib/config/budgets.ts';
const bytes = (value: string) => new TextEncoder().encode(value).length;
await test('actual stock NIP44 self and peer expansion preserves maximal Unicode body, subject, parent and exact escaped rumor under all nested byte caps', () => {
  const keys = [
    generateSecretKey(),
    generateSecretKey(),
    generateSecretKey(),
    generateSecretKey()
  ];
  try {
    const owner = getPublicKey(keys[0]),
      peer = getPublicKey(keys[1]);
    const input = (content: string) =>
      JSON.stringify({
        recipients: [{ public_key: peer, relay_url: null }],
        content,
        reply_to: { id: '1'.repeat(64), relays: null },
        subject: '🥕'.repeat(160)
      });
    const plan = (content: string) =>
      buildCanonicalPairRumor(input(content), owner, 1700000000);
    const recipes: [string, string][] = [
      ['ASCII', 'x'.repeat(4096)],
      ['BMP', 'é'.repeat(2048)],
      ['astral', '😀'.repeat(1024)],
      ['mixed_escaped', '😀é"\n'.repeat(512)]
    ];
    let low = 0,
      high = 4096;
    while (low < high) {
      const middle = Math.ceil((low + high) / 2);
      if (plan('\u0001'.repeat(middle))) low = middle;
      else high = middle - 1;
    }
    let maximal = '\u0001'.repeat(low);
    for (let count = 0; count < 6 && plan(maximal + 'x'); count++)
      maximal += 'x';
    recipes.push(['maximum_escaped_rumor_with_subject_parent', maximal]);
    const rows = [];
    for (const [shape, body] of recipes) {
      const originalInput = input(body),
        rumor = plan(body);
      assert.ok(rumor, shape);
      assert.equal(rumor.content, body);
      assert.equal(input(body), originalInput);
      const rumorWire = JSON.stringify(rumor);
      assert.equal(
        plan(body + 'x'),
        undefined,
        `${shape}: inclusive original boundary`
      );
      if (shape.startsWith('maximum'))
        assert.equal(bytes(rumorWire), PRIVATE_TRANSPORT_BUDGETS.rumorBytes);
      else assert.equal(bytes(body), PRIVATE_TRANSPORT_BUDGETS.bodyBytes);
      for (const [index, role] of ['self', 'peer'].entries()) {
        const target = role === 'self' ? owner : peer;
        const sealKey = nip44.v2.utils.getConversationKey(keys[0], target);
        const wrapKey = nip44.v2.utils.getConversationKey(
          keys[index + 2],
          target
        );
        try {
          const seal = finalizeEvent(
            {
              kind: 13,
              created_at: 1699999900,
              tags: [],
              content: nip44.v2.encrypt(rumorWire, sealKey)
            },
            keys[0]
          );
          const sealWire = JSON.stringify(seal);
          const outer = finalizeEvent(
            {
              kind: 1059,
              created_at: 1699999800,
              tags: [['p', target]],
              content: nip44.v2.encrypt(sealWire, wrapKey)
            },
            keys[index + 2]
          );
          const outerWire = JSON.stringify(outer);
          assert.equal(nip44.v2.decrypt(outer.content, wrapKey), sealWire);
          assert.equal(nip44.v2.decrypt(seal.content, sealKey), rumorWire);
          assert.equal(
            verifyOutboundLayerData(
              rumorWire,
              rumorWire,
              sealWire,
              outerWire,
              owner,
              peer,
              role
            ),
            true
          );
          assert.ok(bytes(body) <= PRIVATE_TRANSPORT_BUDGETS.bodyBytes);
          assert.ok(bytes(rumorWire) <= PRIVATE_TRANSPORT_BUDGETS.rumorBytes);
          assert.ok(bytes(sealWire) <= PRIVATE_TRANSPORT_BUDGETS.sealBytes);
          assert.ok(
            bytes(outerWire) <= PRIVATE_TRANSPORT_BUDGETS.envelopeBytes
          );
          assert.equal(outer.tags.length, 1);
          assert.equal(outer.tags[0][1], target);
          rows.push({
            shape,
            role,
            body_utf8: bytes(body),
            rumor_utf8: bytes(rumorWire),
            seal_ciphertext_utf8: bytes(seal.content),
            signed_seal_utf8: bytes(sealWire),
            outer_ciphertext_utf8: bytes(outer.content),
            signed_outer_utf8: bytes(outerWire)
          });
        } finally {
          sealKey.fill(0);
          wrapKey.fill(0);
        }
      }
    }
    assert.equal(rows.length, 10);
    console.log(
      JSON.stringify({
        fixture:
          'HCP132_ACTUAL_STOCK_CRYPTO_DETACHED_NO_FACTORY_CUSTODY_OR_CLIENT_CLAIM',
        rows
      })
    );
  } finally {
    keys.forEach((key) => key.fill(0));
  }
});
