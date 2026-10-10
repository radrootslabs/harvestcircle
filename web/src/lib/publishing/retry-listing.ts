import type { RelayPolicy } from '../config/relays.ts';
import type { PublicPool } from '../nostr/public-pool.ts';
import {
  listingPublicationOwnership,
  type ListingPublication
} from './sign-listing.ts';
import {
  listingResumeOwnership,
  type ListingResumption
} from './resume-listing.ts';
import {
  deliverControlledListing,
  type ListingDeliveryResult
} from './deliver-listing.ts';

declare const retryBrand: unique symbol;
export type ListingRetry = Readonly<{ [retryBrand]: true }>;
export type ListingRetrySource = ListingPublication | ListingResumption;
type Control = Readonly<{
  source: ListingRetrySource;
  pool: PublicPool;
  policy: RelayPolicy;
  consent: 'retry_stored_listing' | 'resume_stored_listing';
  controller: AbortController;
  result: Promise<ListingDeliveryResult>;
}>;
const controls = new WeakMap<ListingRetry, Control>();
const pending = new WeakMap<ListingRetrySource, ListingRetry>();
// Internal delivery custody is granted only by the original control object.
// A copied disclosure cannot provide a signal or cancel somebody else's job.
export function listingRetryOwnership(token: ListingRetry) {
  const saved = controls.get(token);
  return (
    saved && {
      source: saved.source,
      pool: saved.pool,
      policy: saved.policy,
      consent: saved.consent,
      signal: saved.controller.signal,
      current: () => !saved.controller.signal.aborted
    }
  );
}
export function beginListingRetry(
  source: ListingRetrySource,
  pool: PublicPool,
  policy: RelayPolicy,
  consent: unknown
): ListingRetry | undefined {
  const publication = listingPublicationOwnership(source as ListingPublication),
    resume = listingResumeOwnership(source as ListingResumption),
    original = publication ?? resume;
  if (
    !original ||
    original.policy !== policy ||
    consent !==
      (publication ? 'retry_stored_listing' : 'resume_stored_listing') ||
    typeof window === 'undefined'
  )
    return;
  const active = pending.get(source);
  if (active) return active;
  const token = Object.freeze({}) as ListingRetry,
    controller = new AbortController();
  const result = Promise.resolve()
    .then(() => deliverControlledListing(token))
    .then((value): ListingDeliveryResult => ({
      status:
        value.status === 'storage_failed' || value.status === 'unavailable'
          ? 'needs_action'
          : value.status
    }))
    .finally(() => {
      if (pending.get(source) === token) pending.delete(source);
    });
  controls.set(token, {
    source,
    pool,
    policy,
    consent: consent as Control['consent'],
    controller,
    result
  });
  pending.set(source, token);
  return token;
}
export function listingRetryResult(
  token: ListingRetry
): Promise<ListingDeliveryResult> {
  return controls.get(token)?.result ?? Promise.resolve({ status: 'invalid' });
}
export function stopListingRetry(token: ListingRetry): void {
  controls.get(token)?.controller.abort();
}
