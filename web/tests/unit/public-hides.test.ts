import { expect, test } from 'vitest';
import {
  createLocalPublicHides,
  localPublicHidesSnapshot,
  localPublicPublisherAllowed,
  hideLocalPublicPublisher,
  undoLocalPublicPublisher,
  closeLocalPublicHides,
  type LocalPublicHides
} from '../../src/lib/persistence/local-public-hides.ts';
import { createIdentityViewContext } from '../../src/lib/runtime/view-context.ts';
import type { VerifiedEnvelope } from '../../src/lib/nostr/verified-envelope.ts';

test('SSR construction exposes no owner inventory and acquires no storage', () => {
  const hides = createLocalPublicHides(createIdentityViewContext());
  expect(localPublicHidesSnapshot(hides)).toEqual({ state: 'guest' });
  expect(localPublicPublisherAllowed(hides, 'a'.repeat(64))).toBe(true);
  closeLocalPublicHides(hides);
});

for (const copied of [undefined, null, {}, { state: 'ready', publishers: [] }])
  test(`copied local hide token has no account custody: ${JSON.stringify(copied)}`, async () => {
    const token = copied as LocalPublicHides;
    expect(localPublicHidesSnapshot(token)).toEqual({ state: 'unavailable' });
    expect(localPublicPublisherAllowed(token, 'a'.repeat(64))).toBe(false);
    expect(
      await hideLocalPublicPublisher(
        token,
        {} as VerifiedEnvelope,
        'reviewed_local_public_hide'
      )
    ).toEqual({ status: 'invalid' });
    expect(
      await undoLocalPublicPublisher(
        token,
        'a'.repeat(64),
        'reviewed_local_public_undo'
      )
    ).toEqual({ status: 'invalid' });
    expect(() => closeLocalPublicHides(token)).not.toThrow();
  });

test('a connected-looking render snapshot cannot substitute for a verified publisher', async () => {
  const hides = createLocalPublicHides(createIdentityViewContext());
  expect(
    await hideLocalPublicPublisher(
      hides,
      { pubkey: 'a'.repeat(64) } as unknown as VerifiedEnvelope,
      'reviewed_local_public_hide'
    )
  ).toEqual({ status: 'invalid' });
  closeLocalPublicHides(hides);
});
