import { describe, expect, it } from 'vitest';
import {
  acknowledgeProductViewUncertainty,
  productViewListingOwnership,
  type ProductView,
  type ProductStatusAcknowledgement
} from '../../src/lib/catalog/product-view.ts';
import { enquiryReadinessSnapshot } from '../../src/lib/messaging/enquiry-readiness.ts';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';
import type { InboxResolver } from '../../src/lib/messaging/resolve-inbox.ts';

describe('enquiry availability uses original evidence owners', () => {
  it('copied view and a review string cannot acknowledge last-known status', () => {
    expect(
      acknowledgeProductViewUncertainty(
        {} as ProductView,
        'reviewed_last_known_listing'
      )
    ).toBeUndefined();
  });
  it('a copied acknowledgement cannot manufacture a listing witness', () => {
    expect(
      productViewListingOwnership(
        {} as ProductView,
        {} as ProductStatusAcknowledgement
      )
    ).toBeUndefined();
  });
  it('displayed identity, food and inbox copies cannot create readiness', () => {
    expect(
      enquiryReadinessSnapshot(
        {} as ProductView,
        {} as IdentitySession,
        {} as ProductStatusAcknowledgement,
        {} as InboxResolver
      )
    ).toMatchObject({ action: 'unavailable', sellerInbox: 'unchecked' });
  });
});
