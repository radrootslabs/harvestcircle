import { expect, it } from 'vitest';
import { deliverListing } from '../../src/lib/publishing/deliver-listing.ts';
import {
  authorizeListingAttempt,
  takeListingPublication,
  withListingDeliveryOwnership,
  type ListingDeliveryLease,
  type ListingAttemptPermission
} from '../../src/lib/nostr/listing-publisher.ts';
import type { ListingPublication } from '../../src/lib/publishing/sign-listing.ts';
import type { PublicPool } from '../../src/lib/nostr/public-pool.ts';
import { deploymentRelayPolicy } from '../../src/lib/config/deployment-relays.ts';

it('HCP112 copied durable disclosure cannot authorize a network action during SSR', async () => {
  expect(
    await deliverListing(
      {} as ListingPublication,
      {} as PublicPool,
      deploymentRelayPolicy,
      'publish_stored_listing'
    )
  ).toEqual({ status: 'invalid' });
});
it('HCP112 copied lease cannot grant an event or readback permission', async () => {
  for (const mode of ['publish', 'readback'] as const)
    expect(
      await authorizeListingAttempt(
        {} as ListingDeliveryLease,
        'wss://discovery.example.org',
        mode
      )
    ).toBeUndefined();
});
it('HCP112 copied per-target permission exposes no raw SDK event form', () => {
  for (const mode of ['publish', 'readback'] as const)
    expect(
      takeListingPublication(
        {} as ListingAttemptPermission,
        deploymentRelayPolicy,
        'wss://discovery.example.org',
        mode
      )
    ).toBeUndefined();
});
it('HCP112 copied source cannot execute an arbitrary ownership callback', async () => {
  let called = false;
  expect(
    await withListingDeliveryOwnership(
      {} as ListingPublication,
      deploymentRelayPolicy,
      'publish_stored_listing',
      () => {
        called = true;
        return Promise.resolve('unexpected');
      }
    )
  ).toEqual({ status: 'invalid' });
  expect(called).toBe(false);
});
