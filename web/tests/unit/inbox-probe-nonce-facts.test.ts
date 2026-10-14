import { afterEach, expect, it, vi } from 'vitest';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import type { RelayPolicy } from '../../src/lib/config/relays.ts';
import type {
  InboxProbeNonceProof,
  RecoveredInboxProbeNonceProof,
  InboxProbeNonceReader,
  RecoveredInboxProbeNonceReader
} from '../../src/lib/nostr/self-recovery-reader.ts';
import type { InboxProbeReadbackAttemptResult } from '../../src/lib/nostr/private-pool.ts';
import type { InboxProbeNonceAcknowledgement } from '../../src/lib/persistence/inbox-probes.ts';

afterEach(() => vi.unstubAllGlobals());
for (const browser of [false, true])
  it(`cold ${browser ? 'browser-shaped' : 'SSR'} nonce facts reject forged provenance before native SDK or cleanup work`, async () => {
    vi.resetModules();
    let effects = 0;
    const trap = () => {
      effects++;
      throw Error('unadmitted nonce effect');
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
    const reader = await import('../../src/lib/nostr/self-recovery-reader.ts'),
      journal = await import('../../src/lib/persistence/inbox-probes.ts');
    const repository = {} as PrivateStorageRepository,
      identity = {} as IdentitySession,
      session = {} as PrivateSession,
      policy = {} as RelayPolicy;
    for (const fake of [
      undefined,
      null,
      false,
      0,
      '',
      Symbol(),
      {},
      { status: 'nonce_verified', proof: {}, current: () => true },
      { source: 'recovered', acknowledgement: {}, nonce_verified: true }
    ]) {
      expect(
        reader.captureRecoveredInboxProbeNonceReader(
          repository,
          identity,
          session,
          policy,
          fake as InboxProbeReadbackAttemptResult,
          'reviewed_inbox_probe_nonce_decryption'
        )
      ).toBeUndefined();
      expect(
        await reader.readInboxProbeNonce(fake as InboxProbeNonceReader)
      ).toEqual({ status: 'invalid' });
      expect(
        await reader.readRecoveredInboxProbeNonce(
          fake as RecoveredInboxProbeNonceReader
        )
      ).toEqual({ status: 'invalid' });
      expect(
        reader.inboxProbeNonceVerificationEvidence(
          fake as InboxProbeNonceProof | RecoveredInboxProbeNonceProof,
          repository,
          session,
          policy
        )
      ).toBeUndefined();
      const result = await journal.commitInboxProbeNonceVerification(
        repository,
        session,
        policy,
        fake as InboxProbeNonceProof | RecoveredInboxProbeNonceProof
      );
      expect('acknowledgement' in result).toBe(false);
      expect(
        journal.inboxProbeNonceAcknowledgementBinding(
          repository,
          session,
          fake as InboxProbeNonceAcknowledgement,
          policy
        )
      ).toBeUndefined();
      reader.stopRecoveredInboxProbeNonceReader(
        fake as RecoveredInboxProbeNonceReader
      );
      reader.expireRecoveredInboxProbeNonceReaderWait(
        fake as RecoveredInboxProbeNonceReader
      );
    }
    expect(effects).toBe(0);
  });
