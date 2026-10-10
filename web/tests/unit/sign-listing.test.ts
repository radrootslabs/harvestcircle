import { expect, it } from 'vitest';
import {
  beginListingSigning,
  runListingSigning,
  listingSigningSnapshot,
  listingPublicationSnapshot,
  type ListingSigning,
  type ListingPublication
} from '../../src/lib/publishing/sign-listing.ts';
import type { ListingReview } from '../../src/lib/publishing/prepare-listing.ts';

it('HCP111 copied review cannot create signing ownership', () => {
  expect(
    beginListingSigning({} as ListingReview, 'sign_reviewed_listing')
  ).toBeUndefined();
});
it('HCP111 SSR copied command cannot sign or disclose original state', async () => {
  const copy = {} as ListingSigning;
  expect(listingSigningSnapshot(copy)).toBeUndefined();
  expect(await runListingSigning(copy, 'sign_reviewed_listing')).toEqual({
    status: 'invalid'
  });
});
it('HCP111 copied publication cannot disclose a durable signed plan', () => {
  expect(listingPublicationSnapshot({} as ListingPublication)).toBeUndefined();
});
