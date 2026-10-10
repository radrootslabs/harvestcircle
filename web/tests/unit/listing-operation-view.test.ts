import {
  subscribeListingSigning,
  type ListingSigning
} from '../../src/lib/publishing/sign-listing.ts';
import { expect, it } from 'vitest';
import { render } from 'svelte/server';
import OperationStatus from '../../src/lib/components/OperationStatus.svelte';
import {
  readListingOperationView,
  listingOperationSnapshot,
  closeListingOperationView,
  subscribeListingOperationView,
  type ListingOperationView
} from '../../src/lib/publishing/listing-operation-view.ts';
import type { PublicPublicationRepository } from '../../src/lib/persistence/public-publications.ts';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';

it('HCP114 copied repository and identity cannot disclose a stored operation during SSR', async () => {
  expect(
    await readListingOperationView(
      {} as PublicPublicationRepository,
      {} as IdentitySession,
      '00000000-0000-4000-8000-000000000001'
    )
  ).toEqual({ status: 'unavailable' });
});
it('HCP114 detached state combinations cannot replace an original presentation handle', () => {
  expect(
    listingOperationSnapshot({
      state: 'accepted',
      acceptanceCount: 2,
      artifact: 'unsigned'
    } as unknown as ListingOperationView)
  ).toBeUndefined();
});
it('HCP114 copied Close and subscription cannot borrow another view lifecycle', () => {
  const copied = {} as ListingOperationView;
  let calls = 0;
  const release = subscribeListingOperationView(copied, () => {
    calls++;
  });
  closeListingOperationView(copied);
  release();
  subscribeListingSigning({} as ListingSigning, () => {
    calls++;
  })();
  expect(calls).toBe(0);
});
it('HCP114 SSR rejects copied public evidence and retains calm unknown copy', () => {
  const html = render(OperationStatus, {
    props: { publicOperation: {} as ListingOperationView }
  }).body;
  expect(html).toContain('The outcome is unknown');
  expect(html).not.toContain('Relay acceptance recorded');
  expect(html).not.toMatch(/globally published|editable|<button/);
});
