import {
  productViewSnapshot,
  productViewListingOwnership,
  type ProductView,
  type ProductStatusAcknowledgement
} from '../catalog/product-view.ts';
import {
  identitySessionSnapshot,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  inboxResolutionSnapshot,
  inboxResolverCurrentAfterSample,
  type InboxResolver
} from './resolve-inbox.ts';
import { inboxRelayTargets, type RelayPolicy } from '../config/relays.ts';
import { inboxPreferenceSnapshot } from '../nostr/inbox-preferences.ts';

export type SellerInboxStatus =
  'unchecked' | 'compatible' | 'missing' | 'unsupported' | 'inconclusive';
// Informational observations of original owners, never Send or route authority.
export function sellerInboxStatus(
  resolver: InboxResolver | undefined,
  author: string,
  policy?: RelayPolicy
): SellerInboxStatus {
  if (!resolver) return 'unchecked';
  try {
    const observed = inboxResolutionSnapshot(resolver);
    if (
      observed.author !== author ||
      !inboxResolverCurrentAfterSample(resolver)
    )
      return 'inconclusive';
    if (observed.status === 'ready' && policy) {
      const preference =
        observed.head && inboxPreferenceSnapshot(observed.head);
      if (
        !preference ||
        inboxRelayTargets(policy, preference.relays, 'write').length === 0
      )
        return 'unsupported';
    }
    return observed.status === 'ready' ? 'compatible' : observed.status;
  } catch {
    return 'inconclusive';
  }
}
export function sellerInboxCopy(status: SellerInboxStatus): string {
  return status === 'missing'
    ? 'In-app messaging is not available for this seller in this prototype.'
    : status === 'unsupported'
      ? 'The seller inbox configuration is unsupported in this prototype.'
      : status === 'inconclusive'
        ? 'The seller inbox could not be confirmed from the sources checked.'
        : '';
}
export function enquiryReadinessSnapshot(
  view: ProductView,
  identity?: IdentitySession,
  acknowledgement?: ProductStatusAcknowledgement,
  resolver?: InboxResolver
): Readonly<{
  action:
    | 'unavailable'
    | 'sold'
    | 'withdrawn'
    | 'own'
    | 'acknowledgement_required'
    | 'available';
  naddr?: string;
  sellerInbox: SellerInboxStatus;
}> {
  const blocked = {
    action: 'unavailable' as const,
    sellerInbox: 'unchecked' as const
  };
  try {
    const observed = productViewSnapshot(view);
    const actor = identity && identitySessionSnapshot(identity);
    const inbox = sellerInboxStatus(resolver, observed.reference.pubkey);
    const base = { naddr: observed.reference.naddr, sellerInbox: inbox };
    if (
      observed.eventId &&
      actor &&
      'publicKey' in actor &&
      actor.publicKey === observed.reference.pubkey
    )
      return { ...base, action: 'own' };
    if (observed.outcome === 'sold' || observed.outcome === 'withdrawn')
      return { ...base, action: observed.outcome };
    if (observed.outcome !== 'active')
      return { ...base, action: 'unavailable' };
    const listing = productViewListingOwnership(view, acknowledgement);
    return {
      ...base,
      action: listing?.current()
        ? 'available'
        : observed.lastKnown
          ? 'acknowledgement_required'
          : 'unavailable'
    };
  } catch {
    return blocked;
  }
}
