import { describe, expect, it } from 'vitest';
import { finalizeEvent, generateSecretKey } from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import {
  decodePrivateRecord,
  inspectPrivateSendRecord,
  privateRecordSnapshot,
  privateRecordWire,
  privateReceivedOuterMetadata,
  type PrivateAccessProbe,
  type PrivateAccessProbeReceipt
} from '../../src/lib/persistence/private-records.ts';
import { privateRecordSettlement } from '../../src/lib/persistence/local-inventory.ts';
import { privateSendStatus } from '../../src/lib/messaging/send-status.ts';

const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
const peer = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
const command = '12345678-1234-4234-8234-123456789abc';
const readOrigin = 'wss://read.example.org';
const archiveOrigin = 'wss://archive.example.org';
function policy() {
  return {
    schemaVersion: 1,
    public: [
      {
        origin: 'wss://public.example.org',
        read: true,
        write: false,
        nip50: false
      }
    ],
    inbox: [
      { origin: readOrigin, read: true, write: false },
      { origin: archiveOrigin, read: false, write: true }
    ],
    postingEnabled: false,
    messagingEnabled: true,
    operatorDenylist: []
  };
}
function operator(policyWire = JSON.stringify(policy())) {
  return {
    schemaVersion: 1,
    revision: 1,
    authority: 'source_fixture',
    policyWire,
    evidenceSha256: 'a'.repeat(64),
    relays: [readOrigin, archiveOrigin].map((origin) => ({
      origin,
      auth: 'not_required',
      recipientRead: 'supported',
      namedArchiveAck: 'supported',
      exactReadback: 'supported',
      retention: 'operator_defined'
    }))
  };
}
// Genuine outer signatures/ciphertext structure only; this fixture does not
// prove nested probe contents, operator facts or an actual access workflow.
function artifact(destination = owner) {
  const key = generateSecretKey();
  let conversation: Uint8Array | undefined;
  try {
    conversation = nip44.v2.utils.getConversationKey(key, destination);
    const event = finalizeEvent(
      {
        kind: 1059,
        created_at: 1700000000,
        tags: [['p', destination]],
        content: nip44.v2.encrypt(
          'Structural codec fixture; no nonce proof',
          conversation
        )
      },
      key
    );
    return { eventId: event.id, wire: JSON.stringify(event) };
  } finally {
    conversation?.fill(0);
    key.fill(0);
  }
}
function reservation(): PrivateAccessProbe {
  const policyWire = JSON.stringify(policy());
  return {
    schema: 1,
    family: 'private_access_probe',
    owner,
    id: command,
    revision: 0,
    rumorHash: 'b'.repeat(64),
    createdAt: 1700000000,
    binding: {
      preferenceId: 'c'.repeat(64),
      preferenceCreatedAt: 1699999999,
      readTargets: [readOrigin],
      archiveTargets: [archiveOrigin],
      policyWire,
      operatorWire: JSON.stringify(operator(policyWire))
    },
    self: null,
    receipts: []
  };
}
function prepared(): PrivateAccessProbe {
  return { ...reservation(), revision: 1, self: artifact() };
}
function fact(
  row: PrivateAccessProbe,
  status: PrivateAccessProbeReceipt['status'] = 'accepted'
): PrivateAccessProbeReceipt {
  const readback = status === 'readback' || status === 'nonce_verified';
  return {
    actionId: command,
    origin: readback ? readOrigin : archiveOrigin,
    role: readback ? 'access_readback' : 'access_archive',
    attempt: 1,
    eventId: row.self!.eventId,
    status,
    observedAtMilliseconds: 1700000000000,
    readbackWire: readback ? row.self!.wire : null
  };
}
const decode = (row: unknown, expectedOwner = owner, expectedId = command) =>
  decodePrivateRecord(JSON.stringify(row), expectedOwner, expectedId);

describe('CR008 S2 strict separate probe journal codec', () => {
  it('retains reservation and own ciphertext phases with no plaintext or permission', () => {
    for (const row of [reservation(), prepared()]) {
      const result = decode(row);
      expect(result.ok).toBe(true);
      if (!result.ok) throw Error('valid structural probe rejected');
      expect(privateRecordSnapshot(result.value, owner, command)).toEqual(row);
      expect(privateRecordWire(result.value, owner, command)).toBe(
        JSON.stringify(row)
      );
      expect(
        privateRecordSnapshot(result.value, peer, command)
      ).toBeUndefined();
      expect(
        privateReceivedOuterMetadata(result.value, owner, command)
      ).toBeUndefined();
      expect(
        inspectPrivateSendRecord(JSON.stringify(row), owner, command)
      ).toBeUndefined();
      expect(privateSendStatus(result.value, owner, command)).toBeUndefined();
      expect(privateRecordSettlement(row)).toBe('protected');
    }
  });
  it('admits exact local archive/readback facts without granting readiness or settled cleanup', () => {
    const row = prepared();
    for (const status of [
      'accepted',
      'refused',
      'timed_out',
      'unknown',
      'stopped',
      'readback',
      'nonce_verified'
    ] as const) {
      const next = { ...row, revision: 2, receipts: [fact(row, status)] };
      expect(decode(next).ok).toBe(true);
      expect(privateRecordSettlement(next)).toBe('protected');
    }
    const completeLooking = {
      ...row,
      revision: 4,
      receipts: [fact(row), fact(row, 'readback'), fact(row, 'nonce_verified')]
    };
    expect(decode(completeLooking).ok).toBe(true);
    expect(privateRecordSettlement(completeLooking)).toBe('protected');
    expect(decode({ ...completeLooking, readiness: true }).ok).toBe(false);
  });
  it('preserves explicit unsupported capability metadata without converting it into success', () => {
    const row = reservation(),
      declaration = operator();
    declaration.relays[0].exactReadback = 'unsupported';
    expect(
      decode({
        ...row,
        binding: { ...row.binding, operatorWire: JSON.stringify(declaration) }
      }).ok
    ).toBe(true);
  });
  it('requires separate read/write grants, configured declarations and their exact policy wire', () => {
    const row = reservation();
    for (const patch of [
      { readTargets: [] },
      { archiveTargets: [] },
      { readTargets: [archiveOrigin] },
      { archiveTargets: [readOrigin] },
      { readTargets: [readOrigin, readOrigin] },
      { readTargets: [readOrigin, 'wss://outside.example.org'] },
      { archiveTargets: [archiveOrigin + '/'] },
      { readTargets: [readOrigin, readOrigin, readOrigin, readOrigin] },
      { preferenceId: 'bad' },
      { preferenceCreatedAt: -1 },
      { preferenceCreatedAt: 1.1 },
      { policyWire: '{}' },
      { operatorWire: '{}' }
    ])
      expect(decode({ ...row, binding: { ...row.binding, ...patch } }).ok).toBe(
        false
      );
    for (const declaration of [
      {
        ...operator(),
        authority: 'unconfigured',
        evidenceSha256: null,
        relays: []
      },
      { ...operator(), relays: [operator().relays[0]] },
      {
        ...operator(),
        policyWire: JSON.stringify({ ...policy(), messagingEnabled: false })
      }
    ])
      expect(
        decode({
          ...row,
          binding: { ...row.binding, operatorWire: JSON.stringify(declaration) }
        }).ok
      ).toBe(false);
  });
  it('rejects owner/id/schema/revision violations and phases lacking immutable ciphertext', () => {
    const row = prepared();
    for (const patch of [
      { schema: 2 },
      { family: 'private_send_operation' },
      { owner: peer },
      { id: row.rumorHash },
      { revision: -1 },
      { revision: 1.1 },
      { revision: Number.MAX_SAFE_INTEGER + 1 },
      { rumorHash: 'bad' },
      { createdAt: -1 },
      { createdAt: 1.5 },
      { revision: 0 },
      { self: null }
    ])
      expect(decode({ ...row, ...patch }).ok).toBe(false);
    expect(decode(row, peer).ok).toBe(false);
    expect(decode(row, owner, row.rumorHash).ok).toBe(false);
    expect(decode({ ...reservation(), receipts: [fact(row)] }).ok).toBe(false);
  });
  it('requires independently signed strict own1059 and preserves full original bytes', () => {
    const row = prepared(),
      original = JSON.parse(row.self!.wire) as Record<string, unknown>;
    for (const patch of [
      { kind: 14 },
      { kind: 13 },
      { sig: '0'.repeat(128) },
      { content: 'PRIVATE_PLAINTEXT_SENTINEL' },
      {
        tags: [
          ['p', owner],
          ['nonce', '1', '1']
        ]
      },
      { tags: [['p', peer]] },
      { extra: true }
    ])
      expect(
        decode({
          ...row,
          self: { ...row.self, wire: JSON.stringify({ ...original, ...patch }) }
        }).ok
      ).toBe(false);
    expect(decode({ ...row, self: artifact(peer) }).ok).toBe(false);
    expect(
      decode({ ...row, self: { ...row.self, eventId: 'd'.repeat(64) } }).ok
    ).toBe(false);
    expect(
      decode({
        ...row,
        self: { ...row.self, wire: row.self!.wire + ' '.repeat(32768) }
      }).ok
    ).toBe(false);
    const whitespace = {
        ...row,
        self: { ...row.self!, wire: row.self!.wire + '\n\t' }
      },
      result = decode(whitespace);
    expect(result.ok).toBe(true);
    if (!result.ok) throw Error('valid exact wire rejected');
    expect(privateRecordSnapshot(result.value, owner, command)).toEqual(
      whitespace
    );
  });
  it('rejects relabelled ordinary roles, wrong targets/ids and non-exact readbacks', () => {
    const row = prepared();
    for (const patch of [
      { actionId: 'bad' },
      { role: 'peer' },
      { role: 'self_archive' },
      { origin: readOrigin },
      { attempt: 0 },
      { attempt: 4 },
      { attempt: 1.1 },
      { eventId: 'd'.repeat(64) },
      { status: 'readback' },
      { observedAtMilliseconds: -1 },
      { readbackWire: row.self!.wire }
    ])
      expect(
        decode({ ...row, revision: 2, receipts: [{ ...fact(row), ...patch }] })
          .ok
      ).toBe(false);
    for (const patch of [
      { origin: archiveOrigin },
      { readbackWire: null },
      { readbackWire: row.self!.wire + '\n' },
      { status: 'accepted' },
      { status: 'nonce_verified', readbackWire: artifact().wire }
    ])
      expect(
        decode({
          ...row,
          revision: 2,
          receipts: [{ ...fact(row, 'readback'), ...patch }]
        }).ok
      ).toBe(false);
  });
  it('rejects plaintext, unknown/missing fields and duplicate aliases at every nested level', () => {
    const row = prepared();
    for (const key of [
      'body',
      'nonce',
      'subject',
      'peer',
      'session',
      'verified'
    ]) {
      expect(decode({ ...row, [key]: 'PRIVATE_SENTINEL' }).ok).toBe(false);
      expect(
        decode({
          ...row,
          binding: { ...row.binding, [key]: 'PRIVATE_SENTINEL' }
        }).ok
      ).toBe(false);
    }
    for (const key of Object.keys(row)) {
      const broken: Record<string, unknown> = { ...row };
      delete broken[key];
      expect(decode(broken).ok).toBe(false);
    }
    for (const key of Object.keys(row.binding)) {
      const broken: Record<string, unknown> = { ...row.binding };
      delete broken[key];
      expect(decode({ ...row, binding: broken }).ok).toBe(false);
    }
    const raw = JSON.stringify(row);
    for (const duplicate of [
      raw.replace('"schema":1', '"schema":2,"schema":1'),
      raw.replace(
        '"preferenceId":',
        '"\\u0070referenceId":"d","preferenceId":'
      ),
      raw.replace('"eventId":', '"eventId":"d","eventId":')
    ])
      expect(decodePrivateRecord(duplicate, owner, command).ok).toBe(false);
    for (const operatorWire of [
      JSON.stringify(operator()).replace(
        '"revision":1',
        '"revision":2,"revision":1'
      ),
      JSON.stringify(operator(), null, 2)
    ])
      expect(
        decode({ ...row, binding: { ...row.binding, operatorWire } }).ok
      ).toBe(false);
  });
});
