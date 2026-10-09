import { describe, expect, it } from 'vitest';
import {
  captureProductEnquiry,
  productEnquirySnapshot,
  productEnquiryMessage,
  updateProductEnquiry,
  reuseProductEnquiryConversation,
  closeProductEnquiry,
  type ProductEnquiry
} from '../../src/lib/messaging/product-enquiry.ts';
import type { ProductView } from '../../src/lib/catalog/product-view.ts';
import type { InboxView } from '../../src/lib/messaging/inbox-view.ts';

describe('product enquiry does not borrow detached public or private authority', () => {
  it('SSR cannot capture a product enquiry from a scalar or copied view', () => {
    expect(
      captureProductEnquiry({} as ProductView, 'reviewed_product_enquiry')
    ).toBeUndefined();
  });
  it('unreviewed capture cannot allocate a private draft', () => {
    expect(captureProductEnquiry({} as ProductView, undefined)).toBeUndefined();
  });
  it('a copied enquiry cannot expose private text or a recipient', () => {
    expect(productEnquirySnapshot({} as ProductEnquiry)).toBeUndefined();
  });
  it('a copied enquiry cannot build an initial private message', () => {
    expect(productEnquiryMessage({} as ProductEnquiry)).toBeUndefined();
  });
  it('a fake draft cannot store text or acquire Send authority', () => {
    expect(
      updateProductEnquiry(
        {} as ProductEnquiry,
        'Send to another key',
        'reviewed_product_enquiry_edit'
      )
    ).toBe(false);
  });
  it('a fake product or inbox cannot allocate an arbitrary peer conversation', async () => {
    expect(
      await reuseProductEnquiryConversation(
        {} as ProductEnquiry,
        {} as InboxView,
        'reviewed_product_enquiry_conversation'
      )
    ).toBeUndefined();
  });
  it('closing an unknown draft is safe and cannot revive a message', () => {
    const draft = {} as ProductEnquiry;
    closeProductEnquiry(draft);
    expect(productEnquiryMessage(draft)).toBeUndefined();
  });
});
