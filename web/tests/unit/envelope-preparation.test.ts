import { describe, it, expect } from 'vitest';
import {
  captureEnvelopePreparation,
  prepareEnvelopeRole,
  envelopePreparationSnapshot,
  preparedEnvelopeProof,
  stopEnvelopePreparation,
  expireEnvelopePreparationWait,
  closeEnvelopePreparation,
  type EnvelopePreparation
} from '../../src/lib/messaging/envelope-preparation.ts';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';
import type { ReservedSendIdentity } from '../../src/lib/messaging/send-identity.ts';
describe('opaque pair preparation without browser custody', () => {
  it('does not admit detached identity or reservation in SSR', () => {
    expect(
      captureEnvelopePreparation(
        {} as IdentitySession,
        {} as ReservedSendIdentity,
        'reviewed_envelope_pair'
      )
    ).toBeUndefined();
    expect(
      captureEnvelopePreparation(
        {} as IdentitySession,
        {} as ReservedSendIdentity,
        'wrong'
      )
    ).toBeUndefined();
  });
  it('casts cannot expose encrypted state or spend an operation', async () => {
    const cast = Object.freeze({}) as EnvelopePreparation;
    expect(
      await prepareEnvelopeRole(cast, 'self', 'reviewed_pair_role')
    ).toEqual({ status: 'invalid' });
    expect(envelopePreparationSnapshot(cast)).toBeUndefined();
    expect(preparedEnvelopeProof(cast, 'self')).toBeUndefined();
    stopEnvelopePreparation(cast);
    expireEnvelopePreparationWait(cast);
    closeEnvelopePreparation(cast);
    closeEnvelopePreparation(cast);
    expect(
      await prepareEnvelopeRole(cast, 'peer', 'reviewed_pair_role')
    ).toEqual({ status: 'invalid' });
  });
});
