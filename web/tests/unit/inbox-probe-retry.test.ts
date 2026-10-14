import { afterEach, expect, it, vi } from 'vitest';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import type { RelayPolicy } from '../../src/lib/config/relays.ts';
import type { InboxOperatorQualification } from '../../src/lib/config/inbox-operators.ts';
import type { InboxAccessController } from '../../src/lib/messaging/inbox-access-controller.ts';
import type { RecoveredInboxProbe } from '../../src/lib/nostr/self-recovery-reader.ts';
import type { InboxProbeAcknowledgement } from '../../src/lib/persistence/inbox-probes.ts';

afterEach(() => vi.unstubAllGlobals());
for (const browser of [false, true])
  it(`cold ${browser ? 'browser-shaped' : 'SSR'} Retry rejects forged families before native SDK or cleanup effects`, async () => {
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
    const storage = await import('../../src/lib/persistence/inbox-probes.ts'),
      publisher = await import('../../src/lib/nostr/private-publisher.ts'),
      pool = await import('../../src/lib/nostr/private-pool.ts');
    const repository = {} as PrivateStorageRepository,
      session = {} as PrivateSession,
      access = {} as InboxAccessController,
      policy = {} as RelayPolicy,
      operator = {} as InboxOperatorQualification;
    for (const fake of [
      undefined,
      null,
      false,
      0,
      '',
      Symbol(),
      {},
      { source: 'recovered', current: () => true, nonce_verified: true },
      { status: 'adopted', acknowledgement: {} }
    ]) {
      const result = await storage.adoptRecoveredInboxProbe(
        repository,
        session,
        access,
        policy,
        operator,
        fake as RecoveredInboxProbe,
        'reviewed_original_inbox_probe_retry'
      );
      expect('acknowledgement' in result).toBe(false);
      expect(
        await publisher.captureRecoveredInboxProbeRetryAction(
          repository,
          session,
          policy,
          fake as InboxProbeAcknowledgement,
          'reviewed_original_inbox_probe_retry'
        )
      ).toBeUndefined();
      expect(
        storage.recoveredInboxProbePublicationBinding(
          repository,
          session,
          fake as InboxProbeAcknowledgement,
          policy
        )
      ).toBeUndefined();
      expect(
        pool.privatePoolProbeRetryIdle(fake as PrivateSession, policy)
      ).toBe(false);
    }
    expect(effects).toBe(0);
  });
