import { afterEach, expect, it, vi } from 'vitest';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import type { RelayPolicy } from '../../src/lib/config/relays.ts';
afterEach(() => vi.unstubAllGlobals());
for (const browser of [false, true])
  it(`cold ${browser ? 'browser-shaped' : 'SSR'} probe cleanup refuses forged source, native and retirement authority without effects`, async () => {
    vi.resetModules();
    let effects = 0;
    const trap = () => {
      effects++;
      throw Error('unadmitted cleanup effect');
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
      vi.stubGlobal('indexedDB', { open: trap });
      vi.stubGlobal('WebSocket', trap);
    }
    const cleanup =
        await import('../../src/lib/persistence/inbox-probe-cleanup.ts'),
      journal = await import('../../src/lib/persistence/inbox-probes.ts'),
      native = await import('../../src/lib/persistence/private-storage.ts'),
      publisher = await import('../../src/lib/nostr/private-publisher.ts');
    const repository = {} as PrivateStorageRepository,
      session = {} as PrivateSession,
      policy = {} as RelayPolicy;
    for (const value of [
      undefined,
      null,
      false,
      0,
      '',
      Symbol(),
      {},
      { status: 'nonce_verified', current: () => true },
      { status: 'removed', removed: 1, selection: {} }
    ]) {
      expect(
        await cleanup.captureInboxProbeCleanup(
          repository,
          session,
          policy,
          value as Parameters<typeof cleanup.captureInboxProbeCleanup>[3],
          [],
          'reviewed_inbox_probe_cleanup_selection'
        )
      ).toBeUndefined();
      expect(
        await cleanup.deleteReviewedInboxProbe(
          value as Parameters<typeof cleanup.deleteReviewedInboxProbe>[0],
          'reviewed_delete_local_inbox_probe'
        )
      ).toEqual({ status: 'invalid' });
      expect(
        cleanup.inboxProbeCleanupAdmission(
          value as Parameters<typeof cleanup.inboxProbeCleanupAdmission>[0],
          repository,
          session
        )
      ).toBeUndefined();
      expect(
        await native.removePrivateInboxProbe(
          repository,
          session,
          value as Parameters<typeof native.removePrivateInboxProbe>[2]
        )
      ).toEqual({
        ok: false,
        reason: 'invalid_scope',
        disposition: 'not_removed'
      });
      expect(
        native.privateProbeRemovalEvidence(
          value as Parameters<typeof native.privateProbeRemovalEvidence>[0],
          repository,
          session
        )
      ).toBeUndefined();
      expect(
        publisher.acquireInboxProbeCleanupLease(
          repository,
          session,
          policy,
          value as Parameters<typeof publisher.acquireInboxProbeCleanupLease>[3]
        )
      ).toBeUndefined();
      expect(
        publisher.inboxProbeCleanupReady(
          repository,
          session,
          policy,
          value as Parameters<typeof publisher.inboxProbeCleanupReady>[3]
        )
      ).toBe(false);
      expect(
        journal.appliedInboxProbeArchiveAdmission(
          value as Parameters<
            typeof journal.appliedInboxProbeArchiveAdmission
          >[0]
        )
      ).toBeUndefined();
      publisher.reconcileInboxProbeArchiveAdmission(
        value as Parameters<
          typeof publisher.reconcileInboxProbeArchiveAdmission
        >[0]
      );
      expect(
        publisher.retireInboxProbeCleanupLease(
          value as Parameters<typeof publisher.retireInboxProbeCleanupLease>[0],
          {} as Parameters<typeof publisher.retireInboxProbeCleanupLease>[1]
        )
      ).toBe(false);
      expect(
        journal.retireInboxProbeCleanupSelection(
          value as Parameters<
            typeof journal.retireInboxProbeCleanupSelection
          >[0],
          {} as Parameters<typeof journal.retireInboxProbeCleanupSelection>[1]
        )
      ).toBe(false);
      expect(
        cleanup.inboxProbeRemovalAcknowledgementBinding(
          value as Parameters<
            typeof cleanup.inboxProbeRemovalAcknowledgementBinding
          >[0],
          repository,
          session,
          policy
        )
      ).toBeUndefined();
      cleanup.stopInboxProbeCleanup(
        value as Parameters<typeof cleanup.stopInboxProbeCleanup>[0]
      );
      publisher.releaseInboxProbeCleanupLease(
        value as Parameters<typeof publisher.releaseInboxProbeCleanupLease>[0]
      );
    }
    expect(effects).toBe(0);
  });
