import { describe, expect, it } from 'vitest';
import {
  reviewNewListing,
  listingReviewSnapshot,
  reserveNewListing,
  type ListingReview
} from '../../src/lib/publishing/prepare-listing.ts';
import {
  createPublicPublicationRepository,
  publicPublicationDraftRepository,
  type PublicPublicationRepository
} from '../../src/lib/persistence/public-publications.ts';
import type { BrowserDatabase } from '../../src/lib/persistence/database.ts';
import type { PublicDraftController } from '../../src/lib/publishing/draft-controller.ts';
import type { NewListingReadiness } from '../../src/lib/publishing/new-listing-readiness.ts';

describe('HCP110 immutable public listing intent admission', () => {
  it('SSR and copied database cannot acquire a publication repository', () => {
    expect(
      createPublicPublicationRepository({} as BrowserDatabase, '0'.repeat(64))
    ).toBeUndefined();
  });
  it('copied repository cannot expose an original draft repository', () => {
    expect(
      publicPublicationDraftRepository({} as PublicPublicationRepository)
    ).toBeUndefined();
  });
  it('copied controller/readiness cannot prepare an intent', async () => {
    expect(
      await reviewNewListing(
        {} as PublicPublicationRepository,
        {} as PublicDraftController,
        {} as NewListingReadiness,
        '3d030bf5-901d-45e1-8251-41cbdf805e96',
        'reviewed_new_listing'
      )
    ).toEqual({ status: 'blocked', reason: 'invalid_scope' });
  });
  it('copied review and boolean consent cannot reserve or reveal a template', async () => {
    const copied = {} as ListingReview;
    expect(listingReviewSnapshot(copied)).toBeUndefined();
    expect(await reserveNewListing(copied, true)).toEqual({
      status: 'invalid'
    });
  });
});
