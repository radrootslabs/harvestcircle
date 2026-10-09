import {
  capturePairedRoutingReview,
  pairedRoutingReviewSnapshot,
  approvePairedRoutingReview,
  type PairedRoutingReview,
  type PairedDeliveryAcknowledgement
} from '../persistence/private-sends.ts';
import type { PrivateStorageRepository } from '../persistence/private-storage.ts';
import type { IdentitySession } from '../runtime/identity-session.ts';
import type { PairedDeliveryContext } from './prepare-send.ts';
export type PrivateResumeRouting = PairedRoutingReview;
// Sample actual current discovery owners and a qualified policy. This review
// exposes exact destination/head changes and performs no automatic publication.
// Explicit consent changes only local routes of the same immutable artifacts;
// the separate bounded retry workflow owns every subsequent network attempt.
export function capturePrivateResumeRouting(
  repository: PrivateStorageRepository,
  session: IdentitySession,
  receipt: PairedDeliveryAcknowledgement,
  current: Pick<PairedDeliveryContext, 'policy' | 'own' | 'other'>,
  review: unknown
): PrivateResumeRouting | undefined {
  return capturePairedRoutingReview(
    repository,
    session,
    receipt,
    current,
    review
  );
}
export const privateResumeRoutingSnapshot = pairedRoutingReviewSnapshot;
export const approvePrivateResumeRouting = approvePairedRoutingReview;
