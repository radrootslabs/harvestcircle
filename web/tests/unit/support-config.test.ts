import { describe, expect, it } from 'vitest';
import {
  validateSupportPolicy,
  readSupportPolicy,
  requireLaunchSupportPolicy,
  type SupportPolicy
} from '../../src/lib/config/support.ts';

// Parser-only, explicitly controlled values. No destination is contacted or
// presented as the actual deployment's approved support owner/destination.
const fixture = {
  schemaVersion: 1,
  owner: 'Controlled fixture operator',
  destination: 'mailto:reports@operator.org'
};
const raw = (fields: Record<string, unknown> = {}) =>
  JSON.stringify({ ...fixture, ...fields });

describe('HCP124 original bounded deployment support configuration', () => {
  it.each([
    '3fff::1',
    '3fff:fff:ffff:ffff:ffff:ffff:ffff:ffff',
    '2001:db8::1',
    '2001:1f::1',
    '2001:3f::1',
    '2002::1',
    '2620:4f:8000::1',
    '::ffff:808:808',
    'fc00::1'
  ])('CR006 rejects special IPv6 support destination %s', (address) => {
    expect(
      validateSupportPolicy(raw({ destination: `https://[${address}]/report` }))
    ).toBeUndefined();
  });
  it.each(['2606:4700:4700::1111', '3fff:1000::1', '2001:200::1'])(
    'CR006 preserves canonical public IPv6 HTTPS support destination %s',
    (address) => {
      const href = `https://[${address}]:8443/report`;
      const policy = validateSupportPolicy(raw({ destination: href }));
      expect(policy).toBeDefined();
      expect(policy && readSupportPolicy(policy).destination).toBe(href);
    }
  );
  it('admits the exact inert fixture and returns a detached snapshot', () => {
    const policy = validateSupportPolicy(raw());
    expect(policy).toBeDefined();
    if (!policy) throw Error('fixture admission');
    expect(readSupportPolicy(policy)).toEqual(fixture);
    const detached = readSupportPolicy(policy) as { owner: string };
    detached.owner = 'changed copy';
    expect(readSupportPolicy(policy).owner).toBe(fixture.owner);
    expect(requireLaunchSupportPolicy(policy)).toBe(policy);
  });
  it('empty development inputs are explicit and refuse launch preparation', () => {
    const policy = validateSupportPolicy(raw({ owner: '', destination: '' }));
    expect(policy).toBeDefined();
    if (!policy) throw Error('development policy');
    expect(() => requireLaunchSupportPolicy(policy)).toThrow(
      'support_launch_inputs_missing'
    );
  });
  it('accepts a reviewed HTTPS destination without acquiring transport', () => {
    const policy = validateSupportPolicy(
      raw({ destination: 'https://support.operator.org/report' })
    );
    expect(policy).toBeDefined();
    if (!policy) throw Error('fixture HTTPS admission');
    expect(readSupportPolicy(policy).destination).toBe(
      'https://support.operator.org/report'
    );
  });
  it.each([
    ['malformed JSON', '{'],
    ['unknown field', raw({ transcript: 'not allowed' })],
    ['wrong version', raw({ schemaVersion: 2 })],
    ['wrong owner type', raw({ owner: 42 })],
    ['wrong destination type', raw({ destination: [] })],
    ['owner control', raw({ owner: 'fixture\nforged owner' })],
    ['oversized owner', raw({ owner: 'x'.repeat(257) })],
    ['plaintext HTTP', raw({ destination: 'http://support.operator.org/' })],
    ['script scheme', raw({ destination: 'javascript:alert(1)' })],
    ['data carrier', raw({ destination: 'data:text/plain,report' })],
    ['relative route', raw({ destination: '/about#report' })],
    [
      'credentials',
      raw({ destination: 'https://secret@support.operator.org/' })
    ],
    [
      'mailto headers',
      raw({ destination: 'mailto:reports@operator.org?body=PRIVATE' })
    ],
    [
      'encoded header control',
      raw({ destination: 'mailto:reports%0d%0a@operator.org' })
    ],
    [
      'multiple mail recipients',
      raw({ destination: 'mailto:first@operator.org,second@operator.org' })
    ],
    [
      'HTTPS context query',
      raw({ destination: 'https://support.operator.org/report?body=PRIVATE' })
    ],
    [
      'HTTPS context fragment',
      raw({ destination: 'https://support.operator.org/report#PRIVATE' })
    ],
    ['oversized configuration', ' '.repeat(16385)],
    ['ill formed text', raw({ owner: '\ud800' })]
  ])(
    'rejects %s without making malformed configuration authoritative',
    (_label, value) => {
      expect(validateSupportPolicy(value)).toBeUndefined();
    }
  );
  it('does not evaluate caller getters or admit copied policy objects', () => {
    let reads = 0;
    const caller = {
      get owner() {
        reads++;
        return fixture.owner;
      },
      destination: fixture.destination,
      schemaVersion: 1
    };
    expect(validateSupportPolicy(caller)).toBeUndefined();
    expect(reads).toBe(0);
    expect(() => readSupportPolicy(caller as unknown as SupportPolicy)).toThrow(
      'support_policy_invalid'
    );
  });
});
