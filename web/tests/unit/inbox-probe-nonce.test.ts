import { afterEach, expect, it, vi } from 'vitest';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import type { RelayPolicy } from '../../src/lib/config/relays.ts';
import type { InboxProbeReadbackAttemptResult } from '../../src/lib/nostr/private-pool.ts';
import type {
  InboxProbeNonceReader,
  InboxProbeNonceProof,
  SelfRecoveryReader,
  RecoveredSelfEnvelope
} from '../../src/lib/nostr/self-recovery-reader.ts';
afterEach(() => vi.unstubAllGlobals());
for (const browser of [false, true])
  it(`cold ${browser ? 'browser-shaped' : 'SSR'} nonce admission rejects forged families before provider, storage or socket effects`, async () => {
    vi.resetModules();
    let effects = 0;
    const trap = () => {
      effects++;
      throw Error('forged effect');
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
      policy = {} as RelayPolicy;
    for (const fake of [
      null,
      undefined,
      0,
      false,
      '',
      Symbol(),
      {},
      { status: 'readback', nonce_verified: true, current: () => true },
      { status: 'nonce_verified', proof: {} }
    ]) {
      expect(
        m.captureInboxProbeNonceReader(
          repository,
          identity,
          session,
          policy,
          fake as InboxProbeReadbackAttemptResult,
          'reviewed_inbox_probe_nonce_decryption'
        )
      ).toBeUndefined();
      expect(
        await m.readInboxProbeNonce(fake as InboxProbeNonceReader)
      ).toEqual({ status: 'invalid' });
      expect(
        m.inboxProbeNonceProofBinding(
          fake as InboxProbeNonceProof,
          repository,
          session,
          policy
        )
      ).toBeUndefined();
      m.stopInboxProbeNonceReader(fake as InboxProbeNonceReader);
      m.expireInboxProbeNonceReaderWait(fake as InboxProbeNonceReader);
      expect(
        await m.readSelfRecoveryEnvelope(fake as SelfRecoveryReader)
      ).toEqual({ status: 'invalid' });
      expect(
        m.recoveredSelfEnvelopeSnapshot(fake as RecoveredSelfEnvelope)
      ).toBeUndefined();
    }
    expect(effects).toBe(0);
  });
