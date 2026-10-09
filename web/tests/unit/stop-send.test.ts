import { describe, it, expect } from 'vitest';
import { reconcilePrivateSendAttempt } from '../../src/lib/persistence/private-send-settlements.ts';
import {
  stopPrivateSend,
  privateStopSendSnapshot
} from '../../src/lib/messaging/stop-send.ts';
import type { PrivateRetry } from '../../src/lib/messaging/retry-send.ts';
describe('original-operation stopped send provenance', () => {
  it('structural accepted result cannot mint any original-owner receipt', async () => {
    expect(
      await reconcilePrivateSendAttempt({
        status: 'accepted',
        role: 'peer',
        origin: 'wss://peer.example.org',
        eventId: 'a'.repeat(64),
        actionId: '12345678-1234-4234-8234-123456789abc',
        attempt: 1
      })
    ).toEqual({ status: 'invalid' });
  });
  it('forged retry has no stopped send projection or effect', () => {
    const retry = {} as PrivateRetry;
    stopPrivateSend(retry);
    expect(privateStopSendSnapshot(retry)).toBeUndefined();
  });
});
