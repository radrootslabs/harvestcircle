import { afterEach, expect, it, vi } from 'vitest';
import { getEventHash, type UnsignedEvent } from 'applesauce-core/helpers';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import type { RelayPolicy } from '../../src/lib/config/relays.ts';
import type { InboxOperatorQualification } from '../../src/lib/config/inbox-operators.ts';
import type { InboxAccessController } from '../../src/lib/messaging/inbox-access-controller.ts';
import type {
  InboxProbeRecoveryReader,
  RecoveredInboxProbe,
  InboxProbeNonceProof
} from '../../src/lib/nostr/self-recovery-reader.ts';
import { inboxProbeRumorMatches } from '../../src/lib/nostr/inbox-access.ts';

afterEach(() => vi.unstubAllGlobals());
const owner =
    '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
  time = 1700000000;
const event: UnsignedEvent = {
  pubkey: owner,
  created_at: time,
  kind: 14,
  tags: [['p', owner]],
  content: 'Harvestcircle private inbox access check v1: ' + 'b'.repeat(64)
};
const hash = getEventHash(event),
  raw = JSON.stringify({ id: hash, ...event });
it('the canonical pure diagnostic validator admits only exact original own hash/time/nonce', () => {
  expect(inboxProbeRumorMatches(raw, owner, hash, time)).toBe(true);
  expect(inboxProbeRumorMatches(raw, owner.toUpperCase(), hash, time)).toBe(
    false
  );
  for (const malformed of [
    undefined,
    null,
    false,
    {},
    raw + ' ',
    '{',
    raw.replace('"kind":14', '"kind":1.4e1'),
    raw.replace('"kind":14', '"kind":14,"kind":14'),
    raw.replace('"id":', '"extra":0,"id":'),
    raw.replace('"content":', '"\\u0063ontent":')
  ])
    expect(inboxProbeRumorMatches(malformed, owner, hash, time)).toBe(false);
  for (const change of [
    { kind: 1 },
    { pubkey: 'c'.repeat(64) },
    { created_at: time + 1 },
    { tags: [] },
    { tags: [['p', 'c'.repeat(64)]] },
    {
      tags: [
        ['p', owner],
        ['p', owner]
      ]
    },
    { content: 'ordinary message' },
    { content: event.content + 'b' },
    { content: event.content.slice(0, -1) + 'B' },
    { content: 'x'.repeat(8193) }
  ]) {
    const altered = { ...event, ...change };
    const id = getEventHash(altered);
    expect(
      inboxProbeRumorMatches(
        JSON.stringify({ id, ...altered }),
        owner,
        id,
        time
      )
    ).toBe(false);
  }
  for (const bad of [
    -1,
    1.5,
    Number.MAX_SAFE_INTEGER + 1,
    NaN,
    Infinity,
    String(time),
    null
  ])
    expect(inboxProbeRumorMatches(raw, owner, hash, bad)).toBe(false);
  for (const bad of ['A'.repeat(64), 'c'.repeat(64), 'a'.repeat(63), null])
    expect(inboxProbeRumorMatches(raw, owner, bad, time)).toBe(false);
});
for (const browser of [false, true])
  it(`cold ${browser ? 'browser-shaped' : 'SSR'} recovery rejects fabricated families before effects`, async () => {
    vi.resetModules();
    let effects = 0;
    const trap = () => {
      effects++;
      throw Error('unadmitted effect');
    };
    if (browser) {
      vi.stubGlobal('window', {
        get nostr() {
          return trap();
        }
      });
      vi.stubGlobal('navigator', {
        get locks() {
          return trap();
        }
      });
      vi.stubGlobal('WebSocket', trap);
      vi.stubGlobal('indexedDB', { open: trap });
    }
    const m = await import('../../src/lib/nostr/self-recovery-reader.ts');
    const repository = {} as PrivateStorageRepository,
      identity = {} as IdentitySession,
      session = {} as PrivateSession,
      policy = {} as RelayPolicy,
      operator = {} as InboxOperatorQualification;
    for (const fake of [
      undefined,
      null,
      0,
      false,
      '',
      Symbol(),
      {},
      { current: () => true, nonce_verified: true },
      { status: 'recovered', proof: {} }
    ]) {
      expect(
        m.captureInboxProbeRecoveryReader(
          repository,
          identity,
          session,
          policy,
          fake as InboxAccessController,
          operator,
          '11111111-1111-4111-8111-111111111111',
          'reviewed_inbox_probe_recovery'
        )
      ).toBeUndefined();
      expect(
        await m.recoverInboxProbe(fake as InboxProbeRecoveryReader)
      ).toEqual({ status: 'invalid' });
      expect(
        m.recoveredInboxProbeBinding(
          fake as RecoveredInboxProbe,
          repository,
          session,
          fake as InboxAccessController,
          policy,
          operator
        )
      ).toBeUndefined();
      expect(
        m.recoveredInboxProbeRumorWire(
          fake as RecoveredInboxProbe,
          repository,
          session,
          fake as InboxAccessController,
          policy,
          operator
        )
      ).toBeUndefined();
      expect(
        m.inboxProbeNonceProofBinding(
          fake as InboxProbeNonceProof,
          repository,
          session,
          policy
        )
      ).toBeUndefined();
      m.stopInboxProbeRecoveryReader(fake as InboxProbeRecoveryReader);
      m.expireInboxProbeRecoveryReaderWait(fake as InboxProbeRecoveryReader);
    }
    expect(effects).toBe(0);
  });
