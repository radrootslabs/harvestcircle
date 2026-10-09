import {
  privateSessionOwnership,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  pairedDeliveryAcknowledgementSnapshot,
  verifyPairedDeliveryAcknowledgement,
  type PairedDeliveryAcknowledgement
} from '../persistence/private-sends.ts';
import type { PrivateStorageRepository } from '../persistence/private-storage.ts';
import type { PairedDeliveryContext } from '../messaging/prepare-send.ts';
import {
  inboxRoutePlanSnapshot,
  recheckInboxRoutePlan
} from '../messaging/inbox-routing.ts';
import { readRelayPolicy, type RelayPolicy } from '../config/relays.ts';
import { PRIVATE_TRANSPORT_BUDGETS } from '../config/budgets.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from './verified-envelope.ts';
declare const publicationBrand: unique symbol;
export type PrivatePublication = Readonly<{ [publicationBrand]: true }>;
export type PrivateDeliveryRole = 'peer' | 'self_archive';
type Admission = {
  session: PrivateSession;
  policy: RelayPolicy;
  owner: string;
  command: string;
  role: PrivateDeliveryRole;
  origin: string;
  eventId: string;
  repository: PrivateStorageRepository;
  receipt: PairedDeliveryAcknowledgement;
  wire: string;
  destination: string;
  current(): boolean;
  take(): boolean;
};
const permissions = new WeakMap<PrivatePublication, Admission>();
// Genuine current local pair custody plus its frozen routes is necessary, but
// this capture opens no socket and does not claim recipient delivery. Each
// permission is one effect, not arbitrary bytes, retry or rerouting authority.
export function capturePrivatePublication(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  receipt: PairedDeliveryAcknowledgement,
  context: PairedDeliveryContext,
  role: PrivateDeliveryRole,
  origin: string,
  review: unknown
): PrivatePublication | undefined {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_private_delivery' ||
    (role !== 'peer' && role !== 'self_archive') ||
    typeof origin !== 'string'
  )
    return undefined;
  try {
    const ownership = privateSessionOwnership(session),
      saved = pairedDeliveryAcknowledgementSnapshot(receipt);
    if (!ownership || !saved || ownership.owner !== saved.owner)
      return undefined;
    const policy = context.policy,
      plan = context.plan,
      own = context.own,
      other = context.other;
    const routes = inboxRoutePlanSnapshot(plan),
      manifest = readRelayPolicy(policy);
    if (
      !routes ||
      !manifest.messagingEnabled ||
      saved.deliveryPlan.state !== 'prepared' ||
      JSON.stringify(routes) !== JSON.stringify(saved.deliveryPlan.routes) ||
      recheckInboxRoutePlan(plan, policy, own, other) !== 'unchanged'
    )
      return undefined;
    const route = role === 'peer' ? routes.peer : routes.archive;
    const artifact = role === 'peer' ? saved.peerArtifact : saved.self;
    const destination = role === 'peer' ? saved.peer : saved.owner;
    if (
      route.role !== role ||
      route.author !== destination ||
      !route.targets.includes(origin) ||
      !manifest.inbox.some(
        ({ origin: allowed, write }) => allowed === origin && write
      ) ||
      !boundedUtf8(artifact.wire, PRIVATE_TRANSPORT_BUDGETS.envelopeBytes)
    )
      return undefined;
    const verified = verifyEnvelope(artifact.wire),
      event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
    if (
      !event ||
      event.kind !== 1059 ||
      event.id !== artifact.eventId ||
      event.tags.length !== 1 ||
      event.tags[0].length !== 2 ||
      event.tags[0][0] !== 'p' ||
      event.tags[0][1] !== destination
    )
      return undefined;
    const original = JSON.stringify(saved);
    let consumed = false;
    function current() {
      const fresh = pairedDeliveryAcknowledgementSnapshot(receipt),
        captured = privateSessionOwnership(session);
      return (
        ownership!.current() &&
        captured?.session === ownership!.session &&
        captured.owner === ownership!.owner &&
        !!fresh &&
        JSON.stringify(fresh) === original &&
        recheckInboxRoutePlan(plan, policy, own, other) === 'unchanged'
      );
    }
    if (!current()) return undefined;
    const token = Object.freeze({}) as PrivatePublication;
    permissions.set(token, {
      session,
      policy,
      owner: saved.owner,
      command: saved.id,
      role,
      origin,
      eventId: event.id,
      repository,
      receipt,
      wire: artifact.wire,
      destination,
      current,
      take() {
        if (consumed) return false;
        consumed = true;
        return true;
      }
    });
    return token;
  } catch {
    return undefined;
  }
}
export function privatePublicationSnapshot(token: PrivatePublication) {
  const saved = permissions.get(token);
  return saved?.current()
    ? {
        owner: saved.owner,
        command: saved.command,
        role: saved.role,
        origin: saved.origin,
        eventId: saved.eventId
      }
    : undefined;
}
// The owned private pool consumes this after fresh SDK identity observation and
// under the owner lock. Actual full persisted record readback precedes every
// EVENT. A detached valid signature or guessed local ID cannot mint permission.
export async function takePrivatePublication(
  token: PrivatePublication,
  session: PrivateSession,
  policy: RelayPolicy,
  origin: string
) {
  const saved = permissions.get(token);
  if (
    !saved ||
    saved.session !== session ||
    saved.policy !== policy ||
    saved.origin !== origin ||
    !saved.current() ||
    !saved.take()
  )
    return undefined;
  try {
    if (
      !(await verifyPairedDeliveryAcknowledgement(
        saved.repository,
        saved.receipt
      )) ||
      !saved.current()
    )
      return undefined;
    const verified = verifyEnvelope(saved.wire),
      event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
    if (
      !event ||
      event.kind !== 1059 ||
      event.id !== saved.eventId ||
      event.tags.length !== 1 ||
      event.tags[0].length !== 2 ||
      event.tags[0][0] !== 'p' ||
      event.tags[0][1] !== saved.destination ||
      !saved.current()
    )
      return undefined;
    return {
      event,
      current: () => saved.current(),
      owner: saved.owner,
      command: saved.command,
      role: saved.role,
      origin: saved.origin,
      eventId: saved.eventId
    };
  } catch {
    return undefined;
  }
}
