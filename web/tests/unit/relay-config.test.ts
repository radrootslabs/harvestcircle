import { describe, expect, it } from 'vitest';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import {
  canonicalRelayOrigin,
  validateRelayPolicy,
  readRelayPolicy,
  type RelayPolicy,
  requireLaunchRelayPolicy,
  publicRelayTargets,
  inboxRelayTargets
} from '../../src/lib/config/relays.ts';
import { deploymentRelayPolicy } from '../../src/lib/config/deployment-relays.ts';

const admit = (value: unknown) => validateRelayPolicy(JSON.stringify(value));
const publicRelay = (origin = 'wss://public.example.org') => ({
  origin,
  read: true,
  write: true,
  nip50: false
});
const inboxRelay = (origin = 'wss://inbox.example.org') => ({
  origin,
  read: true,
  write: true
});
const input = () => ({
  schemaVersion: 1,
  public: [publicRelay()],
  inbox: [inboxRelay()],
  postingEnabled: true,
  messagingEnabled: true,
  operatorDenylist: []
});

// Frozen IANA IPv6 Special-Purpose registry revision 2025-10-09, reviewed
// 2026-10-14: https://www.iana.org/assignments/iana-ipv6-special-registry/.
// RFC4291 defines 128-bit prefixes; RFC9637 reserves the full 3fff::/20.
// Conservative relay literals exclude special services even where IANA marks
// a specific service globally reachable. No DNS/connect/operator claim follows.
describe('CR006 complete binary IPv6 literal policy', () => {
  it.each([
    '2001::',
    '2001:1ff:ffff:ffff:ffff:ffff:ffff:ffff',
    '2001:1::1',
    '2001:1::2',
    '2001:1::3',
    '2001:2::1',
    '2001:3::1',
    '2001:4:112::1',
    '2001:10::1',
    '2001:1f:ffff:ffff:ffff:ffff:ffff:ffff',
    '2001:20::1',
    '2001:2f:ffff:ffff:ffff:ffff:ffff:ffff',
    '2001:30::1',
    '2001:3f:ffff:ffff:ffff:ffff:ffff:ffff',
    '2001:db8::',
    '2001:db8:ffff:ffff:ffff:ffff:ffff:ffff',
    '2002::',
    '2002:ffff:ffff:ffff:ffff:ffff:ffff:ffff',
    '2620:4f:8000::',
    '2620:4f:8000:ffff:ffff:ffff:ffff:ffff',
    '3fff::',
    '3fff:fff:ffff:ffff:ffff:ffff:ffff:ffff',
    '::',
    '::1',
    '::ffff:808:808',
    '64:ff9b::808:808',
    '64:ff9b:1::1',
    '100::1',
    '100:0:0:1::1',
    '5f00::1',
    'fc00::1',
    'fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff',
    'fe80::1',
    'febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff',
    'ff02::1',
    '4000::1'
  ])('excludes reviewed special-purpose literal %s', (address) => {
    expect(canonicalRelayOrigin(`wss://[${address}]`)).toBeUndefined();
  });
  it.each([
    '2000::',
    '2000:ffff:ffff:ffff:ffff:ffff:ffff:ffff',
    '2001:200::',
    '2001:db7:ffff:ffff:ffff:ffff:ffff:ffff',
    '2001:db9::',
    '2001:ffff:ffff:ffff:ffff:ffff:ffff:ffff',
    '2003::',
    '2620:4f:7fff:ffff:ffff:ffff:ffff:ffff',
    '2620:4f:8001::',
    '3ffe:ffff:ffff:ffff:ffff:ffff:ffff:ffff',
    '3fff:1000::',
    '3fff:ffff:ffff:ffff:ffff:ffff:ffff:ffff',
    '2606:4700:4700::1111',
    '2606:4700:4700:1:2:3:4:5',
    '2606:0:1:2:3:4:5:6',
    '2606::1:2:3:4:5'
  ])(
    'keeps ordinary public literal outside excluded prefixes %s',
    (address) => {
      const origin = `wss://[${address}]:8443`;
      expect(canonicalRelayOrigin(origin)).toBe(origin);
    }
  );
  it.each([
    'wss://[2606:4700:4700:0:0:0:0:1111]',
    'wss://[2606::1:2:3:4:5:6]',
    'wss://[2606:4700:4700::01111]',
    'wss://[2606::1::2]',
    'wss://[2606:4700:4700::1111%25eth0]',
    'wss://[2606:4700:4700::1111]:443',
    'wss://[2606:4700:4700::1111]:0',
    'wss://[2606:4700:4700::1111]:65536',
    'wss://[2606:4700:4700::1111]/',
    'wss://[2606:4700:4700::1111]/path',
    'wss://user@[2606:4700:4700::1111]',
    'wss://[2606:4700:4700::1111]?q=x',
    'wss://[2606:4700:4700::1111]#fragment'
  ])('retains canonical URL admission before binary policy %s', (origin) => {
    expect(canonicalRelayOrigin(origin)).toBeUndefined();
  });
});

describe('fixed relay policy', () => {
  it('admits canonical WSS origins and explicit optional NIP50 capability', () => {
    expect(canonicalRelayOrigin('wss://relay.example.org:8443')).toBe(
      'wss://relay.example.org:8443'
    );
    const value = input();
    value.public[0].nip50 = true;
    expect(readRelayPolicy(admit(value)!).public[0].nip50).toBe(true);
  });
  it.each([
    '',
    'ws://relay.example.org',
    'https://relay.example.org',
    '//relay.example.org',
    ' wss://relay.example.org',
    'wss://relay.example.org\n',
    'wss://relay.example.org\\x',
    'wss://user:password@relay.example.org',
    'wss://@relay.example.org',
    'wss://relay.example.org/path',
    'wss://relay.example.org?token=private',
    'wss://relay.example.org#fragment',
    'WSS://RELAY.EXAMPLE.ORG',
    'wss://relay.example.org/',
    'wss://relay.example.org:443',
    'wss://relay.example.org:0',
    'wss://relay.example.org:65536',
    'wss://relay%2eexample.org',
    'wss://relay.example.org\u0000',
    'wss://localhost',
    'wss://sub.localhost',
    'wss://relay.local',
    'wss://relay.internal',
    'wss://home.arpa',
    'wss://relay.home.arpa',
    'wss://relay.test',
    'wss://127.0.0.1',
    'wss://127.1',
    'wss://2130706433',
    'wss://0x7f000001',
    'wss://10.1.2.3',
    'wss://172.16.0.1',
    'wss://192.168.0.1',
    'wss://169.254.169.254',
    'wss://100.64.0.1',
    'wss://0.0.0.0',
    'wss://224.0.0.1',
    'wss://[::1]',
    'wss://[::]',
    'wss://[fc00::1]',
    'wss://[fe80::1]',
    'wss://[::ffff:127.0.0.1]',
    'wss://[ff02::1]',
    'wss://singlelabel',
    'wss://relay.example.org.',
    'wss://-bad.example.org',
    'wss://relay.example.org\ud800',
    'wss://' + 'a'.repeat(8193)
  ])('rejects malformed, credential or private-network origin %s', (origin) => {
    expect(canonicalRelayOrigin(origin)).toBeUndefined();
    expect(
      admit({ ...input(), public: [publicRelay(origin)] })
    ).toBeUndefined();
  });
  it('admits public address literals without claiming DNS or connect-time enforcement', () => {
    expect(canonicalRelayOrigin('wss://8.8.8.8')).toBe('wss://8.8.8.8');
    expect(canonicalRelayOrigin('wss://[2606:4700:4700::1111]')).toBe(
      'wss://[2606:4700:4700::1111]'
    );
  });
  it('caps each role independently at three, permits the same origin in separate roles', () => {
    const value = input();
    value.public = [1, 2, 3].map((n) =>
      publicRelay(`wss://public${n}.example.org`)
    );
    value.inbox = value.public.map(({ origin }) => inboxRelay(origin));
    expect(readRelayPolicy(admit(value)!).inbox).toHaveLength(3);
    expect(
      admit({
        ...value,
        public: [...value.public, publicRelay()]
      })
    ).toBeUndefined();
    expect(
      admit({ ...value, inbox: [...value.inbox, inboxRelay()] })
    ).toBeUndefined();
    expect(
      admit({
        ...input(),
        public: [publicRelay(), publicRelay()]
      })
    ).toBeUndefined();
    expect(
      admit({ ...input(), inbox: [inboxRelay(), inboxRelay()] })
    ).toBeUndefined();
  });
  it('rejects unknown fields and wrong schema, array and flag types without echoing secrets', () => {
    for (const value of [
      null,
      [],
      { ...input(), schemaVersion: 2 },
      { ...input(), token: 'private' },
      { ...input(), postingEnabled: 1 },
      { ...input(), messagingEnabled: 'true' },
      { ...input(), public: {} },
      { ...input(), operatorDenylist: ['private'] },
      { ...input(), public: [{ ...publicRelay(), auth: true }] },
      { ...input(), inbox: [{ ...inboxRelay(), nip50: false }] },
      { ...input(), public: [{ ...publicRelay(), read: false, nip50: true }] },
      { ...input(), public: [{ ...publicRelay(), read: false, write: false }] }
    ])
      expect(admit(value)).toBeUndefined();
  });
  it('validates denylist keys and preserves an immutable token with detached views', () => {
    const key =
      '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
    const value = { ...input(), operatorDenylist: [key] };
    const admitted = admit(value)!;
    value.public[0].origin = 'wss://outside.example.org';
    value.operatorDenylist.push('bad');
    expect(readRelayPolicy(admitted).public[0].origin).toBe(
      'wss://public.example.org'
    );
    expect(readRelayPolicy(admitted).operatorDenylist).toEqual([key]);
    expect(Object.isFrozen(admitted)).toBe(true);

    expect(admit({ ...input(), operatorDenylist: [key, key] })).toBeUndefined();
    expect(
      admit({ ...input(), operatorDenylist: ['f'.repeat(64)] })
    ).toBeUndefined();
  });
  it('rejects inherited fields, symbols and accessors without invoking getters', () => {
    let reads = 0;
    const getter = { ...input() };
    Object.defineProperty(getter, 'public', {
      get: () => {
        reads++;
        return [];
      },
      enumerable: true
    });
    expect(validateRelayPolicy(getter)).toBeUndefined();
    expect(reads).toBe(0);
    expect(validateRelayPolicy(Object.create(input()))).toBeUndefined();
    expect(
      validateRelayPolicy({ ...input(), [Symbol('credential')]: 'private' })
    ).toBeUndefined();
  });
  it('ignores public hints and intersects inbox targets with approved recipient preferences', () => {
    const policy = admit(input())!;
    expect(publicRelayTargets(policy, 'read')).toEqual([
      'wss://public.example.org'
    ]);
    expect(
      inboxRelayTargets(
        policy,
        [
          'wss://outside.example.org',
          'wss://inbox.example.org',
          'wss://inbox.example.org'
        ],
        'write'
      )
    ).toEqual(['wss://inbox.example.org']);
    expect(
      inboxRelayTargets(policy, ['wss://outside.example.org'], 'write')
    ).toEqual([]);
    expect(inboxRelayTargets(policy, [], 'read')).toEqual([]);
    const readOnly = admit({
      ...input(),
      inbox: [{ ...inboxRelay(), write: false }]
    })!;
    expect(
      inboxRelayTargets(readOnly, ['wss://inbox.example.org'], 'write')
    ).toEqual([]);
  });
  it('keeps separate effect flags disabled and production inputs empty until approved', () => {
    expect(readRelayPolicy(deploymentRelayPolicy).public).toEqual([]);
    expect(readRelayPolicy(deploymentRelayPolicy).inbox).toEqual([]);
    expect(readRelayPolicy(deploymentRelayPolicy).postingEnabled).toBe(false);
    expect(readRelayPolicy(deploymentRelayPolicy).messagingEnabled).toBe(false);
    expect(() => requireLaunchRelayPolicy(deploymentRelayPolicy)).toThrow(
      'relay_launch_inputs_missing'
    );
    expect(requireLaunchRelayPolicy(admit(input())!)).toBeDefined();
    expect(() => requireLaunchRelayPolicy({} as RelayPolicy)).toThrow(
      'relay_policy_invalid'
    );
    expect(
      admit({ ...input(), public: [publicRelay('wss://127.0.0.1')] })
    ).toBeUndefined();
  });
  it('bounds JSON admission and rejects malformed or non-JSON inputs', () => {
    for (const raw of [
      '{',
      'null',
      '[]',
      '"policy"',
      ' '.repeat(65537),
      JSON.stringify({ ...input(), operatorDenylist: ['é'.repeat(33000)] })
    ])
      expect(validateRelayPolicy(raw)).toBeUndefined();
    expect(validateRelayPolicy(input())).toBeUndefined();
  });
  it('keeps retained egress unchanged after detached manifest and target mutations', () => {
    const token = admit(input())!;
    const view = readRelayPolicy(token);
    (view.public as unknown as { origin: string }[])[0].origin =
      'wss://outside.example.org';
    (view.operatorDenylist as string[]).push('changed');
    const targets = publicRelayTargets(token, 'read') as string[];
    targets.push('wss://outside.example.org');
    expect(publicRelayTargets(token, 'read')).toEqual([
      'wss://public.example.org'
    ]);
    expect(readRelayPolicy(token).operatorDenylist).toEqual([]);
    expect(() => publicRelayTargets({} as RelayPolicy, 'read')).toThrow(
      'relay_policy_invalid'
    );
  });
  it('preserves separate posting and messaging deployment flags', () => {
    const token = admit({
      ...input(),
      postingEnabled: false,
      messagingEnabled: true
    })!;
    expect(readRelayPolicy(token).postingEnabled).toBe(false);
    expect(readRelayPolicy(token).messagingEnabled).toBe(true);
  });
  it('rejects inactive or secret-bearing inbox role data', () => {
    expect(
      admit({
        ...input(),
        inbox: [{ ...inboxRelay(), read: false, write: false }]
      })
    ).toBeUndefined();
    expect(
      admit({ ...input(), inbox: [{ ...inboxRelay(), credential: 'private' }] })
    ).toBeUndefined();
    expect(
      admit({ ...input(), public: [{ ...publicRelay(), write: 'true' }] })
    ).toBeUndefined();
  });
  it('actually fails the explicit launch preparation command on missing deployed inputs', () => {
    const result = spawnSync(
      process.execPath,
      ['tools/check-launch-relays.mjs'],
      {
        cwd: fileURLToPath(new URL('../../', import.meta.url)),
        encoding: 'utf8',
        timeout: 86400000
      }
    );
    expect(result.error).toBeUndefined();
    expect(result.status).toBe(1);
    expect(result.stderr).toContain('relay_launch_inputs_missing');
    expect(result.stdout).not.toContain('PASS');
  });
});
