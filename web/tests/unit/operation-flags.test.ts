import { describe, expect, it } from 'vitest';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import { operationEnabled } from '../../src/lib/config/operation-flags.ts';
import { operationCapabilities } from '../../src/lib/runtime/capabilities.ts';

function policy(postingEnabled: boolean, messagingEnabled: boolean) {
  const value = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [
        {
          origin: 'wss://discovery.example.org',
          read: true,
          write: true,
          nip50: false
        }
      ],
      inbox: [{ origin: 'wss://inbox.example.org', read: true, write: true }],
      postingEnabled,
      messagingEnabled,
      operatorDenylist: []
    })
  );
  if (!value) throw Error('genuine policy missing');
  return value;
}
describe('HCP126 separate static availability without effect authority', () => {
  for (const posting of [false, true])
    for (const messaging of [false, true]) {
      it(`admits posting=${posting}, messaging=${messaging} independently while new listings require both`, () => {
        const original = policy(posting, messaging);
        expect(operationEnabled(original, 'posting')).toBe(posting);
        expect(operationEnabled(original, 'messaging')).toBe(messaging);
        expect(operationEnabled(original, 'new_listing')).toBe(
          posting && messaging
        );
        expect(operationCapabilities(original)).toMatchObject({
          posting,
          messaging,
          newListing: posting && messaging,
          draft: true,
          pendingReview: true,
          qualification: 'not_established_by_flags'
        });
      });
    }
  it('copied and absent policy observations fail closed without hiding retained local work', () => {
    const original = policy(true, true);
    for (const value of [{ ...original }, undefined]) {
      expect(operationEnabled(value, 'posting')).toBe(false);
      expect(operationEnabled(value, 'messaging')).toBe(false);
      expect(operationEnabled(value, 'new_listing')).toBe(false);
      expect(operationCapabilities(value)).toMatchObject({
        posting: false,
        messaging: false,
        newListing: false,
        draft: true,
        pendingReview: true
      });
    }
    expect(operationEnabled(original, 'receive_as_order')).toBe(false);
    const view = operationCapabilities(original);
    expect(view).not.toHaveProperty('sign');
    expect(view).not.toHaveProperty('send');
    expect(view).not.toHaveProperty('resume');
    expect(operationCapabilities(original)).not.toBe(view);
  });
});
