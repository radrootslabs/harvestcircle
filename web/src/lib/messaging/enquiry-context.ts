import {
  enquiryReferences,
  type EnquiryReferences
} from '../nostr/enquiry-references.ts';
import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../nostr/verified-envelope.ts';
import { projectFoodParts } from '../contracts/food-availability-v1/read.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { buildUnsignedMessageTemplate } from '../nostr/message-template.ts';
import type { MessageWireParts } from '../contracts/message-v1/index.ts';
declare const enquiryBrand: unique symbol;
export type EnquiryContext = Readonly<{ readonly [enquiryBrand]: true }>;
export type EnquiryContextSnapshot = EnquiryReferences &
  Readonly<{
    title: string;
    advertisedPrice: string;
    status: 'active' | 'sold';
    claims: 'signed_advertisement_assertions';
  }>;
const owners = new WeakMap<EnquiryContext, EnquiryContextSnapshot>();
// This immutable observation is not current listing availability, account,
// room, inbox, encryption or Send authority. Those gates have separate owners.
export function captureEnquiryContext(
  proof: VerifiedEnvelope
): EnquiryContext | undefined {
  const event = verifiedEnvelopeSnapshot(proof),
    refs = enquiryReferences(proof);
  if (!event || !refs) return undefined;
  const food = projectFoodParts(event);
  if (
    food.outcome !== 'focused' ||
    food.projection.identifier !== refs.identifier
  )
    return undefined;
  const token = Object.freeze({}) as EnquiryContext;
  owners.set(token, {
    ...refs,
    title: food.projection.title,
    advertisedPrice: `${food.projection.price.amount} ${food.projection.price.currency}/${food.projection.price.unit}`,
    status: food.projection.status,
    claims: 'signed_advertisement_assertions'
  });
  return token;
}
export function enquiryContextSnapshot(
  context: EnquiryContext
): EnquiryContextSnapshot | undefined {
  const owner = owners.get(context);
  return owner ? { ...owner } : undefined;
}
export function buildEnquiryMessage(
  context: EnquiryContext,
  text: unknown
): MessageWireParts | undefined {
  const owner = owners.get(context);
  if (
    !owner ||
    owner.status !== 'active' ||
    typeof text !== 'string' ||
    !boundedUtf8(text, 4096)
  )
    return undefined;
  const seed = `Enquiry about ${JSON.stringify(owner.title)}\nProduct: nostr:${owner.naddr}\nViewed listing: nostr:${owner.nevent}\nAdvertised price (seller assertion): ${owner.advertisedPrice}\nThis is a message, not a confirmed order.`;
  const content = seed + (text.length ? '\n\n' + text : '');
  return buildUnsignedMessageTemplate(
    JSON.stringify({
      recipients: [{ public_key: owner.peer, relay_url: null }],
      content,
      reply_to: null,
      subject: null
    })
  );
}
export type EnquiryCitationAssessment =
  | Readonly<{
      outcome: 'unresolved';
      reason:
        | 'missing_history'
        | 'different_publisher'
        | 'different_version'
        | 'unsupported_listing';
    }>
  | Readonly<{
      outcome: 'matched_signed_advertisement';
      peer: string;
      eventId: string;
    }>;
// The caller supplies independently verified public history. No arbitrary body
// parser, URL fetch, private parent search or business-state mutation occurs.
export function assessEnquiryCitation(
  context: EnquiryContext,
  proof: VerifiedEnvelope | undefined
): EnquiryCitationAssessment | undefined {
  const owner = owners.get(context);
  if (!owner) return undefined;
  const event = proof ? verifiedEnvelopeSnapshot(proof) : undefined;
  if (!event) return { outcome: 'unresolved', reason: 'missing_history' };
  if (event.kind !== 30402)
    return { outcome: 'unresolved', reason: 'unsupported_listing' };
  if (event.pubkey !== owner.peer)
    return { outcome: 'unresolved', reason: 'different_publisher' };
  if (event.id !== owner.eventId)
    return { outcome: 'unresolved', reason: 'different_version' };
  const food = projectFoodParts(event);
  if (
    food.outcome !== 'focused' ||
    food.projection.identifier !== owner.identifier
  )
    return { outcome: 'unresolved', reason: 'unsupported_listing' };
  return {
    outcome: 'matched_signed_advertisement',
    peer: owner.peer,
    eventId: owner.eventId
  };
}
