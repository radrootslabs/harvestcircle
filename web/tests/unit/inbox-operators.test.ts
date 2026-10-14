import { describe, expect, it } from 'vitest';
import {
  inboxOperatorQualificationSnapshot,
  inboxOperatorQualificationWire,
  validateInboxOperatorQualification,
  type InboxOperatorQualification
} from '../../src/lib/config/inbox-operators.ts';
import { deploymentInboxOperatorQualification } from '../../src/lib/config/deployment-inbox-operators.ts';
import { deploymentRelayPolicy } from '../../src/lib/config/deployment-relays.ts';
import {
  readRelayPolicy,
  validateRelayPolicy,
  type RelayPolicy
} from '../../src/lib/config/relays.ts';

function policyInput() {
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
    inbox: [{ origin: 'wss://inbox.example.org', read: true, write: true }],
    postingEnabled: false,
    messagingEnabled: true,
    operatorDenylist: []
  };
}
function input() {
  return {
    schemaVersion: 1,
    revision: 1,
    authority: 'source_fixture',
    policyWire: JSON.stringify(policyInput()),
    evidenceSha256: 'a'.repeat(64),
    relays: [
      {
        origin: 'wss://inbox.example.org',
        auth: 'not_required',
        recipientRead: 'supported',
        namedArchiveAck: 'supported',
        exactReadback: 'supported',
        retention: 'operator_defined'
      }
    ]
  };
}
const admit = (value: unknown) =>
  validateInboxOperatorQualification(JSON.stringify(value));
const genuinePolicy = () => validateRelayPolicy(JSON.stringify(policyInput()))!;

describe('CR008 S1 source-owned operator declaration boundaries', () => {
  it('keeps deployment unconfigured and bound to the original disabled policy', () => {
    expect(
      inboxOperatorQualificationSnapshot(deploymentInboxOperatorQualification)
    ).toEqual({
      schemaVersion: 1,
      revision: 1,
      authority: 'unconfigured',
      policyWire: JSON.stringify(readRelayPolicy(deploymentRelayPolicy)),
      evidenceSha256: null,
      relays: []
    });
    expect(
      inboxOperatorQualificationWire(
        deploymentInboxOperatorQualification,
        deploymentRelayPolicy
      )
    ).toBeDefined();
    expect(
      inboxOperatorQualificationWire(
        deploymentInboxOperatorQualification,
        genuinePolicy()
      )
    ).toBeUndefined();
  });
  it('retains the exact full declaration and separate fixture/operator authority, without readiness', () => {
    for (const authority of ['source_fixture', 'operator']) {
      const value = { ...input(), authority },
        token = admit(value)!;
      expect(inboxOperatorQualificationSnapshot(token)).toEqual(value);
      expect(inboxOperatorQualificationWire(token, genuinePolicy())).toBe(
        JSON.stringify(value)
      );
      expect(Object.isFrozen(token)).toBe(true);
      expect(Object.keys(token)).toEqual([]);
      expect(inboxOperatorQualificationSnapshot(token)).not.toHaveProperty(
        'readiness'
      );
    }
    expect(admit({ ...input(), readiness: true })).toBeUndefined();
  });
  it('retains unsupported declarations and explicit AUTH requirements as metadata', () => {
    for (const auth of ['not_required', 'explicit_required', 'unsupported']) {
      const value = input();
      value.relays[0] = {
        ...value.relays[0],
        auth,
        recipientRead: 'unsupported',
        namedArchiveAck: 'unsupported',
        exactReadback: 'unsupported',
        retention: 'unsupported'
      };
      expect(inboxOperatorQualificationSnapshot(admit(value)!)).toEqual(value);
    }
  });
  it('cannot mutate retained origins, capabilities, evidence or authority using detached views', () => {
    const value = input(),
      token = admit(value)!,
      original = JSON.stringify(value);
    value.relays[0].origin = 'wss://outside.example.org';
    value.evidenceSha256 = 'b'.repeat(64);
    const view = inboxOperatorQualificationSnapshot(
      token
    )! as unknown as ReturnType<typeof input>;
    view.authority = 'operator';
    view.relays[0].auth = 'unsupported';
    view.relays.push({
      ...view.relays[0],
      origin: 'wss://outside.example.org'
    });
    view.policyWire = '{}';
    expect(inboxOperatorQualificationWire(token, genuinePolicy())).toBe(
      original
    );
    expect(inboxOperatorQualificationSnapshot(token)?.authority).toBe(
      'source_fixture'
    );
    expect(inboxOperatorQualificationSnapshot(token)?.relays).toHaveLength(1);
    const forged = { ...token } as InboxOperatorQualification;
    expect(inboxOperatorQualificationSnapshot(forged)).toBeUndefined();
    expect(
      inboxOperatorQualificationWire(forged, genuinePolicy())
    ).toBeUndefined();
    expect(
      inboxOperatorQualificationWire(token, {} as RelayPolicy)
    ).toBeUndefined();
  });
  it('invalidates binding for every changed policy dimension, even when the selected origin survives', () => {
    const token = admit(input())!;
    for (const patch of [
      { messagingEnabled: false },
      { postingEnabled: true },
      { inbox: [{ ...policyInput().inbox[0], write: false }] },
      { public: [{ ...policyInput().public[0], write: true }] },
      {
        operatorDenylist: [
          '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
        ]
      }
    ]) {
      const changed = validateRelayPolicy(
        JSON.stringify({ ...policyInput(), ...patch })
      )!;
      expect(changed).toBeDefined();
      expect(inboxOperatorQualificationWire(token, changed)).toBeUndefined();
    }
  });
  it('retains at most three unique approved canonical inbox origins and never public-only hints', () => {
    const value = input(),
      origins = [1, 2, 3].map((n) => `wss://inbox${n}.example.org`);
    value.policyWire = JSON.stringify({
      ...policyInput(),
      inbox: origins.map((origin) => ({ origin, read: true, write: true }))
    });
    value.relays = origins.map((origin) => ({ ...value.relays[0], origin }));
    expect(admit(value)).toBeDefined();
    expect(
      admit({ ...value, relays: [...value.relays, value.relays[0]] })
    ).toBeUndefined();
    expect(
      admit({ ...value, relays: [value.relays[0], value.relays[0]] })
    ).toBeUndefined();
    for (const origin of [
      'wss://public.example.org',
      'wss://outside.example.org',
      'wss://inbox.example.org/',
      'wss://inbox.example.org:443',
      'ws://inbox.example.org',
      'wss://127.0.0.1',
      'wss://user@inbox.example.org'
    ])
      expect(
        admit({ ...input(), relays: [{ ...input().relays[0], origin }] })
      ).toBeUndefined();
  });
  it('rejects missing/unknown root and relay fields, false booleans and malformed evidence without coercion', () => {
    const value = input();
    for (const patch of [
      { schemaVersion: 2 },
      { revision: 0 },
      { revision: -1 },
      { revision: 1.1 },
      { revision: Number.MAX_SAFE_INTEGER + 1 },
      { authority: 'approved' },
      { authority: true },
      { evidenceSha256: null },
      { evidenceSha256: 'A'.repeat(64) },
      { evidenceSha256: 'a'.repeat(63) },
      { evidenceSha256: true },
      { relays: [] },
      { relays: {} },
      { policyWire: {} },
      { policyWire: '{}' },
      { privateKey: 'SECRET_SENTINEL' }
    ])
      expect(admit({ ...value, ...patch })).toBeUndefined();
    for (const patch of [
      { auth: true },
      { recipientRead: true },
      { namedArchiveAck: false },
      { exactReadback: 'ready' },
      { retention: 'unlimited' },
      { credential: 'SECRET_SENTINEL' }
    ])
      expect(
        admit({ ...value, relays: [{ ...value.relays[0], ...patch }] })
      ).toBeUndefined();
    for (const key of Object.keys(value)) {
      const broken: Record<string, unknown> = { ...value };
      delete broken[key];
      expect(admit(broken)).toBeUndefined();
    }
    for (const key of Object.keys(value.relays[0])) {
      const broken: Record<string, unknown> = { ...value.relays[0] };
      delete broken[key];
      expect(admit({ ...value, relays: [broken] })).toBeUndefined();
    }
    expect(
      admit({
        ...value,
        authority: 'unconfigured',
        relays: [],
        evidenceSha256: null
      })
    ).toBeDefined();
    expect(
      admit({ ...value, authority: 'unconfigured', evidenceSha256: null })
    ).toBeUndefined();
    expect(
      admit({ ...value, authority: 'unconfigured', relays: [] })
    ).toBeUndefined();
  });
  it('rejects duplicate/escaped keys, whitespace and altered canonical numeric spelling in either JSON layer', () => {
    const raw = JSON.stringify(input());
    for (const malformed of [
      raw.replace('"revision":1', '"revision":2,"revision":1'),
      raw.replace('"authority":', '"\\u0061uthority":'),
      raw.replace(
        '"auth":"not_required"',
        '"auth":"unsupported","auth":"not_required"'
      ),
      raw.replace('"revision":1', '"revision":1e0'),
      raw + '\n',
      JSON.stringify(input(), null, 2)
    ])
      expect(validateInboxOperatorQualification(malformed)).toBeUndefined();
    for (const policyWire of [
      JSON.stringify(policyInput()).replace(
        '"messagingEnabled":true',
        '"messagingEnabled":false,"messagingEnabled":true'
      ),
      JSON.stringify(policyInput()).replace(
        '"schemaVersion"',
        '"\\u0073chemaVersion"'
      ),
      JSON.stringify(policyInput(), null, 2)
    ])
      expect(admit({ ...input(), policyWire })).toBeUndefined();
  });
  it('bounds raw UTF8 before parsing and does not execute caller getters or coercions', () => {
    for (const raw of [
      '{',
      'null',
      '[]',
      'false',
      ' '.repeat(65537),
      JSON.stringify({ extra: 'é'.repeat(33000) }),
      '"\ud800"'
    ])
      expect(validateInboxOperatorQualification(raw)).toBeUndefined();
    let reads = 0;
    const hostile = {
      get relays() {
        reads++;
        throw Error('getter executed');
      },
      toString() {
        reads++;
        return JSON.stringify(input());
      }
    };
    expect(validateInboxOperatorQualification(hostile)).toBeUndefined();
    expect(
      validateInboxOperatorQualification(Object.create(input()))
    ).toBeUndefined();
    expect(
      validateInboxOperatorQualification({
        ...input(),
        [Symbol('secret')]: true
      })
    ).toBeUndefined();
    expect(reads).toBe(0);
  });
});
