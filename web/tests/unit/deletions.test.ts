import { afterAll, describe, expect, it } from 'vitest';
import { finalizeEvent } from 'applesauce-core/helpers';
import {
  verifyEnvelope,
  type VerifiedEnvelope
} from '../../src/lib/nostr/verified-envelope.ts';
import {
  admitDeletionRequest,
  deletionRequestEnvelope,
  deletionRequestSnapshot,
  type DeletionRequest
} from '../../src/lib/nostr/deletion-adapter.ts';
import {
  evaluateDeletion,
  evaluatePublicHeadDeletion
} from '../../src/lib/catalog/deletions.ts';
import {
  createPublicHeadCandidate,
  selectPublicHead,
  publicHeadSnapshot
} from '../../src/lib/catalog/heads.ts';

const keys: Uint8Array[] = [];
afterAll(() => {
  for (const key of keys) key.fill(0);
});
function author() {
  const key = crypto.getRandomValues(new Uint8Array(32));
  keys.push(key);
  return (time: number, tags: string[][], kind = 30402, content = '') => {
    const event = finalizeEvent({ kind, created_at: time, tags, content }, key);
    const result = verifyEnvelope(JSON.stringify(event));
    if (!result.ok) throw new Error('Invalid signed test event');
    return { event, proof: result.value };
  };
}
function admitted(proof: VerifiedEnvelope) {
  const result = admitDeletionRequest(proof);
  if (!result.ok) throw new Error(result.code);
  return result.value;
}
function fixture() {
  const a = author(),
    b = author(),
    target = a(100, [['d', 'stock']]);
  const address = `30402:${target.event.pubkey}:stock`;
  const request = (time: number, tags = [['a', address]]) =>
    admitted(a(time, tags, 5).proof);
  return { a, b, target, address, request };
}
describe('verified owned NIP09 deletion evidence', () => {
  it('retains the genuine original verified envelope for retained tombstone evidence', () => {
    const f = fixture();
    const original = f.a(100, [['a', f.address]], 5, 'withdrawal evidence');
    const request = admitted(original.proof);
    expect(deletionRequestEnvelope(request)).toBe(original.proof);
    expect(deletionRequestEnvelope({} as DeletionRequest)).toBeUndefined();
  });
  it('retains admitted deletion before the target is evaluated', () => {
    const f = fixture(),
      deletion = f.request(100);
    expect(evaluateDeletion(f.target.proof, [deletion])).toMatchObject({
      outcome: 'suppressed',
      reason: 'deletion_address_reference',
      addressReference: { inclusiveCutoff: 100 }
    });
  });
  it('rejects matching references by the wrong author', () => {
    const f = fixture();
    const wrong = admitted(
      f.b(
        101,
        [
          ['e', f.target.event.id],
          ['a', f.address]
        ],
        5
      ).proof
    );
    expect(evaluateDeletion(f.target.proof, [wrong])).toMatchObject({
      outcome: 'visible',
      reason: 'deletion_request_author_mismatch',
      eventReference: null,
      addressReference: null
    });
  });
  it('uses inclusive address cutoff and allows a genuinely later revision', () => {
    const f = fixture(),
      request = f.request(100);
    expect(evaluateDeletion(f.target.proof, [request]).outcome).toBe(
      'suppressed'
    );
    expect(
      evaluateDeletion(f.a(101, [['d', 'stock']]).proof, [request])
    ).toMatchObject({
      outcome: 'visible',
      reason: 'deletion_address_cutoff_precedes_target',
      addressReference: { inclusiveCutoff: 100 }
    });
    expect(
      evaluateDeletion(f.a(99, [['d', 'stock']]).proof, [request]).outcome
    ).toBe('suppressed');
  });
  it('applies an exact e reference regardless of request timestamp or k mismatch', () => {
    const f = fixture(),
      request = f.request(0, [
        ['e', f.target.event.id],
        ['k', '1']
      ]);
    expect(evaluateDeletion(f.target.proof, [request])).toMatchObject({
      outcome: 'suppressed',
      reason: 'deletion_event_id_reference',
      addressReference: null
    });
  });
  it('cannot resurrect an older head after withdrawal or repeated old arrival', () => {
    const f = fixture();
    const old = createPublicHeadCandidate(f.a(99, [['d', 'stock']]).proof)!;
    const head = createPublicHeadCandidate(f.target.proof)!;
    const tombstones = [f.request(100)];
    const selected = selectPublicHead(head, old);
    expect(selected.decision).toBe('older');
    expect(publicHeadSnapshot(selected.head).id).toBe(f.target.event.id);
    expect(evaluatePublicHeadDeletion(selected.head, tombstones).outcome).toBe(
      'suppressed'
    );
    expect(evaluatePublicHeadDeletion(selected.head, tombstones).outcome).toBe(
      'suppressed'
    );
  });
  it('never treats kind5 itself as retractable evidence', () => {
    const f = fixture(),
      target = f.a(100, [['e', f.target.event.id]], 5);
    expect(
      evaluateDeletion(target.proof, [f.request(101, [['e', target.event.id]])])
    ).toEqual({
      outcome: 'visible',
      reason: 'deletion_request_immune',
      eventReference: null,
      addressReference: null
    });
  });
  it('distinguishes unrelated requests from wrong authors', () => {
    const f = fixture();
    expect(
      evaluateDeletion(f.target.proof, [
        f.request(101, [['e', 'f'.repeat(64)]])
      ]).reason
    ).toBe('deletion_no_authorized_reference');
  });
  it('chooses lowest exact request ID and greatest cutoff/lower-ID address evidence independent of order and repeats', () => {
    const f = fixture(),
      exactA = f.request(0, [['e', f.target.event.id]]),
      exactB = f.request(200, [['e', f.target.event.id]]);
    const addressA = f.request(120),
      addressB = f.request(120, [
        ['a', f.address],
        ['x', 'different']
      ]),
      stale = f.request(110);
    const rows = [exactA, addressA, exactB, addressB, stale];
    const expected = evaluateDeletion(f.target.proof, rows);
    expect(
      evaluateDeletion(f.target.proof, [...rows].reverse().concat(rows))
    ).toEqual(expected);
    expect(expected).toMatchObject({
      reason: 'deletion_event_id_and_address_reference',
      eventReference: {
        requestId: [exactA, exactB]
          .map((v) => deletionRequestSnapshot(v)!.id)
          .sort()[0]
      },
      addressReference: {
        inclusiveCutoff: 120,
        requestId: [addressA, addressB]
          .map((v) => deletionRequestSnapshot(v)!.id)
          .sort()[0]
      }
    });
  });
  it('retains stale authorized address evidence in preference to wrong-author reason', () => {
    const f = fixture(),
      wrong = admitted(f.b(200, [['e', f.target.event.id]], 5).proof);
    expect(
      evaluateDeletion(f.target.proof, [wrong, f.request(99)]).reason
    ).toBe('deletion_address_cutoff_precedes_target');
  });
  it('uses first d exactly, with no address for missing or malformed first d', () => {
    const f = fixture(),
      empty = f.request(100, [['a', `30402:${f.target.event.pubkey}:`]]);
    for (const tags of [[], [['d']], [['d'], ['d', 'stock']]])
      expect(
        evaluateDeletion(f.a(100, tags).proof, [empty, f.request(100)]).outcome
      ).toBe('visible');
    expect(evaluateDeletion(f.a(100, [['d', '']]).proof, [empty]).outcome).toBe(
      'suppressed'
    );
    expect(
      evaluateDeletion(
        f.a(100, [
          ['d', 'stock'],
          ['d', 'other']
        ]).proof,
        [f.request(100)]
      ).outcome
    ).toBe('suppressed');
  });
  it('preserves opaque identifier bytes and does not normalize titles, whitespace, Unicode or colon', () => {
    const f = fixture();
    for (const id of ['a:b', ' stock ', 'é', 'e\u0301', '\ufeffstock']) {
      const target = f.a(100, [['d', id]]);
      const request = f.request(100, [
        ['a', `30402:${target.event.pubkey}:${id}`]
      ]);
      expect(evaluateDeletion(target.proof, [request]).outcome).toBe(
        'suppressed'
      );
      expect(evaluateDeletion(f.target.proof, [request]).outcome).toBe(
        'visible'
      );
    }
  });
  it('accepts generic replaceable coordinate kinds with empty identifiers', () => {
    const f = fixture();
    for (const kind of [0, 3, 10000, 19999]) {
      const target = f.a(100, [['d', 'ignored']], kind);
      const request = f.request(100, [
        ['a', `${kind}:${target.event.pubkey}:`]
      ]);
      expect(evaluateDeletion(target.proof, [request]).outcome).toBe(
        'suppressed'
      );
    }
  });
  it('canonicalizes mixed-case e/a and plus/leading-zero coordinate kinds while retaining first raw provenance', () => {
    const f = fixture();
    const tags = [
      ['e', f.target.event.id.toUpperCase(), 'relay'],
      ['e', f.target.event.id],
      ['a', `+030402:${f.target.event.pubkey.toUpperCase()}:stock`, 'extra'],
      ['a', f.address],
      ['x', 'unknown']
    ];
    const request = f.request(100, tags),
      view = deletionRequestSnapshot(request)!;
    expect(view.rawTags).toEqual(tags);
    expect(view.eventTargets).toEqual([
      { tagIndex: 0, eventId: f.target.event.id, rawTag: tags[0] }
    ]);
    expect(view.addressTargets).toEqual([
      { tagIndex: 2, coordinate: f.address, rawTag: tags[2] }
    ]);
    expect(evaluateDeletion(f.target.proof, [request]).reason).toBe(
      'deletion_event_id_and_address_reference'
    );
  });
  it('keeps k advisory and returns ordered shape/invalid/duplicate/conflict diagnostics', () => {
    const f = fixture(),
      request = f.request(100, [
        ['k'],
        ['a', f.address],
        ['k', '+30402'],
        ['k', '30402'],
        ['k', '30402', 'duplicate'],
        ['k', '31923'],
        ['k', '65536']
      ]);
    const view = deletionRequestSnapshot(request)!;
    expect(view.kindAdvisories.map((v) => v.kind)).toEqual([30402, 31923]);
    expect(view.diagnostics.map((v) => [v.tagIndex, v.code])).toEqual([
      [0, 'deletion_kind_advisory_shape_ignored'],
      [2, 'deletion_kind_advisory_invalid_ignored'],
      [4, 'deletion_kind_advisory_duplicate_ignored'],
      [5, 'deletion_kind_advisory_conflict_ignored'],
      [6, 'deletion_kind_advisory_invalid_ignored']
    ]);
    expect(evaluateDeletion(f.target.proof, [request]).outcome).toBe(
      'suppressed'
    );
    expect(
      deletionRequestSnapshot(
        f.request(100, [
          ['a', f.address],
          ['e', f.target.event.id],
          ['k', '31923']
        ])
      )!.diagnostics
    ).toEqual([]);
  });
  it('rejects any malformed e/a in source order rather than partially admitting a request', () => {
    const f = fixture();
    for (const [tags, code] of [
      [[['a', f.address], ['e'], ['a', 'bad']], 'deletion_event_target_shape'],
      [
        [
          ['e', f.target.event.id],
          ['a', 'bad'],
          ['e', 'bad']
        ],
        'deletion_address_target_invalid'
      ],
      [[['e', 'bad']], 'deletion_event_target_invalid'],
      [[['a']], 'deletion_address_target_shape'],
      [[['x', 'unknown'], ['k']], 'deletion_target_missing']
    ] as [string[][], string][])
      expect(admitDeletionRequest(f.a(100, tags, 5).proof)).toMatchObject({
        ok: false,
        code
      });
  });
  it('rejects unsupported/invalid address kinds, nonempty replaceable identifiers, bad curve keys and whitespace', () => {
    const f = fixture();
    for (const value of [
      `1:${f.target.event.pubkey}:`,
      `0:${f.target.event.pubkey}:x`,
      '30402:' + '0'.repeat(64) + ':stock',
      ` 30402:${f.target.event.pubkey}:stock`,
      `30402:${f.target.event.pubkey}:stock` +
        'x'.repeat(4097 - f.address.length)
    ]) {
      const tags = [['a', value]];
      const wire = JSON.stringify(
        finalizeEvent({ kind: 5, created_at: 100, tags, content: '' }, keys[0])
      );
      const proof = verifyEnvelope(wire);
      if (!proof.ok) {
        expect(value.length).toBeGreaterThanOrEqual(4096);
        continue;
      }
      expect(admitDeletionRequest(proof.value).ok).toBe(false);
    }
  });
  it('requires genuine proofs and admitted ownership rather than SDK flags or forged tokens', () => {
    const f = fixture();
    expect(admitDeletionRequest({} as VerifiedEnvelope)).toEqual({
      ok: false,
      code: 'deletion_proof_invalid'
    });
    expect(admitDeletionRequest(f.target.proof)).toEqual({
      ok: false,
      code: 'unsupported_kind'
    });
    expect(deletionRequestSnapshot({} as DeletionRequest)).toBeUndefined();
    expect(
      evaluateDeletion(f.target.proof, [{} as DeletionRequest]).outcome
    ).toBe('visible');
    expect(() => evaluateDeletion({} as VerifiedEnvelope, [])).toThrow(
      'deletion_target_proof_invalid'
    );
  });
  it('protects retained tombstone metadata and raw tags from caller mutation', () => {
    const f = fixture(),
      request = f.request(100),
      view = deletionRequestSnapshot(request)!;
    (view.rawTags as string[][])[0][1] = 'bad';
    (view.addressTargets as unknown as { coordinate: string }[])[0].coordinate =
      'bad';
    expect(deletionRequestSnapshot(request)!.rawTags[0][1]).toBe(f.address);
    expect(evaluateDeletion(f.target.proof, [request]).outcome).toBe(
      'suppressed'
    );
  });
});
