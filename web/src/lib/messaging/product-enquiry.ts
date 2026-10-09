import {
  productViewListingOwnership,
  type ProductView
} from '../catalog/product-view.ts';
import {
  captureEnquiryContext,
  enquiryContextSnapshot,
  buildEnquiryMessage,
  type EnquiryContextSnapshot,
  type EnquiryContext
} from './enquiry-context.ts';
import { openInboxConversation, type InboxView } from './inbox-view.ts';
import type { MessageWireParts } from '../contracts/message-v1/index.ts';

declare const enquiryBrand: unique symbol;
export type ProductEnquiry = Readonly<{ readonly [enquiryBrand]: true }>;
type Snapshot = EnquiryContextSnapshot &
  Readonly<{
    text: string;
    revision: number;
    memoryOnly: true;
    saved: false;
    listingCurrent: boolean;
  }>;
type Owner = {
  ownership():
    Readonly<{ context: EnquiryContext; current(): boolean }> | undefined;
  snapshot(): Snapshot | undefined;
  message(): MessageWireParts | undefined;
  update(text: unknown, review: unknown): boolean;
  reuse(view: InboxView, review: unknown): Promise<string | undefined>;
  close(): void;
};
const owners = new WeakMap<ProductEnquiry, Owner>();

// A guest may capture and edit a memory-only draft. This is neither identity,
// inbox readiness nor Send authority, and performs no network/native/SDK call.
export function captureProductEnquiry(
  view: ProductView,
  review: unknown
): ProductEnquiry | undefined {
  if (typeof window === 'undefined' || review !== 'reviewed_product_enquiry')
    return;
  const listing = productViewListingOwnership(view);
  if (!listing?.current()) return;
  const context = captureEnquiryContext(listing.proof);
  const observed = context && enquiryContextSnapshot(context);
  if (!context || !observed || observed.status !== 'active') return;
  let active = true,
    revision = 0,
    text = `Hello, I am interested in ${JSON.stringify(observed.title)}. Is it available?`;
  if (!buildEnquiryMessage(context, text)) return;
  function close() {
    active = false;
    text = '';
  }
  function current() {
    if (!active || !listing!.available()) {
      close();
      return false;
    }
    return true;
  }
  const token = Object.freeze({}) as ProductEnquiry;
  owners.set(token, {
    ownership: () =>
      current()
        ? { context, current: () => current() && listing.current() }
        : undefined,
    snapshot() {
      return current()
        ? {
            ...observed,
            text,
            revision,
            memoryOnly: true,
            saved: false,
            listingCurrent: listing.current()
          }
        : undefined;
    },
    message() {
      return current() && listing.current()
        ? buildEnquiryMessage(context, text)
        : undefined;
    },
    update(value, permission) {
      if (
        permission !== 'reviewed_product_enquiry_edit' ||
        !current() ||
        typeof value !== 'string' ||
        !buildEnquiryMessage(context, value)
      )
        return false;
      text = value;
      revision++;
      return true;
    },
    async reuse(inbox, permission) {
      if (
        permission !== 'reviewed_product_enquiry_conversation' ||
        !current() ||
        !listing.current()
      )
        return;
      const path = await openInboxConversation(
        inbox,
        observed.peer,
        'reviewed_admitted_conversation_navigation'
      );
      return current() && listing.current() ? path : undefined;
    },
    close
  });
  return token;
}
export function productEnquirySnapshot(
  token: ProductEnquiry
): Snapshot | undefined {
  return owners.get(token)?.snapshot();
}
export function productEnquiryMessage(
  token: ProductEnquiry
): MessageWireParts | undefined {
  return owners.get(token)?.message();
}
export function updateProductEnquiry(
  token: ProductEnquiry,
  text: unknown,
  review: unknown
): boolean {
  return owners.get(token)?.update(text, review) ?? false;
}
export function reuseProductEnquiryConversation(
  token: ProductEnquiry,
  view: InboxView,
  review: unknown
): Promise<string | undefined> {
  return owners.get(token)?.reuse(view, review) ?? Promise.resolve(undefined);
}
export function closeProductEnquiry(token: ProductEnquiry): void {
  owners.get(token)?.close();
}
export function productEnquirySendOwnership(token: ProductEnquiry) {
  return owners.get(token)?.ownership();
}
