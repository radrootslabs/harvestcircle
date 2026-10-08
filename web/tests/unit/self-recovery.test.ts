import { describe, expect, it } from 'vitest';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';
import type { ReservedSendIdentity } from '../../src/lib/messaging/send-identity.ts';
import type { VerifiedOutboundEnvelope } from '../../src/lib/nostr/verify-outbound-envelope.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import {
  commitSelfRecovery,
  selfRecoveryAcknowledgementSnapshot,
  type SelfRecoveryAcknowledgement
} from '../../src/lib/persistence/private-sends.ts';
import {
  captureSelfRecoveryPreparation,
  prepareSelfRecovery,
  selfRecoveryPreparationSnapshot,
  preparedSelfRecovery,
  selfRecoveryPeerPreparation,
  stopSelfRecoveryPreparation,
  type SelfRecoveryPreparation
} from '../../src/lib/messaging/prepare-send.ts';

describe('self recovery is an explicit current-owner local acknowledgement', () => {
  it('SSR and forged namespaces/identities cannot start crypto or persistence', () => {
    expect(
      captureSelfRecoveryPreparation(
        {} as PrivateStorageRepository,
        {} as IdentitySession,
        {} as ReservedSendIdentity,
        'reviewed_self_recovery'
      )
    ).toBeUndefined();
  });
  it('forged preparation and receipt objects grant no acknowledgement or peer permission', async () => {
    const preparation = {} as SelfRecoveryPreparation,
      receipt = {} as SelfRecoveryAcknowledgement;
    expect(
      await prepareSelfRecovery(preparation, 'reviewed_self_recovery')
    ).toEqual({ status: 'invalid' });
    expect(selfRecoveryPreparationSnapshot(preparation)).toBeUndefined();
    expect(preparedSelfRecovery(preparation)).toBeUndefined();
    expect(
      await selfRecoveryPeerPreparation(preparation, receipt)
    ).toBeUndefined();
    expect(selfRecoveryAcknowledgementSnapshot(receipt)).toBeUndefined();
    expect(() => stopSelfRecoveryPreparation(preparation)).not.toThrow();
  });
  it('detached factory data cannot produce a durable receipt', async () => {
    expect(
      await commitSelfRecovery(
        {} as PrivateStorageRepository,
        {} as IdentitySession,
        {} as ReservedSendIdentity,
        {} as VerifiedOutboundEnvelope,
        'reviewed_self_commit'
      )
    ).toEqual({ status: 'invalid' });
  });
  it('unreviewed values are rejected without coercion or provider callbacks', async () => {
    let conversions = 0;
    const review = {
      toString() {
        conversions++;
        return 'reviewed_self_commit';
      }
    };
    expect(
      await commitSelfRecovery(
        {} as PrivateStorageRepository,
        {} as IdentitySession,
        {} as ReservedSendIdentity,
        {} as VerifiedOutboundEnvelope,
        review
      )
    ).toEqual({ status: 'invalid' });
    expect(
      captureSelfRecoveryPreparation(
        {} as PrivateStorageRepository,
        {} as IdentitySession,
        {} as ReservedSendIdentity,
        review
      )
    ).toBeUndefined();
    expect(conversions).toBe(0);
  });
});
