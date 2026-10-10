import type { OwnedEditSource } from './edit-source.ts';
import type { PublicDraftController } from './draft-controller.ts';
import { reviewOwnedListing } from './prepare-listing.ts';

// Original retained-source custody is mandatory. A copied public event or a
// display snapshot cannot authorize a removal request.
export function reviewOwnedWithdrawal(
  source: OwnedEditSource,
  controller: PublicDraftController,
  command: unknown,
  consent: unknown
) {
  return reviewOwnedListing(source, controller, command, 'withdrawal', consent);
}
