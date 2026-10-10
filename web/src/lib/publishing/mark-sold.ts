import { reviewOwnedListing } from './prepare-listing.ts';
import type { OwnedEditSource } from './edit-source.ts';
import type { PublicDraftController } from './draft-controller.ts';
// Sold changes public availability only; no payment/reservation record exists.
export function reviewMarkSold(
  source: OwnedEditSource,
  controller: PublicDraftController,
  command: unknown,
  consent: unknown
) {
  return reviewOwnedListing(source, controller, command, 'sold', consent);
}
