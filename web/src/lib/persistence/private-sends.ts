import {
  identityMessagingOwnership,
  recheckIdentityOwner,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  reservedSendSnapshot,
  type ReservedSendIdentity
} from '../messaging/send-identity.ts';
import {
  verifiedOutboundSnapshot,
  verifiedOutboundRecoverySource,
  type VerifiedOutboundEnvelope
} from '../nostr/verify-outbound-envelope.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';
import {
  decodePrivateRecord,
  privateRecordSnapshot,
  privateRecordWire,
  type PrivateRecordHandle,
  type PrivateSendReservation,
  type PrivateSendOperation
} from './private-records.ts';
import {
  inboxRoutePlanSnapshot,
  createInboxRoutePlan,
  recheckInboxRoutePlan,
  type InboxRoutePlan
} from '../messaging/inbox-routing.ts';
import type { PairedDeliveryContext } from '../messaging/prepare-send.ts';
import {
  loadPrivateRecord,
  commitPrivateRecord,
  type PrivateStorageRepository,
  type PrivateStorageFailure,
  type PrivateStorageResult
} from './private-storage.ts';
import {
  preparePrivateReceiptTransition,
  commitPrivateReceiptTransition,
  type PrivateReceiptCommitResult
} from './private-receipts.ts';

declare const acknowledgementBrand: unique symbol;
export type SelfRecoveryAcknowledgement = Readonly<{
  [acknowledgementBrand]: true;
}>;
type Saved = Readonly<{
  record: PrivateSendReservation;
  revision: number;
  wire: string;
  self: Readonly<{ eventId: string; wire: string }>;
  current(): boolean;
}>;
const acknowledgements = new WeakMap<SelfRecoveryAcknowledgement, Saved>();
export type SelfRecoveryCommitResult =
  | Readonly<{
      status: 'saved' | 'existing' | 'reconciled';
      receipt: SelfRecoveryAcknowledgement;
    }>
  | Readonly<{ status: PrivateStorageFailure | 'invalid' | 'stopped' }>;

// A namespace read is not a crypto proof or acknowledgement. Defensive owner,
// command and immutable-rumor matching precedes any future SDK preparation.
export async function readSelfRecoveryBase(
  repository: PrivateStorageRepository,
  reserved: ReservedSendIdentity
): Promise<PrivateStorageResult<PrivateRecordHandle>> {
  const record = reservedSendSnapshot(reserved);
  if (!record) return { ok: false, reason: 'invalid_record' };
  const loaded = await loadPrivateRecord(
    repository,
    'private_sends',
    record.id
  );
  if (!loaded.ok)
    return {
      ok: false,
      reason: loaded.reason === 'invalid_scope' ? 'unavailable' : loaded.reason
    };
  const row = privateRecordSnapshot(loaded.value, record.owner, record.id);
  if (
    !row ||
    row.family === 'received_envelope' ||
    row.peer !== record.peer ||
    row.rumorHash !== record.rumorHash ||
    row.createdAt !== record.createdAt
  )
    return { ok: false, reason: 'invalid_record' };
  if (!reservedSendSnapshot(reserved))
    return { ok: false, reason: 'invalid_record' };
  return loaded;
}

function input(
  session: IdentitySession,
  reserved: ReservedSendIdentity,
  proof: VerifiedOutboundEnvelope
) {
  const record = reservedSendSnapshot(reserved),
    ownership = identityMessagingOwnership(session),
    observed = verifiedOutboundSnapshot(proof);
  if (
    !record ||
    !ownership?.current() ||
    !observed ||
    ownership.owner !== record.owner ||
    observed.role !== 'self' ||
    observed.destination !== record.owner ||
    observed.command !== record.id ||
    observed.owner !== record.owner ||
    observed.peer !== record.peer ||
    observed.rumorHash !== record.rumorHash
  )
    return undefined;
  const verified = verifyEnvelope(observed.wire),
    event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
  if (!event || event.kind !== 1059) return undefined;
  const current = () => {
    const original = reservedSendSnapshot(reserved),
      outer = verifiedOutboundSnapshot(proof);
    return (
      ownership.current() &&
      original?.owner === record.owner &&
      original.id === record.id &&
      original.peer === record.peer &&
      original.rumorHash === record.rumorHash &&
      original.createdAt === record.createdAt &&
      outer?.role === 'self' &&
      outer.destination === record.owner &&
      outer.wire === observed.wire
    );
  };
  return current()
    ? {
        record,
        current,
        ownerCurrent: () => ownership.current(),
        self: { eventId: event.id, wire: observed.wire },
        recoveryWire: verifiedOutboundRecoverySource(proof)
      }
    : undefined;
}

// No signer/network await occurs in an IDB transaction. Only genuine current
// factory association plus acknowledged exact ciphertext readback mints this
// local fact. It grants no relay readiness, pair plan or transmission permission.
export async function commitSelfRecovery(
  repository: PrivateStorageRepository,
  session: IdentitySession,
  reserved: ReservedSendIdentity,
  proof: VerifiedOutboundEnvelope,
  review: unknown
): Promise<SelfRecoveryCommitResult> {
  if (typeof window === 'undefined' || review !== 'reviewed_self_commit')
    return { status: 'invalid' };
  const captured = input(session, reserved, proof);
  if (!captured) return { status: 'invalid' };
  try {
    const loaded = await readSelfRecoveryBase(repository, reserved);
    if (!captured.current()) return { status: 'stopped' };
    if (!loaded.ok) return { status: loaded.reason };
    const base = privateRecordSnapshot(
      loaded.value,
      captured.record.owner,
      captured.record.id
    );
    if (!base || base.family === 'received_envelope')
      return { status: 'invalid_record' };
    if (
      captured.recoveryWire !== undefined &&
      privateRecordWire(
        loaded.value,
        captured.record.owner,
        captured.record.id
      ) !== captured.recoveryWire
    )
      return { status: 'conflict' };
    let expected: PrivateRecordHandle,
      status: 'saved' | 'existing' | 'reconciled';
    if (base.family === 'private_send_operation') {
      if (
        base.self.eventId !== captured.self.eventId ||
        base.self.wire !== captured.self.wire
      )
        return { status: 'conflict' };
      expected = loaded.value;
      status = 'existing';
    } else {
      const raw = JSON.stringify({
        schema: 1,
        family: 'private_send_operation',
        owner: base.owner,
        id: base.id,
        revision: base.revision + 1,
        peer: base.peer,
        rumorHash: base.rumorHash,
        createdAt: base.createdAt,
        self: { eventId: captured.self.eventId, wire: captured.self.wire },
        peerArtifact: null
      });
      const decoded = decodePrivateRecord(raw, base.owner, base.id);
      if (!decoded.ok) return { status: 'invalid_record' };
      expected = decoded.value;
      if (!captured.current()) return { status: 'stopped' };
      const committed = await commitPrivateRecord(
        repository,
        expected,
        loaded.value
      );
      if (!captured.current()) return { status: 'stopped' };
      if (!committed.ok && committed.reason !== 'unknown_completion')
        return { status: committed.reason };
      status = !committed.ok
        ? 'reconciled'
        : committed.value.state === 'existing'
          ? 'existing'
          : 'saved';
    }
    const expectedWire = privateRecordWire(
      expected,
      captured.record.owner,
      captured.record.id
    );
    const readback = await loadPrivateRecord(
      repository,
      'private_sends',
      captured.record.id
    );
    if (!captured.current()) return { status: 'stopped' };
    if (!readback.ok) return { status: 'unknown_completion' };
    const wire = privateRecordWire(
        readback.value,
        captured.record.owner,
        captured.record.id
      ),
      record = privateRecordSnapshot(
        readback.value,
        captured.record.owner,
        captured.record.id
      );
    if (
      !wire ||
      wire !== expectedWire ||
      !record ||
      record.family !== 'private_send_operation' ||
      record.self.eventId !== captured.self.eventId ||
      record.self.wire !== captured.self.wire
    )
      return { status: 'conflict' };
    const receipt = Object.freeze({}) as SelfRecoveryAcknowledgement;
    acknowledgements.set(receipt, {
      record: captured.record,
      revision: record.revision,
      wire,
      self: captured.self,
      // The acknowledged local fact survives clearing the crypto preparation.
      // Effect permission remains separately gated by the live workflow.
      current: captured.ownerCurrent
    });
    return { status, receipt };
  } catch {
    return { status: captured.current() ? 'unavailable' : 'stopped' };
  }
}

export function selfRecoveryAcknowledgementSnapshot(
  receipt: SelfRecoveryAcknowledgement
) {
  const saved = acknowledgements.get(receipt);
  return saved?.current()
    ? {
        owner: saved.record.owner,
        id: saved.record.id,
        peer: saved.record.peer,
        rumorHash: saved.record.rumorHash,
        createdAt: saved.record.createdAt,
        revision: saved.revision,
        self: { eventId: saved.self.eventId, wire: saved.self.wire },
        copy: 'Saved encrypted in this browser' as const,
        fact: 'acknowledged_local_ciphertext_only' as const
      }
    : undefined;
}

// Recheck exact local evidence before a workflow advances. This does not
// promise retention after the read; clearing/eviction and later CAS remain real.
export async function verifySelfRecoveryAcknowledgement(
  repository: PrivateStorageRepository,
  receipt: SelfRecoveryAcknowledgement
): Promise<boolean> {
  const saved = acknowledgements.get(receipt);
  if (!saved?.current()) return false;
  const loaded = await loadPrivateRecord(
    repository,
    'private_sends',
    saved.record.id
  );
  return (
    saved.current() &&
    loaded.ok &&
    privateRecordWire(loaded.value, saved.record.owner, saved.record.id) ===
      saved.wire
  );
}

declare const pairedAcknowledgementBrand: unique symbol;
export type PairedDeliveryAcknowledgement = Readonly<{
  [pairedAcknowledgementBrand]: true;
}>;
type Paired = Readonly<{
  record: PrivateSendOperation;
  wire: string;
  current(): boolean;
}>;
const pairedAcknowledgements = new WeakMap<
  PairedDeliveryAcknowledgement,
  Paired
>();
declare const routingReviewBrand: unique symbol;
export type PairedRoutingReview = Readonly<{ [routingReviewBrand]: true }>;
type RoutingReview = Readonly<{
  repository: PrivateStorageRepository;
  session: IdentitySession;
  receipt: PairedDeliveryAcknowledgement;
  saved: Paired;
  context: PairedDeliveryContext;
  changed: boolean;
  current(): boolean;
}>;
const routingReviewStates = new WeakMap<PairedRoutingReview, RoutingReview>();
const pendingRoutingReviews = new WeakMap<
  PairedDeliveryAcknowledgement,
  PairedRoutingReview
>();
// Genuine current discovery is a reviewed destination candidate, not Send.
// Detection immediately fences previously captured retry/publication custody.
export function capturePairedRoutingReview(
  repository: PrivateStorageRepository,
  session: IdentitySession,
  receipt: PairedDeliveryAcknowledgement,
  candidate: Pick<PairedDeliveryContext, 'policy' | 'own' | 'other'>,
  review: unknown
): PairedRoutingReview | undefined {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_private_route_resolution'
  )
    return undefined;
  const saved = pairedAcknowledgements.get(receipt),
    ownership = identityMessagingOwnership(session);
  if (
    !saved?.current() ||
    !saved.record.deliveryPlan ||
    !saved.record.peerArtifact ||
    !ownership?.current() ||
    ownership.owner !== saved.record.owner
  )
    return undefined;
  // Unready current resolution must also fence the old destination. No
  // fallback capability survives merely because there is no new usable plan.
  const token = Object.freeze({}) as PairedRoutingReview;
  pendingRoutingReviews.set(receipt, token);
  const plan = createInboxRoutePlan(
    candidate.policy,
    saved.record.owner,
    saved.record.peer,
    candidate.own,
    candidate.other
  );
  if (!plan) return undefined;
  const context = {
    plan,
    policy: candidate.policy,
    own: candidate.own,
    other: candidate.other
  };
  const routes = inboxRoutePlanSnapshot(plan);
  if (
    !routes ||
    routes.peer.author !== saved.record.peer ||
    routes.archive.author !== saved.record.owner ||
    recheckInboxRoutePlan(
      context.plan,
      context.policy,
      context.own,
      context.other
    ) !== 'unchanged'
  )
    return undefined;
  const current = () =>
    saved.current() &&
    ownership.current() &&
    pendingRoutingReviews.get(receipt) === token &&
    recheckInboxRoutePlan(
      context.plan,
      context.policy,
      context.own,
      context.other
    ) === 'unchanged';
  routingReviewStates.set(token, {
    repository,
    session,
    receipt,
    saved,
    context,
    changed:
      JSON.stringify(routes) !==
      JSON.stringify(saved.record.deliveryPlan.routes),
    current
  });
  return token;
}
export function pairedRoutingReviewSnapshot(token: PairedRoutingReview) {
  const state = routingReviewStates.get(token);
  if (!state?.current() || !state.saved.record.deliveryPlan) return undefined;
  const previous = state.saved.record.deliveryPlan.routes,
    next = inboxRoutePlanSnapshot(state.context.plan);
  if (!next) return undefined;
  function changes(role: 'peer' | 'archive') {
    return {
      removed: previous[role].targets.filter(
        (origin) => !next![role].targets.includes(origin)
      ),
      added: next![role].targets.filter(
        (origin) => !previous[role].targets.includes(origin)
      )
    };
  }
  return {
    owner: state.saved.record.owner,
    peer: state.saved.record.peer,
    command: state.saved.record.id,
    status: state.changed
      ? ('review_required' as const)
      : ('unchanged' as const),
    previous: JSON.parse(JSON.stringify(previous)) as typeof previous,
    next,
    changes: { peer: changes('peer'), archive: changes('archive') },
    fact: 'destination_review_only' as const
  };
}
export type PairedRoutingResult =
  | Readonly<{
      status: 'updated' | 'unchanged';
      receipt: PairedDeliveryAcknowledgement;
    }>
  | Readonly<{ status: PrivateStorageFailure | 'invalid' | 'stopped' }>;
export async function approvePairedRoutingReview(
  token: PairedRoutingReview,
  review: unknown
): Promise<PairedRoutingResult> {
  const state = routingReviewStates.get(token);
  if (
    !state ||
    review !== 'reviewed_private_destination_update' ||
    !navigator.locks?.request
  )
    return { status: 'invalid' };
  if (!state.current()) return { status: 'stopped' };
  const { repository, session, receipt, saved, context } = state;
  try {
    const owner = await recheckIdentityOwner(session);
    if (
      owner.state !== 'messaging_capable' ||
      owner.publicKey !== saved.record.owner ||
      owner.admission === 'busy' ||
      !state.current()
    )
      return { status: 'stopped' };
    return await navigator.locks.request(
      'harvestcircle:owner:' + saved.record.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock) => {
        if (!lock || !state.current()) return { status: 'stopped' as const };
        const base = await loadPrivateRecord(
          repository,
          'private_sends',
          saved.record.id
        );
        if (!state.current()) return { status: 'stopped' as const };
        if (!base.ok) return { status: base.reason };
        if (
          privateRecordWire(base.value, saved.record.owner, saved.record.id) !==
          saved.wire
        )
          return { status: 'conflict' as const };
        const routes = inboxRoutePlanSnapshot(context.plan),
          row = saved.record;
        if (!routes || !row.deliveryPlan) return { status: 'invalid' as const };
        let expected = base.value;
        if (state.changed) {
          if (row.revision === Number.MAX_SAFE_INTEGER)
            return { status: 'invalid_record' as const };
          const decoded = decodePrivateRecord(
            JSON.stringify({
              ...row,
              revision: row.revision + 1,
              deliveryPlan: {
                state: 'prepared',
                routes,
                previousRoutes: [
                  ...(row.deliveryPlan.previousRoutes ?? []),
                  row.deliveryPlan.routes
                ]
              }
            }),
            row.owner,
            row.id
          );
          if (!decoded.ok) return { status: 'invalid_record' as const };
          expected = decoded.value;
          if (!state.current()) return { status: 'stopped' as const };
          const committed = await commitPrivateRecord(
            repository,
            expected,
            base.value
          );
          if (!state.current())
            return { status: 'unknown_completion' as const };
          if (!committed.ok) return { status: committed.reason };
        }
        const loaded = await loadPrivateRecord(
          repository,
          'private_sends',
          row.id
        );
        if (!state.current() || !loaded.ok)
          return { status: 'unknown_completion' as const };
        const wire = privateRecordWire(loaded.value, row.owner, row.id),
          next = privateRecordSnapshot(loaded.value, row.owner, row.id);
        if (
          !wire ||
          wire !== privateRecordWire(expected, row.owner, row.id) ||
          !next ||
          next.family !== 'private_send_operation' ||
          JSON.stringify(next.self) !== JSON.stringify(row.self) ||
          JSON.stringify(next.peerArtifact) !== JSON.stringify(row.peerArtifact)
        )
          return { status: 'conflict' as const };
        pairedAcknowledgements.set(receipt, {
          record: next,
          wire,
          current: saved.current
        });
        pendingRoutingReviews.delete(receipt);
        routingReviewStates.delete(token);
        return {
          status: state.changed ? ('updated' as const) : ('unchanged' as const),
          receipt
        };
      }
    );
  } catch {
    return { status: state.current() ? 'unavailable' : 'stopped' };
  }
}
// Explicit restart admission authenticates the self recovery copy and verifies
// exact durable pair custody. It does not decrypt or newly authenticate the
// nested peer ciphertext, regenerate either artifact, or perform publication.
export async function acknowledgeRecoveredPairedDelivery(
  repository: PrivateStorageRepository,
  session: IdentitySession,
  reserved: ReservedSendIdentity,
  self: SelfRecoveryAcknowledgement,
  context: PairedDeliveryContext,
  review: unknown
): Promise<PairedDeliveryCommitResult> {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_stored_private_retry' ||
    !navigator.locks?.request
  )
    return { status: 'invalid' };
  const saved = acknowledgements.get(self),
    original = reservedSendSnapshot(reserved),
    ownership = identityMessagingOwnership(session);
  if (
    !saved?.current() ||
    !original ||
    !ownership?.current() ||
    JSON.stringify(saved.record) !== JSON.stringify(original) ||
    original.owner !== ownership.owner
  )
    return { status: 'invalid' };
  const current = () =>
    ownership.current() &&
    saved.current() &&
    !!reservedSendSnapshot(reserved) &&
    recheckInboxRoutePlan(
      context.plan,
      context.policy,
      context.own,
      context.other
    ) === 'unchanged';
  try {
    const checked = await recheckIdentityOwner(session);
    if (
      checked.state !== 'messaging_capable' ||
      checked.publicKey !== ownership.owner ||
      checked.admission === 'busy' ||
      !current()
    )
      return { status: 'stopped' };
    return await navigator.locks.request(
      'harvestcircle:owner:' + ownership.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock) => {
        if (!lock || !current()) return { status: 'stopped' as const };
        const loaded = await loadPrivateRecord(
          repository,
          'private_sends',
          original.id
        );
        if (!current()) return { status: 'stopped' as const };
        if (!loaded.ok) return { status: loaded.reason };
        const row = privateRecordSnapshot(
            loaded.value,
            original.owner,
            original.id
          ),
          wire = privateRecordWire(loaded.value, original.owner, original.id),
          routes = inboxRoutePlanSnapshot(context.plan);
        if (
          !row ||
          row.family !== 'private_send_operation' ||
          !row.peerArtifact ||
          !row.deliveryPlan ||
          !wire ||
          wire !== saved.wire ||
          JSON.stringify(row.deliveryPlan.routes) !== JSON.stringify(routes)
        )
          return { status: 'conflict' as const };
        const verified = verifyEnvelope(row.peerArtifact.wire),
          event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
        if (
          !event ||
          event.kind !== 1059 ||
          event.id !== row.peerArtifact.eventId ||
          event.tags.length !== 1 ||
          event.tags[0].length !== 2 ||
          event.tags[0][0] !== 'p' ||
          event.tags[0][1] !== row.peer ||
          !current()
        )
          return { status: 'invalid' as const };
        const receipt = Object.freeze({}) as PairedDeliveryAcknowledgement;
        pairedAcknowledgements.set(receipt, {
          record: row,
          wire,
          current: () => ownership.current()
        });
        return { status: 'existing' as const, receipt };
      }
    );
  } catch {
    return { status: current() ? 'unavailable' : 'stopped' };
  }
}
export type PairedDeliveryCommitResult =
  | Readonly<{
      status: 'prepared' | 'existing' | 'reconciled';
      receipt: PairedDeliveryAcknowledgement;
    }>
  | Readonly<{ status: PrivateStorageFailure | 'invalid' | 'stopped' }>;

// Requires genuine current self acknowledgement, peer factory proof and route
// capture. Structural local record admission alone cannot mint this receipt.
// Caller owns WebLock and current route recheck; this function owns short CAS.
export async function commitPairedDelivery(
  repository: PrivateStorageRepository,
  session: IdentitySession,
  reserved: ReservedSendIdentity,
  self: SelfRecoveryAcknowledgement,
  peer: VerifiedOutboundEnvelope,
  plan: InboxRoutePlan,
  review: unknown
): Promise<PairedDeliveryCommitResult> {
  if (typeof window === 'undefined' || review !== 'reviewed_pair_commit')
    return { status: 'invalid' };
  const savedSelf = acknowledgements.get(self),
    original = reservedSendSnapshot(reserved),
    ownership = identityMessagingOwnership(session),
    proof = verifiedOutboundSnapshot(peer),
    routes = inboxRoutePlanSnapshot(plan);
  if (
    !savedSelf?.current() ||
    !original ||
    !ownership?.current() ||
    !proof ||
    !routes ||
    original.owner !== ownership.owner ||
    savedSelf.record.owner !== original.owner ||
    savedSelf.record.id !== original.id ||
    savedSelf.record.peer !== original.peer ||
    savedSelf.record.rumorHash !== original.rumorHash ||
    savedSelf.record.createdAt !== original.createdAt ||
    proof.role !== 'peer' ||
    proof.destination !== original.peer ||
    proof.owner !== original.owner ||
    proof.command !== original.id ||
    proof.peer !== original.peer ||
    proof.rumorHash !== original.rumorHash ||
    routes.peer.author !== original.peer ||
    routes.archive.author !== original.owner
  )
    return { status: 'invalid' };
  const verified = verifyEnvelope(proof.wire),
    event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
  if (!event || event.kind !== 1059) return { status: 'invalid' };
  const current = () => {
    const fresh = reservedSendSnapshot(reserved),
      peerNow = verifiedOutboundSnapshot(peer);
    return (
      ownership.current() &&
      savedSelf.current() &&
      fresh?.owner === original.owner &&
      fresh.id === original.id &&
      fresh.peer === original.peer &&
      fresh.rumorHash === original.rumorHash &&
      fresh.createdAt === original.createdAt &&
      peerNow?.role === 'peer' &&
      peerNow.wire === proof.wire
    );
  };
  try {
    const loaded = await readSelfRecoveryBase(repository, reserved);
    if (!current()) return { status: 'stopped' };
    if (!loaded.ok) return { status: loaded.reason };
    const base = privateRecordSnapshot(
        loaded.value,
        original.owner,
        original.id
      ),
      baseWire = privateRecordWire(loaded.value, original.owner, original.id);
    if (
      !base ||
      base.family !== 'private_send_operation' ||
      base.self.eventId !== savedSelf.self.eventId ||
      base.self.wire !== savedSelf.self.wire
    )
      return { status: 'conflict' };
    const peerArtifact = { eventId: event.id, wire: proof.wire },
      deliveryPlan = { state: 'prepared' as const, routes };
    let expected: PrivateRecordHandle,
      status: 'prepared' | 'existing' | 'reconciled';
    if (base.peerArtifact || base.deliveryPlan) {
      if (
        base.peerArtifact?.eventId !== peerArtifact.eventId ||
        base.peerArtifact.wire !== peerArtifact.wire ||
        JSON.stringify(base.deliveryPlan) !== JSON.stringify(deliveryPlan)
      )
        return { status: 'conflict' };
      expected = loaded.value;
      status = 'existing';
    } else {
      if (baseWire !== savedSelf.wire) return { status: 'conflict' };
      const decoded = decodePrivateRecord(
        JSON.stringify({
          schema: 1,
          family: 'private_send_operation',
          owner: base.owner,
          id: base.id,
          revision: base.revision + 1,
          peer: base.peer,
          rumorHash: base.rumorHash,
          createdAt: base.createdAt,
          self: { eventId: base.self.eventId, wire: base.self.wire },
          peerArtifact,
          deliveryPlan
        }),
        base.owner,
        base.id
      );
      if (!decoded.ok) return { status: 'invalid_record' };
      expected = decoded.value;
      if (!current()) return { status: 'stopped' };
      const committed = await commitPrivateRecord(
        repository,
        expected,
        loaded.value
      );
      if (!current()) return { status: 'stopped' };
      if (!committed.ok && committed.reason !== 'unknown_completion')
        return { status: committed.reason };
      status = !committed.ok
        ? 'reconciled'
        : committed.value.state === 'existing'
          ? 'existing'
          : 'prepared';
    }
    const wire = privateRecordWire(expected, original.owner, original.id),
      readback = await loadPrivateRecord(
        repository,
        'private_sends',
        original.id
      );
    if (!current()) return { status: 'stopped' };
    if (!readback.ok) return { status: 'unknown_completion' };
    const observed = privateRecordSnapshot(
      readback.value,
      original.owner,
      original.id
    );
    if (
      !wire ||
      privateRecordWire(readback.value, original.owner, original.id) !== wire ||
      !observed ||
      observed.family !== 'private_send_operation' ||
      !observed.peerArtifact ||
      !observed.deliveryPlan
    )
      return { status: 'conflict' };
    const receipt = Object.freeze({}) as PairedDeliveryAcknowledgement;
    pairedAcknowledgements.set(receipt, {
      record: observed,
      wire,
      current: () => ownership.current()
    });
    return { status, receipt };
  } catch {
    return { status: current() ? 'unavailable' : 'stopped' };
  }
}
export function pairedDeliveryAcknowledgementSnapshot(
  receipt: PairedDeliveryAcknowledgement
) {
  const saved = pairedAcknowledgements.get(receipt);
  if (
    !saved?.current() ||
    pendingRoutingReviews.has(receipt) ||
    !saved.record.peerArtifact ||
    !saved.record.deliveryPlan
  )
    return undefined;
  const row = saved.record;
  return {
    owner: row.owner,
    id: row.id,
    revision: row.revision,
    peer: row.peer,
    rumorHash: row.rumorHash,
    createdAt: row.createdAt,
    self: { ...row.self },
    peerArtifact: { ...saved.record.peerArtifact },
    deliveryPlan: JSON.parse(
      JSON.stringify(saved.record.deliveryPlan)
    ) as NonNullable<PrivateSendOperation['deliveryPlan']>,
    fact: 'acknowledged_local_pair_only' as const
  };
}
export async function verifyPairedDeliveryAcknowledgement(
  repository: PrivateStorageRepository,
  receipt: PairedDeliveryAcknowledgement
): Promise<boolean> {
  const saved = pairedAcknowledgements.get(receipt);
  if (!saved?.current() || pendingRoutingReviews.has(receipt)) return false;
  const loaded = await loadPrivateRecord(
    repository,
    'private_sends',
    saved.record.id
  );
  return (
    saved.current() &&
    !pendingRoutingReviews.has(receipt) &&
    loaded.ok &&
    privateRecordWire(loaded.value, saved.record.owner, saved.record.id) ===
      saved.wire
  );
}
// A separately decoded row cannot supply acceptance facts merely because the
// owner and local ID match. Require the full wire acknowledged by this genuine
// current custody, then let the workflow recheck actual storage before use.
export function pairedDeliveryRecordMatches(
  receipt: PairedDeliveryAcknowledgement,
  record: PrivateRecordHandle
): boolean {
  const saved = pairedAcknowledgements.get(receipt);
  return (
    !!saved?.current() &&
    !pendingRoutingReviews.has(receipt) &&
    privateRecordWire(record, saved.record.owner, saved.record.id) ===
      saved.wire
  );
}
// Pure genuine custody metadata. A workflow must still verify full readback
// under the owner lock before suppressing or admitting a network effect.
export function pairedDeliveryTargetAccepted(
  receipt: PairedDeliveryAcknowledgement,
  role: 'peer' | 'self_archive',
  origin: string,
  eventId: string
): boolean {
  const saved = pairedAcknowledgements.get(receipt);
  return (
    !!saved?.current() &&
    !pendingRoutingReviews.has(receipt) &&
    (saved.record.receipts ?? []).some(
      (fact) =>
        fact.role === role &&
        fact.origin === origin &&
        fact.eventId === eventId &&
        fact.status === 'accepted'
    )
  );
}
// Only the already genuine current pair can renew its exact custody after a
// controlled append of named receipt metadata. A decoded namespace row alone
// cannot mint this acknowledgment. Earlier permissions invalidate on revision.
export async function commitPairedDeliveryReceipt(
  repository: PrivateStorageRepository,
  receipt: PairedDeliveryAcknowledgement,
  receiptWire: unknown
): Promise<PrivateReceiptCommitResult> {
  const saved = pairedAcknowledgements.get(receipt);
  if (!saved?.current() || pendingRoutingReviews.has(receipt))
    return { status: 'invalid' };
  try {
    const loaded = await loadPrivateRecord(
      repository,
      'private_sends',
      saved.record.id
    );
    if (!loaded.ok) return { status: loaded.reason };
    if (
      !saved.current() ||
      privateRecordWire(loaded.value, saved.record.owner, saved.record.id) !==
        saved.wire
    )
      return { status: 'conflict' };
    const transition = preparePrivateReceiptTransition(
      loaded.value,
      saved.record.owner,
      saved.record.id,
      receiptWire
    );
    if (!transition.ok) return { status: 'invalid' };
    const committed = await commitPrivateReceiptTransition(
      repository,
      transition.value
    );
    if (!('record' in committed)) return committed;
    const row = privateRecordSnapshot(
        committed.record,
        saved.record.owner,
        saved.record.id
      ),
      wire = privateRecordWire(
        committed.record,
        saved.record.owner,
        saved.record.id
      );
    if (!saved.current()) return { status: 'unknown_completion' };
    if (
      !row ||
      row.family !== 'private_send_operation' ||
      !wire ||
      !row.peerArtifact ||
      !row.deliveryPlan
    )
      return { status: 'conflict' };
    pairedAcknowledgements.set(receipt, {
      record: row,
      wire,
      current: saved.current
    });
    return committed;
  } catch {
    return { status: 'unknown_completion' };
  }
}
