import { reviewOwnedListing } from './prepare-listing.ts';
import type { OwnedEditSource } from './edit-source.ts';
import type { PublicDraftController } from './draft-controller.ts';
export { ownedRevisionTimestamp as nextOwnedRevisionTime } from './prepare-listing.ts';
// One immutable ListingReview and the existing durable publisher own the effect.
export function reviewOwnedRevision(
  source: OwnedEditSource,
  controller: PublicDraftController,
  command: unknown,
  consent: unknown
) {
  return reviewOwnedListing(source, controller, command, 'active', consent);
}
