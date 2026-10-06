import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../nostr/verified-envelope.ts';
import {
  canonicalDeletionCoordinate,
  deletionRequestSnapshot,
  type DeletionRequest
} from '../nostr/deletion-adapter.ts';
import { publicHeadEnvelope, type PublicHead } from './heads.ts';

export type DeletionDecision = Readonly<{
  outcome: 'visible' | 'suppressed';
  reason:
    | 'deletion_request_immune'
    | 'deletion_no_authorized_reference'
    | 'deletion_request_author_mismatch'
    | 'deletion_address_cutoff_precedes_target'
    | 'deletion_event_id_reference'
    | 'deletion_address_reference'
    | 'deletion_event_id_and_address_reference';
  eventReference: Readonly<{ requestId: string }> | null;
  addressReference: Readonly<{
    coordinate: string;
    inclusiveCutoff: number;
    requestId: string;
  }> | null;
}>;
// Pure reduction over caller-retained admitted proofs. Storage admission and
// coherent retention/eviction remain working-set duties; no implicit cache here.
export function evaluateDeletion(
  target: VerifiedEnvelope,
  requests: readonly DeletionRequest[]
): DeletionDecision {
  const event = verifiedEnvelopeSnapshot(target);
  if (!event) throw new Error('deletion_target_proof_invalid');
  if (event.kind === 5)
    return {
      outcome: 'visible',
      reason: 'deletion_request_immune',
      eventReference: null,
      addressReference: null
    };
  const replaceable =
    event.kind === 0 ||
    event.kind === 3 ||
    (event.kind >= 10000 && event.kind <= 19999);
  const identifier = replaceable
    ? ''
    : event.kind >= 30000 && event.kind <= 39999
      ? event.tags.find((t) => t[0] === 'd')?.[1]
      : undefined;
  // Unlike generic head ordering, missing/malformed FIRST d is no coordinate.
  const coordinate =
    identifier === undefined
      ? undefined
      : canonicalDeletionCoordinate(
          `${event.kind}:${event.pubkey}:${identifier}`
        );
  let eventReference: DeletionDecision['eventReference'] = null;
  let addressReference: DeletionDecision['addressReference'] = null;
  let unauthorized = false;
  for (const token of requests) {
    const request = deletionRequestSnapshot(token);
    if (!request) continue;
    const exact = request.eventTargets.some((v) => v.eventId === event.id);
    const address =
      coordinate !== undefined &&
      request.addressTargets.some((v) => v.coordinate === coordinate);
    if (!exact && !address) continue;
    if (request.pubkey !== event.pubkey) {
      unauthorized = true;
      continue;
    }
    if (
      exact &&
      (eventReference === null || request.id < eventReference.requestId)
    )
      eventReference = { requestId: request.id };
    if (
      address &&
      (addressReference === null ||
        request.created_at > addressReference.inclusiveCutoff ||
        (request.created_at === addressReference.inclusiveCutoff &&
          request.id < addressReference.requestId))
    )
      addressReference = {
        coordinate,
        inclusiveCutoff: request.created_at,
        requestId: request.id
      };
  }
  const addressApplies =
    addressReference !== null &&
    event.created_at <= addressReference.inclusiveCutoff;
  const reason: DeletionDecision['reason'] =
    eventReference !== null
      ? addressApplies
        ? 'deletion_event_id_and_address_reference'
        : 'deletion_event_id_reference'
      : addressApplies
        ? 'deletion_address_reference'
        : addressReference !== null
          ? 'deletion_address_cutoff_precedes_target'
          : unauthorized
            ? 'deletion_request_author_mismatch'
            : 'deletion_no_authorized_reference';
  return {
    outcome:
      eventReference !== null || addressApplies ? 'suppressed' : 'visible',
    reason,
    eventReference,
    addressReference
  };
}
// Evaluate the actual selected winner, never an older focused search match.
// Suppression retains both the winner and request evidence; it claims no erase.
export function evaluatePublicHeadDeletion(
  head: PublicHead,
  requests: readonly DeletionRequest[]
): DeletionDecision {
  return evaluateDeletion(publicHeadEnvelope(head), requests);
}
