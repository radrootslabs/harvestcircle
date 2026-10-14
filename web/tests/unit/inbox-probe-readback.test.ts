import { afterEach, expect, it, vi } from 'vitest';
import { finalizeEvent, generateSecretKey } from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { inboxProbeReadbackMatches } from '../../src/lib/persistence/private-records.ts';
import type { InboxProbeAcknowledgement } from '../../src/lib/persistence/inbox-probes.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { RelayPolicy } from '../../src/lib/config/relays.ts';
import type {
  InboxProbeReadback,
  InboxProbePublicationAction
} from '../../src/lib/nostr/private-publisher.ts';
import type {
  PrivatePool,
  InboxProbeReadbackAttemptResult
} from '../../src/lib/nostr/private-pool.ts';
afterEach(() => vi.unstubAllGlobals());
for (const browser of [false, true])
  it(`cold ${browser ? 'browser-shaped' : 'SSR'} readback families reject forged inputs before any effects`, async () => {
    vi.resetModules();
    let effects = 0;
    const trap = () => {
      effects++;
      throw Error('forged effects');
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
    }
    const p = await import('../../src/lib/nostr/private-publisher.ts'),
      t = await import('../../src/lib/nostr/private-pool.ts'),
      s = await import('../../src/lib/persistence/inbox-probes.ts');
    for (const fake of [
      null,
      undefined,
      0,
      false,
      '',
      Symbol(),
      {},
      { current: () => true, status: 'readback' }
    ]) {
      expect(
        p.captureInboxProbeReadback(
          {} as PrivateStorageRepository,
          {} as PrivateSession,
          {} as RelayPolicy,
          fake as InboxProbeAcknowledgement,
          fake as InboxProbePublicationAction,
          'wss://inbox.example.org',
          'reviewed_inbox_probe_readback'
        )
      ).toBeUndefined();
      expect(
        p.inboxProbeReadbackSnapshot(fake as InboxProbeReadback)
      ).toBeUndefined();
      expect(
        await t.readInboxProbeAttempt(
          {} as PrivatePool,
          fake as InboxProbeReadback,
          new AbortController().signal
        )
      ).toEqual({ status: 'stopped' });
      expect(
        t.inboxProbeReadbackAttemptEvidence(
          fake as InboxProbeReadbackAttemptResult
        )
      ).toBeUndefined();
      expect(
        await s.commitInboxProbeReadbackAttempt(
          {} as PrivateStorageRepository,
          fake as InboxProbeReadbackAttemptResult
        )
      ).toEqual({ status: 'invalid', disposition: 'original_protected' });
    }
    expect(effects).toBe(0);
  });
it('pure readback comparison preserves bounded alternate serialization and unsigned metadata with independently identical signed core', () => {
  const owner =
      '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
    key = generateSecretKey(),
    conversation = nip44.v2.utils.getConversationKey(key, owner);
  try {
    const e = finalizeEvent(
        {
          kind: 1059,
          created_at: 1700000000,
          tags: [['p', owner]],
          content: nip44.v2.encrypt(
            'Structural source fixture, no nonce proof',
            conversation
          )
        },
        key
      ),
      self = { eventId: e.id, wire: JSON.stringify(e) };
    const alternate = {
      sig: e.sig,
      content: e.content,
      tags: e.tags,
      created_at: e.created_at,
      kind: e.kind,
      pubkey: e.pubkey,
      id: e.id,
      observed: { source: 'fixture' }
    };
    for (const wire of [
      self.wire,
      self.wire + '\n',
      JSON.stringify(alternate, null, 2)
    ])
      expect(inboxProbeReadbackMatches(self, owner, wire)).toBe(true);
    for (const wire of [
      null,
      undefined,
      {},
      JSON.stringify({ ...e, kind: 14 }),
      JSON.stringify({ ...e, sig: '0'.repeat(128), verified: true }),
      JSON.stringify({ ...e, id: 'f'.repeat(64) }),
      JSON.stringify({ ...e, content: e.content + 'a' }),
      JSON.stringify({ ...e, tags: [['p', 'f'.repeat(64)]] }),
      self.wire.replace('"kind":1059', '"kind":1059,"kind":1059'),
      self.wire.replace('"kind":1059', '"kind":1.059e3'),
      self.wire + ' '.repeat(32768)
    ])
      expect(inboxProbeReadbackMatches(self, owner, wire)).toBe(false);
    expect(inboxProbeReadbackMatches(self, 'f'.repeat(64), self.wire)).toBe(
      false
    );
  } finally {
    conversation.fill(0);
    key.fill(0);
  }
});
