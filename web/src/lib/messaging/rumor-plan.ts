import { buildCanonicalPairRumor } from '../nostr/rumor-template.ts';
import { messageFromWireParts } from '../contracts/message-v1/index.ts';
import {
  identityMessagingOwnership,
  subscribeIdentityInvalidation,
  type IdentitySession
} from '../runtime/identity-session.ts';
import { buildEnquiryMessage, type EnquiryContext } from './enquiry-context.ts';
declare const rumorBrand: unique symbol;
export type RumorPlan = Readonly<{ readonly [rumorBrand]: true }>;
export type RumorPlanSnapshot = Readonly<{
  owner: string;
  peer: string;
  id: string;
  createdAt: number;
  wire: string;
}>;
type Controller = RumorPlanSnapshot & {
  current(): boolean;
  unsubscribe(): void;
};
const plans = new WeakMap<RumorPlan, Controller>();
// Local memory-only planning capability, not cryptographically admitted inbound
// history, inbox readiness, CAS reservation, mounted view or encryption/Send.
// Incoming replies later require085/092 authenticated seal/rumor/room admission.
function capture(
  session: IdentitySession,
  rawMessage: string,
  observedTime: unknown
): RumorPlan | undefined {
  const ownership = identityMessagingOwnership(session);
  if (!ownership || !ownership.current()) return undefined;
  const rumor = buildCanonicalPairRumor(
    rawMessage,
    ownership.owner,
    observedTime
  );
  if (!rumor || !ownership.current()) return undefined;
  const peer = rumor.tags.find((row) => row[0] === 'p')?.[1];
  if (!peer) return undefined;
  const token = Object.freeze({}) as RumorPlan;
  const unsubscribe = subscribeIdentityInvalidation(session, () => {
    if (!ownership.current()) stopRumorPlan(token);
  });
  plans.set(token, {
    owner: ownership.owner,
    peer,
    id: rumor.id,
    createdAt: rumor.created_at,
    wire: JSON.stringify(rumor),
    current: ownership.current,
    unsubscribe
  });
  return token;
}
export function captureEnquiryRumor(
  session: IdentitySession,
  context: EnquiryContext,
  text: unknown,
  observedTime: unknown
): RumorPlan | undefined {
  const parts = buildEnquiryMessage(context, text);
  const message = parts && messageFromWireParts(JSON.stringify(parts));
  return message
    ? capture(session, JSON.stringify(message), observedTime)
    : undefined;
}
export function captureReplyRumor(
  session: IdentitySession,
  parent: RumorPlan,
  text: unknown,
  observedTime: unknown
): RumorPlan | undefined {
  if (typeof text !== 'string') return undefined;
  const original = rumorPlanSnapshot(parent),
    ownership = identityMessagingOwnership(session);
  if (
    !original ||
    !ownership ||
    !ownership.current() ||
    (ownership.owner !== original.owner && ownership.owner !== original.peer)
  )
    return undefined;
  const peer =
    ownership.owner === original.owner ? original.peer : original.owner;
  // The parent hash comes only from a genuine captured local rumor, never a
  // caller-provided product ID/body/URL/detached snapshot or outer signer.
  return capture(
    session,
    JSON.stringify({
      recipients: [{ public_key: peer, relay_url: null }],
      content: text,
      reply_to: { id: original.id, relays: null },
      subject: null
    }),
    observedTime
  );
}
export function rumorPlanSnapshot(
  plan: RumorPlan
): RumorPlanSnapshot | undefined {
  const value = plans.get(plan);
  if (!value) return undefined;
  if (!value.current()) {
    stopRumorPlan(plan);
    return undefined;
  }
  return {
    owner: value.owner,
    peer: value.peer,
    id: value.id,
    createdAt: value.createdAt,
    wire: value.wire
  };
}
export function rumorEnvelopePlan(plan: RumorPlan) {
  const value = rumorPlanSnapshot(plan);
  return value
    ? [
        { role: 'peer' as const, recipient: value.peer, rumorWire: value.wire },
        {
          role: 'self_archive' as const,
          recipient: value.owner,
          rumorWire: value.wire
        }
      ]
    : undefined;
}
export function stopRumorPlan(plan: RumorPlan): void {
  const value = plans.get(plan);
  plans.delete(plan);
  value?.unsubscribe();
}
