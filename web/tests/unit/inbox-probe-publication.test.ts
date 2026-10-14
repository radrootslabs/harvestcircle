import { afterEach, expect, it, vi } from 'vitest';
import type { InboxProbeAcknowledgement } from '../../src/lib/persistence/inbox-probes.ts';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import type { RelayPolicy } from '../../src/lib/config/relays.ts';
import type {
  InboxProbePublication,
  InboxProbePublicationAction
} from '../../src/lib/nostr/private-publisher.ts';
import type {
  PrivatePool,
  InboxProbeArchiveAttemptResult
} from '../../src/lib/nostr/private-pool.ts';
afterEach(() => vi.unstubAllGlobals());
for (const browser of [false, true]) {
  it(`cold ${browser ? 'browser-shaped' : 'SSR'} publication/receipt imports and forged capabilities cause no effects`, async () => {
    vi.resetModules();
    let effects = 0;
    const trap = () => {
      effects++;
      throw Error('forged work');
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
    const publisher = await import('../../src/lib/nostr/private-publisher.ts'),
      pool = await import('../../src/lib/nostr/private-pool.ts'),
      persistence = await import('../../src/lib/persistence/inbox-probes.ts');
    const repository = {} as PrivateStorageRepository,
      session = {} as PrivateSession,
      policy = {} as RelayPolicy;
    for (const fake of [
      null,
      undefined,
      false,
      0,
      'token',
      Symbol(),
      {},
      { status: 'accepted', current: () => true }
    ]) {
      expect(
        publisher.captureInboxProbePublicationAction(
          repository,
          session,
          policy,
          fake as InboxProbeAcknowledgement,
          'reviewed_inbox_probe_network'
        )
      ).toBeUndefined();
      expect(
        publisher.captureInboxProbePublication(
          repository,
          session,
          policy,
          fake as InboxProbeAcknowledgement,
          fake as InboxProbePublicationAction,
          'wss://inbox.example.org',
          'reviewed_inbox_probe_publication'
        )
      ).toBeUndefined();
      expect(
        await pool.publishInboxProbeArchiveAttempt(
          {} as PrivatePool,
          fake as InboxProbePublication,
          new AbortController().signal
        )
      ).toEqual({ status: 'stopped' });
      expect(
        pool.inboxProbeArchiveAttemptEvidence(
          fake as InboxProbeArchiveAttemptResult
        )
      ).toBeUndefined();
      expect(
        await persistence.commitInboxProbeArchiveAttempt(
          repository,
          fake as InboxProbeArchiveAttemptResult
        )
      ).toEqual({ status: 'invalid', disposition: 'original_protected' });
    }
    expect(effects).toBe(0);
  });
}
