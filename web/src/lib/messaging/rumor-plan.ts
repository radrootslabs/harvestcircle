import { buildCanonicalPairRumor } from '../nostr/rumor-template.ts';
import { messageFromWireParts } from '../contracts/message-v1/index.ts';
import {
  identityMessagingOwnership,
  subscribeIdentityInvalidation,
  type IdentitySession
} from '../runtime/identity-session.ts';
import { buildEnquiryMessage, type EnquiryContext } from './enquiry-context.ts';
import {
  recoveredSelfEnvelopeSnapshot,
  type RecoveredSelfEnvelope
} from '../nostr/self-recovery-reader.ts';
import {
  privateSessionOwnership,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  unlockedSessionOwnership,
  unlockedConversationCurrent,
  type UnlockedSession
} from './unlocked-session.ts';
import {
  conversationSnapshot,
  conversationOwnership,
  type AdmittedConversation
} from './admit-conversation.ts';
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
  observedTime: unknown,
  additionalCurrent: () => boolean = () => true
): RumorPlan | undefined {
  const ownership = identityMessagingOwnership(session);
  if (!ownership || !ownership.current() || !additionalCurrent())
    return undefined;
  const rumor = buildCanonicalPairRumor(
    rawMessage,
    ownership.owner,
    observedTime
  );
  if (!rumor || !ownership.current() || !additionalCurrent()) return undefined;
  const peer = rumor.tags.find((row) => row[0] === 'p')?.[1];
  if (!peer) return undefined;
  const token = Object.freeze({}) as RumorPlan;
  const unsubscribe = subscribeIdentityInvalidation(session, () => {
    if (!ownership.current() || !additionalCurrent()) stopRumorPlan(token);
  });
  plans.set(token, {
    owner: ownership.owner,
    peer,
    id: rumor.id,
    createdAt: rumor.created_at,
    wire: JSON.stringify(rumor),
    current: () => ownership.current() && additionalCurrent(),
    unsubscribe
  });
  return token;
}
// Reply authority comes from the original authenticated room and its exact
// private/cache generation. A detached snapshot, parent hash or outer author
// cannot select the peer or borrow a same-owner session.
export function captureAuthenticatedReplyRumor(
  identity: IdentitySession,
  session: PrivateSession,
  unlocked: UnlockedSession,
  room: AdmittedConversation,
  text: unknown,
  observedTime: unknown
): RumorPlan | undefined {
  const owner = identityMessagingOwnership(identity),
    privateOwner = privateSessionOwnership(session),
    cache = unlockedSessionOwnership(unlocked),
    admitted = conversationOwnership(room),
    data = conversationSnapshot(room);
  if (
    !owner ||
    !privateOwner ||
    !cache ||
    !admitted ||
    !data ||
    typeof text !== 'string'
  )
    return;
  const current = () => {
    const actual = conversationSnapshot(room);
    return (
      owner.current() &&
      privateOwner.current() &&
      cache.current() &&
      admitted.current() &&
      owner.owner === data.owner &&
      privateOwner.owner === owner.owner &&
      privateOwner.session === owner.session &&
      cache.owner === owner.owner &&
      cache.session === owner.session &&
      admitted.owner === owner.owner &&
      admitted.session === owner.session &&
      actual?.owner === data.owner &&
      actual.peer === data.peer &&
      actual.rumorId === data.rumorId &&
      unlockedConversationCurrent(unlocked, room)
    );
  };
  if (!current()) return;
  return capture(
    identity,
    JSON.stringify({
      recipients: [{ public_key: data.peer, relay_url: null }],
      content: text,
      reply_to: { id: data.rumorId, relays: null },
      subject: null
    }),
    observedTime,
    current
  );
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
// Historical time/hash/wire are admitted only by actual authenticated SDK
// recovery. This creates no new intent, reservation, event or persistent body.
export function restoreRecoveredRumor(
  session: IdentitySession,
  recovered: RecoveredSelfEnvelope
): RumorPlan | undefined {
  const ownership = identityMessagingOwnership(session),
    snapshot = recoveredSelfEnvelopeSnapshot(recovered);
  if (
    !ownership?.current() ||
    !snapshot ||
    snapshot.record.owner !== ownership.owner
  )
    return undefined;
  const token = Object.freeze({}) as RumorPlan;
  const current = () =>
    ownership.current() && !!recoveredSelfEnvelopeSnapshot(recovered);
  const unsubscribe = subscribeIdentityInvalidation(session, () => {
    if (!current()) stopRumorPlan(token);
  });
  plans.set(token, {
    owner: snapshot.record.owner,
    peer: snapshot.record.peer,
    id: snapshot.record.rumorHash,
    createdAt: snapshot.record.createdAt,
    wire: snapshot.rumorWire,
    current,
    unsubscribe
  });
  return current() ? token : undefined;
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
