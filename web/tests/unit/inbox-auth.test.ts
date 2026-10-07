import { describe, it, expect } from 'vitest';
import { PRIVATE_AUTH_BUDGETS } from '../../src/lib/config/budgets.ts';
import {
  inboxAuthSnapshot,
  respondInboxAuthentication,
  closeInboxAuthentication
} from '../../src/lib/nostr/inbox-auth.ts';
describe('connection AUTH admission', () => {
  it('retains the exact two-response authority', () => {
    expect(PRIVATE_AUTH_BUDGETS.responsesPerConnectionAction).toBe(2);
  });
  it('forged actions never become signer or transport authority', async () => {
    const forged = {} as never;
    expect(inboxAuthSnapshot(forged)).toBeUndefined();
    expect(await respondInboxAuthentication(forged)).toEqual({
      status: 'invalid'
    });
    expect(() => closeInboxAuthentication(forged)).not.toThrow();
  });
});
