import { describe, expect, it } from 'vitest';
import { finalizeEvent, getEventHash } from 'applesauce-core/helpers';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../../src/lib/config/budgets.ts';
import type {
  PublicDraftRecord,
  PublicRecord
} from '../../src/lib/contracts/local-records.ts';
import {
  decodePublicRecord,
  publicRecordSnapshot,
  publicRecordWire,
  decodeConversationMapping,
  conversationMappingSnapshot,
  type PublicRecordHandle
} from '../../src/lib/persistence/records.ts';
const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
const peer = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
const id = '3d030bf5-901d-45e1-8251-41cbdf805e96';
const anotherId = 'b1d6d23d-79f3-4779-9b03-e289712cc73f';
const form = {
  title: '',
  description: '',
  location: '',
  amount: '.',
  currency: '',
  unit: '',
  quantity: '',
  contactType: '' as const,
  contactValue: ''
};
const draft: PublicDraftRecord = {
  schema: 1,
  family: 'public_draft',
  owner,
  id,
  revision: 0,
  savedAtMilliseconds: 1000,
  form
};
function read(value: unknown, expectedOwner = owner, expectedId = id) {
  return decodePublicRecord(JSON.stringify(value), expectedOwner, expectedId);
}
function operation(
  kind: 30402 | 5 | 10050,
  tagsOverride?: (eventId: string, pubkey: string) => string[][],
  createdAt = 101
) {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    const base = finalizeEvent(
      {
        kind: kind === 10050 ? 10050 : 30402,
        created_at: 100,
        tags:
          kind === 10050
            ? []
            : [
                ['d', 'public'],
                ['published_at', '100']
              ],
        content: 'public source fixture'
      },
      key
    );
    const template = {
      pubkey: base.pubkey,
      kind,
      created_at: createdAt,
      tags: tagsOverride
        ? tagsOverride(base.id, base.pubkey)
        : kind === 5
          ? [
              ['e', base.id],
              ['a', `30402:${base.pubkey}:public`]
            ]
          : kind === 10050
            ? []
            : [
                ['d', 'public'],
                ['published_at', '100']
              ],
      content: 'public outgoing fixture'
    };
    const templateWire = JSON.stringify(template);
    const templateHash = getEventHash(template);
    const artifact = finalizeEvent(
      { ...template, tags: template.tags.map((tag) => [...tag]) },
      key
    );
    return {
      schema: 1,
      family: kind === 10050 ? 'preference_operation' : 'public_operation',
      owner: base.pubkey,
      id,
      revision: 1,
      ...(kind === 10050 ? { consent: 'explicit_review' } : {}),
      source: {
        type: kind === 10050 ? 'inbox_head' : 'public_head',
        wire: JSON.stringify(base)
      },
      capture: {
        kind,
        wire: templateWire,
        hash: templateHash,
        targets: ['wss://one.example.org'],
        policyFingerprint: 'a'.repeat(64)
      },
      artifact: { eventId: artifact.id, wire: JSON.stringify(artifact) },
      receipts: [
        {
          actionId: anotherId,
          origin: 'wss://one.example.org',
          role: kind === 10050 ? 'preference' : 'publication',
          attempt: 1,
          eventId: artifact.id,
          status: 'accepted',
          observedAtMilliseconds: 1234,
          readbackWire: JSON.stringify(artifact)
        }
      ]
    };
  } finally {
    key.fill(0);
  }
}
describe('owner-scoped canonical public record codecs', () => {
  it('preserves incomplete strings exactly without publication validation', () => {
    const result = read(draft);
    expect(result.ok).toBe(true);
    if (!result.ok) throw new Error(result.reason);
    expect(publicRecordSnapshot(result.value, owner, id)).toEqual(draft);
    expect(publicRecordWire(result.value, owner, id)).toBe(
      JSON.stringify(draft)
    );
  });
  it('rejects cross-owner/cross-ID decoding and handle reads', () => {
    expect(read(draft, peer)).toEqual({ ok: false, reason: 'owner_mismatch' });
    expect(read(draft, owner, anotherId)).toEqual({
      ok: false,
      reason: 'id_mismatch'
    });
    const result = read(draft);
    if (!result.ok) throw new Error(result.reason);
    expect(publicRecordSnapshot(result.value, peer, id)).toBeUndefined();
    expect(publicRecordWire(result.value, owner, anotherId)).toBeUndefined();
  });
  it('rejects noncanonical owner/UUID scope rather than normalizing it', () => {
    for (const bad of ['', owner.toUpperCase(), '0'.repeat(64)])
      expect(read(draft, bad).ok).toBe(false);
    for (const bad of ['', id.toUpperCase(), 'a'.repeat(64)])
      expect(read(draft, owner, bad).ok).toBe(false);
  });
  for (const schema of [0, 2, '1', null])
    it(`rejects unknown schema ${String(schema)}`, () => {
      expect(read({ ...draft, schema })).toEqual({
        ok: false,
        reason: 'unsupported_schema'
      });
    });
  it('rejects malformed/noncanonical/duplicate-key input without coercion', () => {
    for (const raw of [
      '{',
      'null',
      '[]',
      JSON.stringify(draft, null, 2),
      JSON.stringify(draft).replace('"schema":1', '"schema":2,"schema":1'),
      JSON.stringify(draft).replace('"revision":0', '"revision":-0')
    ])
      expect(decodePublicRecord(raw, owner, id).ok).toBe(false);
  });
  it('does not inspect getters on non-string database input', () => {
    let touched = false;
    const raw = {
      get schema() {
        touched = true;
        throw new Error('test-only');
      }
    };
    expect(decodePublicRecord(raw, owner, id)).toEqual({
      ok: false,
      reason: 'malformed'
    });
    expect(touched).toBe(false);
  });
  it('bounds malformed raw bytes before parsing', () => {
    expect(
      decodePublicRecord(
        'x'.repeat(LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes + 1),
        owner,
        id
      )
    ).toEqual({ ok: false, reason: 'oversized' });
    expect(
      decodePublicRecord(
        'é'.repeat(LOCAL_PERSISTENCE_BUDGETS.publicOperationBytes / 2 + 1),
        owner,
        id
      )
    ).toEqual({ ok: false, reason: 'oversized' });
  });
  it('counts composed UTF8 at the inclusive draft limit', () => {
    const blank = { ...form, amount: '' };
    expect(
      read({ ...draft, form: { ...blank, description: 'é'.repeat(8192) } }).ok
    ).toBe(true);
    expect(
      read({
        ...draft,
        form: { ...blank, description: 'é'.repeat(8192), title: 'x' }
      }).ok
    ).toBe(false);
    expect(
      read({ ...draft, form: { ...blank, description: '\ud800' } }).ok
    ).toBe(false);
  });
  it('rejects unknown/private fields, raw private families and unsafe revisions', () => {
    for (const record of [
      { ...draft, body: 'test-only private body' },
      { ...draft, family: 'private_message' },
      { ...draft, form: { ...form, subject: 'test-only' } },
      { ...draft, revision: -1 },
      { ...draft, revision: Number.MAX_SAFE_INTEGER + 1 },
      { ...draft, savedAtMilliseconds: -1 },
      { ...draft, form: { ...form, contactType: 'legacy' } }
    ])
      expect(read(record).ok).toBe(false);
    // Executable TypeScript check: a private-body field has no public member.
    const wrong: PublicRecord = {
      ...draft,
      // @ts-expect-error private body is outside the public record contract
      body: 'test-only private body'
    };
    expect(read(wrong).ok).toBe(false);
  });
  it('detaches snapshots and rejects fabricated capabilities', () => {
    const result = read(draft);
    if (!result.ok) throw new Error(result.reason);
    const first = publicRecordSnapshot(
      result.value,
      owner,
      id
    ) as PublicDraftRecord;
    Object.assign(first.form, { description: 'changed' });
    expect(publicRecordSnapshot(result.value, owner, id)).toEqual(draft);
    expect(
      publicRecordSnapshot(Object.freeze({}) as PublicRecordHandle, owner, id)
    ).toBeUndefined();
  });
  for (const kind of [30402, 5, 10050] as const)
    it(`binds actual signed ${kind} artifacts and separate named ACK/readback metadata`, () => {
      const value = operation(kind),
        result = read(value, value.owner);
      expect(result.ok).toBe(true);
      if (!result.ok) throw new Error(result.reason);
      expect(publicRecordSnapshot(result.value, value.owner, id)).toEqual(
        value
      );
    });
  it('rejects changed template/hash, artifact identity and destination bindings', () => {
    const value = operation(30402);
    for (const row of [
      { ...value, capture: { ...value.capture, hash: 'b'.repeat(64) } },
      {
        ...value,
        capture: {
          ...value.capture,
          wire: value.capture.wire.replace('outgoing', 'changed')
        }
      },
      {
        ...value,
        capture: {
          ...value.capture,
          targets: ['wss://one.example.org', 'wss://one.example.org']
        }
      },
      { ...value, capture: { ...value.capture, targets: ['wss://127.0.0.1'] } },
      { ...value, artifact: { ...value.artifact, eventId: 'b'.repeat(64) } },
      {
        ...value,
        receipts: [{ ...value.receipts[0], origin: 'wss://other.example.org' }]
      },
      { ...value, receipts: [{ ...value.receipts[0], attempt: 4 }] },
      { ...value, receipts: [{ ...value.receipts[0], role: 'archive' }] },
      {
        ...value,
        receipts: [{ ...value.receipts[0], eventId: 'b'.repeat(64) }]
      },
      { ...value, artifact: null }
    ])
      expect(read(row, value.owner).ok).toBe(false);
  });
  it('binds genuine replacement coordinates, original publication time and advancing revision time', () => {
    for (const value of [
      operation(30402, () => [
        ['d', 'other'],
        ['published_at', '100']
      ]),
      operation(30402, () => [
        ['d', 'public'],
        ['published_at', '101']
      ]),
      operation(30402, undefined, 100)
    ])
      expect(read(value, value.owner).ok).toBe(false);
  });
  it('binds genuine withdrawal to the captured version/coordinate and rejects additional unrelated targets', () => {
    for (const value of [
      operation(5, () => [['e', 'b'.repeat(64)]]),
      operation(5, (_, pubkey) => [['a', `30402:${pubkey}:other`]]),
      operation(5, (eventId) => [
        ['e', eventId],
        ['e', 'b'.repeat(64)]
      ]),
      operation(5, () => []),
      operation(5, undefined, 99)
    ])
      expect(read(value, value.owner).ok).toBe(false);
  });
  it('preserves original verified signed-wire layout while outer/template serialization stays canonical', () => {
    const value = operation(30402);
    function formatted(wire: string) {
      const parsed: unknown = JSON.parse(wire);
      return JSON.stringify(parsed, null, 2);
    }
    const aliases = {
      ...value,
      source: { ...value.source, wire: formatted(value.source.wire) },
      artifact: { ...value.artifact, wire: formatted(value.artifact.wire) },
      receipts: [
        {
          ...value.receipts[0],
          readbackWire: formatted(value.receipts[0].readbackWire)
        }
      ]
    };
    const ready = read(aliases, aliases.owner);
    expect(ready.ok).toBe(true);
    if (!ready.ok) throw new Error(ready.reason);
    expect(publicRecordSnapshot(ready.value, aliases.owner, id)).toEqual(
      aliases
    );
  });
  it('admits prepared-only records with no remote receipt claims', () => {
    const value = operation(30402);
    expect(
      read(
        {
          ...value,
          artifact: null,
          receipts: [],
          source: { type: 'draft', id, revision: 0 }
        },
        value.owner
      ).ok
    ).toBe(true);
  });
  it('rejects private signed kinds and forged signature/readback', () => {
    const value = operation(30402);
    for (const kind of [14, 13, 1059]) {
      // Genuine generic NIP01 signatures for forbidden kinds, not qualified
      // private message construction/encryption. Runtime input ignores TS casts.
      const privateKind = operation(kind as 30402);
      expect(verifyEnvelope(privateKind.artifact.wire).ok).toBe(true);
      expect(read(privateKind, privateKind.owner).ok).toBe(false);
    }
    const artifact = JSON.parse(value.artifact.wire) as {
      sig: string;
      kind: number;
    };
    artifact.sig = '0'.repeat(128);
    expect(
      read(
        {
          ...value,
          artifact: { ...value.artifact, wire: JSON.stringify(artifact) }
        },
        value.owner
      ).ok
    ).toBe(false);
    expect(
      read(
        {
          ...value,
          receipts: [{ ...value.receipts[0], readbackWire: value.source.wire }]
        },
        value.owner
      ).ok
    ).toBe(false);
  });
  it('keeps preference consent/source/role distinct from public operations', () => {
    const value = operation(10050);
    for (const row of [
      { ...value, consent: 'inferred' },
      { ...value, source: { type: 'draft', id, revision: 0 } },
      { ...value, family: 'public_operation' },
      { ...value, receipts: [{ ...value.receipts[0], role: 'publication' }] }
    ])
      expect(read(row, value.owner).ok).toBe(false);
  });
});
describe('separate opaque conversation mapping contracts', () => {
  const mapping = { schema: 1, family: 'conversation_handle', owner, id, peer };
  it('binds two distinct validated keys to an opaque local UUID', () => {
    const result = decodeConversationMapping(
      JSON.stringify(mapping),
      owner,
      id
    );
    expect(result.ok).toBe(true);
    if (!result.ok) throw new Error(result.reason);
    expect(conversationMappingSnapshot(result.value, owner, id)).toEqual(
      mapping
    );
    expect(conversationMappingSnapshot(result.value, peer, id)).toBeUndefined();
    expect(read(mapping).ok).toBe(false);
  });
  it('rejects self peer, private plaintext/listing fields and unknown scope', () => {
    for (const row of [
      { ...mapping, peer: owner },
      { ...mapping, peer: '0'.repeat(64) },
      { ...mapping, body: 'test-only' },
      { ...mapping, listing: 'test-only' },
      { ...mapping, schema: 2 }
    ])
      expect(decodeConversationMapping(JSON.stringify(row), owner, id).ok).toBe(
        false
      );
    expect(
      decodeConversationMapping(JSON.stringify(mapping), peer, id).ok
    ).toBe(false);
  });
});
