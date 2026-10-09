import {
  privateSessionOwnership,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  pairedDeliveryAcknowledgementSnapshot,
  pairedDeliveryTargetAccepted,
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
import {
  PRIVATE_TRANSPORT_BUDGETS,
  PRIVATE_PUBLICATION_BUDGETS
} from '../config/budgets.ts';
import { newLocalId } from '../private-handles.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from './verified-envelope.ts';
declare const publicationBrand: unique symbol;
export type PrivatePublication = Readonly<{ [publicationBrand]: true }>;
export type PrivateDeliveryRole = 'peer' | 'self_archive';
declare const actionBrand: unique symbol;
export type PrivatePublicationAction = Readonly<{ [actionBrand]: true }>;
type NetworkAttempt = Readonly<{
  actionId: string;
  attempt: number;
  remaining(): number;
  finish(): void;
}>;
type Action = {
  session: PrivateSession;
  receipt: PairedDeliveryAcknowledgement;
  context: PairedDeliveryContext;
  current(): boolean;
  begin(
    role: PrivateDeliveryRole,
    origin: string,
    eventId: string
  ): NetworkAttempt | undefined;
};
const actions = new WeakMap<PrivatePublicationAction, Action>();
function immutablePair(receipt: PairedDeliveryAcknowledgement) {
  const row = pairedDeliveryAcknowledgementSnapshot(receipt);
  return (
    row &&
    JSON.stringify({
      owner: row.owner,
      id: row.id,
      peer: row.peer,
      rumorHash: row.rumorHash,
      createdAt: row.createdAt,
      self: row.self,
      peerArtifact: row.peerArtifact,
      deliveryPlan: row.deliveryPlan
    })
  );
}
// One explicit action has one network meter, including all artifacts/targets.
// SDK identity approval and persisted readback happen before an attempt starts.
export function capturePrivatePublicationAction(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  receipt: PairedDeliveryAcknowledgement,
  context: PairedDeliveryContext,
  review: unknown
): PrivatePublicationAction | undefined {
  if (review !== 'reviewed_private_retry') return undefined;
  const row = pairedDeliveryAcknowledgementSnapshot(receipt),
    original = immutablePair(receipt),
    ownership = privateSessionOwnership(session);
  if (!row || !original || !ownership?.current()) return undefined;
  for (const route of [
    row.deliveryPlan.routes.peer,
    row.deliveryPlan.routes.archive
  ])
    for (const origin of route.targets) {
      if (
        !capturePrivatePublication(
          repository,
          session,
          receipt,
          context,
          route.role,
          origin,
          'reviewed_private_delivery'
        )
      )
        return undefined;
    }
  const actionId = newLocalId();
  if (!actionId) return undefined;
  const counts = new Map<string, number>();
  let used = 0,
    running = false,
    broken = false,
    last = -1;
  function clock() {
    const now = performance.now();
    if (!Number.isFinite(now) || now < 0 || now < last) {
      broken = true;
      return undefined;
    }
    last = now;
    return now;
  }
  function current() {
    return (
      !broken &&
      ownership!.current() &&
      immutablePair(receipt) === original &&
      recheckInboxRoutePlan(
        context.plan,
        context.policy,
        context.own,
        context.other
      ) === 'unchanged'
    );
  }
  const token = Object.freeze({}) as PrivatePublicationAction;
  actions.set(token, {
    session,
    receipt,
    context,
    current,
    begin(role, origin, eventId) {
      if (
        !current() ||
        running ||
        used >= PRIVATE_PUBLICATION_BUDGETS.networkActionMilliseconds
      )
        return undefined;
      const key = JSON.stringify([role, origin, eventId]),
        count = counts.get(key) ?? 0;
      if (count >= PRIVATE_PUBLICATION_BUDGETS.attemptsPerTargetAction)
        return undefined;
      const start = clock();
      if (start === undefined) return undefined;
      counts.set(key, count + 1);
      running = true;
      let finished = false;
      function remaining() {
        const now = clock();
        return finished || now === undefined || !current()
          ? 0
          : Math.max(
              0,
              PRIVATE_PUBLICATION_BUDGETS.networkActionMilliseconds -
                used -
                (now - start!)
            );
      }
      return {
        actionId,
        attempt: count + 1,
        remaining,
        finish() {
          if (finished) return;
          const now = clock();
          finished = true;
          running = false;
          used =
            now === undefined
              ? PRIVATE_PUBLICATION_BUDGETS.networkActionMilliseconds
              : Math.min(
                  PRIVATE_PUBLICATION_BUDGETS.networkActionMilliseconds,
                  used + now - start
                );
        }
      };
    }
  });
  return token;
}
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
  action?: Action;
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
  review: unknown,
  action?: PrivatePublicationAction
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
      saved = pairedDeliveryAcknowledgementSnapshot(receipt),
      budget = action && actions.get(action);
    if (
      action &&
      (!budget ||
        budget.session !== session ||
        budget.receipt !== receipt ||
        budget.context !== context ||
        !budget.current())
    )
      return undefined;
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
      ...(budget ? { action: budget } : {}),
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
    if (
      saved.action &&
      pairedDeliveryTargetAccepted(
        saved.receipt,
        saved.role,
        saved.origin,
        saved.eventId
      )
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
      repository: saved.repository,
      // Frozen original operation/artifact scope for metadata-only settlement.
      // It survives session loss without becoming new effect permission.
      pairWire: (() => {
        const row = pairedDeliveryAcknowledgementSnapshot(saved.receipt)!;
        return JSON.stringify({
          owner: row.owner,
          id: row.id,
          peer: row.peer,
          rumorHash: row.rumorHash,
          createdAt: row.createdAt,
          self: row.self,
          peerArtifact: row.peerArtifact
        });
      })(),
      current: () => saved.current(),
      owner: saved.owner,
      command: saved.command,
      role: saved.role,
      origin: saved.origin,
      eventId: saved.eventId,
      beginNetwork: () =>
        saved.action
          ? saved.action.begin(saved.role, saved.origin, saved.eventId)
          : undefined,
      networkMetered: !!saved.action
    };
  } catch {
    return undefined;
  }
}
