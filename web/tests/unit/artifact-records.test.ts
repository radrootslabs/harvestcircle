import { expect, it } from 'vitest';
import { finalizeEvent, getEventHash } from 'applesauce-core/helpers';
import {
  decodePublicRecord,
  publicRecordSnapshot
} from '../../src/lib/persistence/records.ts';
import type {
  PublicOperationRecord,
  PreferenceOperationRecord,
  PublicTargetReceipt
} from '../../src/lib/contracts/local-records.ts';
import {
  bindCapturedArtifact,
  capturedArtifactSnapshot,
  preparePublicArtifactTransition,
  preparePublicReceiptTransition,
  publicTransitionSnapshot,
  type CapturedArtifact,
  type PublicOperationTransition
} from '../../src/lib/persistence/artifact-records.ts';
import { decideFrozenTransition } from '../../src/lib/persistence/operation-transactions.ts';
const id = '3d030bf5-901d-45e1-8251-41cbdf805e96';
const actionId = 'b1d6d23d-79f3-4779-9b03-e289712cc73f';
function fixture(kind: 30402 | 10050 | 1059 = 30402) {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    const event = finalizeEvent(
      {
        kind,
        created_at: 100,
        tags:
          kind === 30402
            ? [
                ['d', 'transition'],
                ['published_at', '100']
              ]
            : [],
        content:
          kind === 1059 ? 'controlled ciphertext fixture' : 'public fixture'
      },
      key
    );
    const template = {
      pubkey: event.pubkey,
      kind: kind === 1059 ? 30402 : kind,
      created_at: event.created_at,
      tags: event.tags,
      content: event.content
    };
    const capture = {
      kind: kind === 10050 ? (10050 as const) : (30402 as const),
      wire: JSON.stringify(template),
      hash: getEventHash(template),
      targets: ['wss://one.example.org'],
      policyFingerprint: 'a'.repeat(64)
    };
    const row: PublicOperationRecord | PreferenceOperationRecord =
      kind === 10050
        ? {
            schema: 1,
            family: 'preference_operation',
            owner: event.pubkey,
            id,
            revision: 0,
            source: { type: 'inbox_head', wire: null },
            consent: 'explicit_review',
            capture: { ...capture, kind: 10050 },
            artifact: null,
            receipts: []
          }
        : {
            schema: 1,
            family: 'public_operation',
            owner: event.pubkey,
            id,
            revision: 0,
            source: { type: 'draft', id, revision: 0 },
            capture: { ...capture, kind: 30402 },
            artifact: null,
            receipts: []
          };
    return { row, event, wire: JSON.stringify(event) };
  } finally {
    key.fill(0);
  }
}
function handle(row: PublicOperationRecord | PreferenceOperationRecord) {
  const decoded = decodePublicRecord(JSON.stringify(row), row.owner, row.id);
  if (!decoded.ok) throw new Error(decoded.reason);
  return decoded.value;
}
function artifact(f: ReturnType<typeof fixture>) {
  const result = bindCapturedArtifact(
    f.wire,
    f.row.owner,
    f.event.kind,
    f.event.id
  );
  if (!result.ok) throw new Error(result.reason);
  return result.value;
}
function signedRow(f: ReturnType<typeof fixture>) {
  return {
    ...f.row,
    revision: 1,
    artifact: { eventId: f.event.id, wire: f.wire }
  };
}
function receipt(f: ReturnType<typeof fixture>): PublicTargetReceipt {
  return {
    actionId,
    origin: 'wss://one.example.org',
    role:
      f.row.family === 'preference_operation' ? 'preference' : 'publication',
    attempt: 1,
    eventId: f.event.id,
    status: 'accepted',
    observedAtMilliseconds: 1000,
    readbackWire: null
  };
}
it('SDK signature binding rejects changed author, kind, hash and forged wire', () => {
  const f = fixture();
  expect(bindCapturedArtifact(f.wire, f.row.owner, 30402, f.event.id).ok).toBe(
    true
  );
  for (const args of [
    [
      f.wire,
      'c6047f9441ed7d6d3045406e95c07cd85c778e4bcef3ca7abac09b95c709ee5',
      30402,
      f.event.id
    ],
    [f.wire, f.row.owner, 10050, f.event.id],
    [f.wire, f.row.owner, 30402, 'b'.repeat(64)],
    [
      JSON.stringify({ ...f.event, sig: '0'.repeat(128) }),
      f.row.owner,
      30402,
      f.event.id
    ]
  ] as const)
    expect(bindCapturedArtifact(args[0], args[1], args[2], args[3]).ok).toBe(
      false
    );
});
it('outer1059 binding is only a mechanical capability and cannot enter public record transitions', () => {
  const f = fixture(1059);
  expect(bindCapturedArtifact(f.wire, f.row.owner, 1059, f.event.id).ok).toBe(
    true
  );
  expect(
    preparePublicArtifactTransition(handle(f.row), f.row.owner, id, artifact(f))
      .ok
  ).toBe(false);
  expect(bindCapturedArtifact(f.wire, f.row.owner, 14, f.event.id).ok).toBe(
    false
  );
});
it('artifact transition binds original full wire and advances one revision with immutable capture', () => {
  const f = fixture();
  const base = handle(f.row);
  const result = preparePublicArtifactTransition(
    base,
    f.row.owner,
    id,
    artifact(f)
  );
  expect(result.ok).toBe(true);
  if (!result.ok) return;
  const state = publicTransitionSnapshot(result.value, f.row.owner, id);
  expect(state?.baseWire).toBe(JSON.stringify(f.row));
  expect(state?.revision).toBe(1);
  const decoded = decodePublicRecord(state?.nextWire, f.row.owner, id);
  expect(decoded.ok).toBe(true);
  if (!decoded.ok) return;
  const next = publicRecordSnapshot(decoded.value, f.row.owner, id);
  expect(next).toMatchObject({
    ...f.row,
    revision: 1,
    artifact: { eventId: f.event.id, wire: f.wire }
  });
});
it('same exact already-bound artifact is idempotent but different signed wire conflicts', () => {
  const f = fixture();
  const row = signedRow(f);
  const same = preparePublicArtifactTransition(
    handle(row),
    row.owner,
    id,
    artifact(f)
  );
  expect(same.ok).toBe(true);
  if (!same.ok) return;
  expect(publicTransitionSnapshot(same.value, row.owner, id)?.nextWire).toBe(
    JSON.stringify(row)
  );
  const formatted = bindCapturedArtifact(
    JSON.stringify(f.event, null, 2),
    row.owner,
    30402,
    f.event.id
  );
  expect(formatted.ok).toBe(true);
  if (!formatted.ok) return;
  expect(
    preparePublicArtifactTransition(handle(row), row.owner, id, formatted.value)
  ).toEqual({ ok: false, reason: 'conflict' });
});
it('forged capabilities, wrong scope and maximum revision fail before transitions', () => {
  const f = fixture();
  expect(
    preparePublicArtifactTransition(
      handle(f.row),
      f.row.owner,
      id,
      {} as CapturedArtifact
    ).ok
  ).toBe(false);
  expect(
    publicTransitionSnapshot({} as PublicOperationTransition, f.row.owner, id)
  ).toBeUndefined();
  expect(
    preparePublicArtifactTransition(
      handle({ ...f.row, revision: Number.MAX_SAFE_INTEGER }),
      f.row.owner,
      id,
      artifact(f)
    ).ok
  ).toBe(false);
  expect(
    preparePublicArtifactTransition(
      handle(f.row),
      'b'.repeat(64),
      id,
      artifact(f)
    ).ok
  ).toBe(false);
});
it('preference artifact remains typed, consent and source unchanged', () => {
  const f = fixture(10050);
  const result = preparePublicArtifactTransition(
    handle(f.row),
    f.row.owner,
    id,
    artifact(f)
  );
  expect(result.ok).toBe(true);
  if (!result.ok) return;
  const state = publicTransitionSnapshot(result.value, f.row.owner, id);
  expect(JSON.parse(state?.nextWire ?? 'null')).toMatchObject({
    family: 'preference_operation',
    consent: 'explicit_review',
    source: { type: 'inbox_head', wire: null },
    capture: f.row.capture
  });
});
it('receipt append preserves separate observations and exact duplicate has no revision growth', () => {
  const f = fixture();
  const row = signedRow(f);
  const fact = receipt(f);
  const first = preparePublicReceiptTransition(
    handle(row),
    row.owner,
    id,
    JSON.stringify(fact)
  );
  expect(first.ok).toBe(true);
  if (!first.ok) return;
  const state = publicTransitionSnapshot(first.value, row.owner, id);
  const next = JSON.parse(state?.nextWire ?? 'null') as PublicOperationRecord;
  expect(next.receipts).toEqual([fact]);
  expect(next.revision).toBe(2);
  const duplicate = preparePublicReceiptTransition(
    handle(next),
    row.owner,
    id,
    JSON.stringify(fact)
  );
  expect(duplicate.ok).toBe(true);
  if (!duplicate.ok) return;
  expect(
    publicTransitionSnapshot(duplicate.value, row.owner, id)?.nextWire
  ).toBe(JSON.stringify(next));
  const timed = preparePublicReceiptTransition(
    handle(next),
    row.owner,
    id,
    JSON.stringify({
      ...fact,
      status: 'timed_out',
      observedAtMilliseconds: 999
    })
  );
  expect(timed.ok).toBe(true);
  if (!timed.ok) return;
  expect(
    (
      JSON.parse(
        publicTransitionSnapshot(timed.value, row.owner, id)?.nextWire ?? 'null'
      ) as PublicOperationRecord
    ).receipts
  ).toHaveLength(2);
});
it('same fact identity with changed readback conflicts; wrong role/target/event or unsigned record rejects', () => {
  const f = fixture();
  const fact = receipt(f);
  const row = { ...signedRow(f), receipts: [fact] };
  expect(
    preparePublicReceiptTransition(
      handle(row),
      row.owner,
      id,
      JSON.stringify({ ...fact, readbackWire: f.wire })
    )
  ).toEqual({ ok: false, reason: 'conflict' });
  for (const changed of [
    { ...fact, role: 'preference' },
    { ...fact, origin: 'wss://other.example.org' },
    { ...fact, eventId: 'b'.repeat(64) },
    { ...fact, body: 'forbidden' }
  ])
    expect(
      preparePublicReceiptTransition(
        handle(signedRow(f)),
        row.owner,
        id,
        JSON.stringify(changed)
      ).ok
    ).toBe(false);
  expect(
    preparePublicReceiptTransition(
      handle(f.row),
      row.owner,
      id,
      JSON.stringify(fact)
    ).ok
  ).toBe(false);
});
it('returned snapshots are detached and cannot retarget an existing transition', () => {
  const f = fixture();
  const token = artifact(f);
  const snapshot = capturedArtifactSnapshot(token);
  expect(snapshot?.wire).toBe(f.wire);
  if (snapshot) (snapshot as { wire: string }).wire = 'changed';
  expect(capturedArtifactSnapshot(token)?.wire).toBe(f.wire);
  const result = preparePublicArtifactTransition(
    handle(f.row),
    f.row.owner,
    id,
    token
  );
  if (!result.ok) throw new Error(result.reason);
  const view = publicTransitionSnapshot(result.value, f.row.owner, id);
  if (view) (view as { nextWire: string }).nextWire = 'changed';
  expect(
    publicTransitionSnapshot(result.value, f.row.owner, id)?.nextWire
  ).not.toBe('changed');
});
it('full original and expected payload comparison distinguishes committed, base and conflict', () => {
  expect(decideFrozenTransition('next', 'base', 'next')).toBe('committed');
  expect(decideFrozenTransition('base', 'base', 'next')).toBe('base_observed');
  expect(decideFrozenTransition('different', 'base', 'next')).toBe('conflict');
  expect(decideFrozenTransition(undefined, 'base', 'next')).toBe('conflict');
});

it('actual signed kind5 binds its immutable own-author source and rejects cross-kind binding', () => {
  const key = crypto.getRandomValues(new Uint8Array(32));
  try {
    const base = finalizeEvent(
      {
        kind: 30402,
        created_at: 99,
        tags: [
          ['d', 'withdraw'],
          ['published_at', '99']
        ],
        content: 'public source'
      },
      key
    );
    const event = finalizeEvent(
      { kind: 5, created_at: 100, tags: [['e', base.id]], content: 'withdraw' },
      key
    );
    const template = {
      pubkey: event.pubkey,
      kind: 5,
      created_at: 100,
      tags: event.tags,
      content: event.content
    };
    const row: PublicOperationRecord = {
      schema: 1,
      family: 'public_operation',
      owner: event.pubkey,
      id,
      revision: 0,
      source: { type: 'public_head', wire: JSON.stringify(base) },
      capture: {
        kind: 5,
        wire: JSON.stringify(template),
        hash: getEventHash(template),
        targets: ['wss://one.example.org'],
        policyFingerprint: 'a'.repeat(64)
      },
      artifact: null,
      receipts: []
    };
    const bound = bindCapturedArtifact(
      JSON.stringify(event),
      row.owner,
      5,
      event.id
    );
    expect(bound.ok).toBe(true);
    if (!bound.ok) return;
    const result = preparePublicArtifactTransition(
      handle(row),
      row.owner,
      id,
      bound.value
    );
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(
      JSON.parse(
        publicTransitionSnapshot(result.value, row.owner, id)?.nextWire ??
          'null'
      )
    ).toMatchObject({
      capture: row.capture,
      source: row.source,
      artifact: { eventId: event.id, wire: JSON.stringify(event) }
    });
    expect(
      bindCapturedArtifact(JSON.stringify(event), row.owner, 30402, event.id).ok
    ).toBe(false);
  } finally {
    key.fill(0);
  }
});
