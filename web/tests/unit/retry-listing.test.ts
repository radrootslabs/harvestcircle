import { expect, it } from 'vitest';
import {
  beginListingRetry,
  listingRetryResult,
  stopListingRetry,
  type ListingRetry
} from '../../src/lib/publishing/retry-listing.ts';
import {
  prepareListingResume,
  resumeListing,
  listingResumeSnapshot,
  type ListingResumption
} from '../../src/lib/publishing/resume-listing.ts';
import type { ListingPublication } from '../../src/lib/publishing/sign-listing.ts';
import type { PublicPool } from '../../src/lib/nostr/public-pool.ts';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';
import type { PublicPublicationRepository } from '../../src/lib/persistence/public-publications.ts';
import { deploymentRelayPolicy } from '../../src/lib/config/deployment-relays.ts';

it('HCP113 copied publication cannot mint an explicit retry or network action during SSR', () => {
  expect(
    beginListingRetry(
      {} as ListingPublication,
      {} as PublicPool,
      deploymentRelayPolicy,
      'retry_stored_listing'
    )
  ).toBeUndefined();
});
it('HCP113 copied repository and identity cannot reopen another stored command during SSR', async () => {
  expect(
    await prepareListingResume(
      {} as PublicPublicationRepository,
      {} as IdentitySession,
      deploymentRelayPolicy,
      '00000000-0000-4000-8000-000000000001',
      'reviewed_stored_listing_resume'
    )
  ).toEqual({ status: 'invalid' });
});
it('HCP113 copied resume disclosure cannot substitute for original stored authority', () => {
  const copied = {} as ListingResumption;
  expect(listingResumeSnapshot(copied)).toBeUndefined();
  expect(
    resumeListing(copied, {} as PublicPool, 'resume_stored_listing')
  ).toBeUndefined();
});
it('HCP113 copied Stop control neither invokes an effect nor borrows a pending result', async () => {
  const copied = {} as ListingRetry;
  stopListingRetry(copied);
  expect(await listingRetryResult(copied)).toEqual({ status: 'invalid' });
});
