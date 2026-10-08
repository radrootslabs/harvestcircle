import { describe, it, expect } from 'vitest';
import type { SelfRecoveryPreparation } from '../../src/lib/messaging/prepare-send.ts';
import {
  preparePairedDelivery,
  preparedPairedDelivery
} from '../../src/lib/messaging/prepare-send.ts';
import {
  pairedDeliveryAcknowledgementSnapshot,
  type PairedDeliveryAcknowledgement
} from '../../src/lib/persistence/private-sends.ts';
import type { PairedDeliveryContext } from '../../src/lib/messaging/prepare-send.ts';

describe('paired delivery is acknowledged local evidence only', () => {
  it('SSR forged preparation cannot perform crypto, persistence or acquire a ready receipt', async () => {
    const token = {} as SelfRecoveryPreparation;
    expect(
      await preparePairedDelivery(
        token,
        {} as PairedDeliveryContext,
        'reviewed_pair_preparation'
      )
    ).toEqual({ status: 'invalid' });
    expect(preparedPairedDelivery(token)).toBeUndefined();
  });
  it('a detached paired receipt grants no local acknowledgement', () => {
    expect(
      pairedDeliveryAcknowledgementSnapshot({} as PairedDeliveryAcknowledgement)
    ).toBeUndefined();
  });
  it('unreviewed input is inert without coercion', async () => {
    let calls = 0;
    const review = {
      toString() {
        calls++;
        return 'reviewed_pair_preparation';
      }
    };
    expect(
      await preparePairedDelivery(
        {} as SelfRecoveryPreparation,
        {} as PairedDeliveryContext,
        review
      )
    ).toEqual({ status: 'invalid' });
    expect(calls).toBe(0);
  });
});
