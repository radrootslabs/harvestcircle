import { describe, expect, it } from 'vitest';
import {
  createProductRequestController,
  productRequestSnapshot,
  productRequestText,
  updateProductRequestText,
  configureProductRequest,
  sendProductRequest,
  requestProductRequestNavigation,
  keepProductRequestEditing,
  discardProductRequestNavigation,
  closeProductRequestController,
  type ProductRequestController
} from '../../src/lib/messaging/product-request-controller.ts';
import type { ProductEnquiry } from '../../src/lib/messaging/product-enquiry.ts';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';
import type { InboxSetupView } from '../../src/lib/messaging/inbox-setup-view.ts';

describe('inline product request requires original opaque owners', () => {
  it('SSR and a copied product enquiry cannot allocate a request', () => {
    expect(
      createProductRequestController({} as ProductEnquiry)
    ).toBeUndefined();
  });
  it('a copied request cannot reveal text or readiness', () => {
    expect(
      productRequestSnapshot({} as ProductRequestController)
    ).toBeUndefined();
    expect(productRequestText({} as ProductRequestController)).toBeUndefined();
  });
  it('unreviewed or copied edit has no private storage authority', () => {
    expect(
      updateProductRequestText(
        {} as ProductRequestController,
        'private',
        undefined
      )
    ).toBe(false);
  });
  it('displayed identity and setup copies cannot configure a request', () => {
    expect(
      configureProductRequest(
        {} as ProductRequestController,
        {} as IdentitySession,
        {} as InboxSetupView,
        'reviewed_product_request_setup'
      )
    ).toBe(false);
  });
  it('copied and unreviewed Send cannot reserve or publish', async () => {
    expect(
      await sendProductRequest({} as ProductRequestController, undefined)
    ).toEqual({ status: 'invalid' });
  });
  it('a copied request cannot grant navigation or discard authority', () => {
    expect(
      requestProductRequestNavigation({} as ProductRequestController)
    ).toBe('invalid');
    expect(
      discardProductRequestNavigation(
        {} as ProductRequestController,
        'reviewed_discard_private_text'
      )
    ).toBe('invalid');
  });
  it('keeping a copied request cannot revive its private editor', () => {
    expect(keepProductRequestEditing({} as ProductRequestController)).toBe(
      false
    );
  });
  it('closing an unknown request is safe and never starts Send', () => {
    const copied = {} as ProductRequestController;
    closeProductRequestController(copied);
    expect(productRequestSnapshot(copied)).toBeUndefined();
  });
});
