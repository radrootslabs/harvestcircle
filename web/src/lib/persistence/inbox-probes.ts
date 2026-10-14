import {
  inboxProbeGiftwrapSnapshot,
  inboxProbeGiftwrapSeal,
  type InboxProbeGiftwrap
} from '../nostr/giftwrap-builder.ts';
import { inboxProbeSealIdentity } from '../nostr/seal-builder.ts';
import {
  inboxProbeSnapshot,
  inboxProbeRumorWire,
  inboxProbeReservationHandle,
  type InboxProbeIdentity
} from '../nostr/inbox-access.ts';
import {
  privateSessionOwnership,
  withIdlePrivateLocalStorage,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  privateStorageOwner,
  commitPrivateRecord,
  observePrivateRecord,
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from './private-storage.ts';
import {
  decodePrivateRecord,
  privateRecordWire,
  type PrivateAccessProbe
} from './private-records.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';

declare const acknowledgementBrand: unique symbol;
export type InboxProbeAcknowledgement = Readonly<{
  [acknowledgementBrand]: true;
}>;
export type InboxProbeCommitResult =
  | Readonly<{
      status: 'prepared' | 'existing' | 'reconciled';
      acknowledgement: InboxProbeAcknowledgement;
    }>
  | Readonly<{
      status: PrivateStorageFailure | 'invalid' | 'busy' | 'stopped';
      disposition: 'original_protected';
    }>;
type Saved = Readonly<{
  repository: PrivateStorageRepository;
  session: PrivateSession;
  identity: InboxProbeIdentity;
  record: PrivateAccessProbe;
  wire: string;
  current(): boolean;
  revoke(): void;
}>;
const acknowledgements = new WeakMap<InboxProbeAcknowledgement, Saved>();
const actions = new WeakMap<
  InboxProbeGiftwrap,
  Readonly<{
    repository: PrivateStorageRepository;
    session: PrivateSession;
    pending(): boolean;
  }>
>();
const refuse = (
  status: PrivateStorageFailure | 'invalid' | 'busy' | 'stopped'
): InboxProbeCommitResult => ({ status, disposition: 'original_protected' });

// This owner accepts only the original genuine construction chain. Decoding a
// row, copying a snapshot or supplying the same public key grants no authority.
function input(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  wrap: InboxProbeGiftwrap
) {
  const artifact = inboxProbeGiftwrapSnapshot(wrap),
    seal = inboxProbeGiftwrapSeal(wrap),
    identity = seal && inboxProbeSealIdentity(seal),
    reservation = identity && inboxProbeSnapshot(identity),
    handle = identity && inboxProbeReservationHandle(identity),
    ownership = privateSessionOwnership(session);
  if (
    !artifact ||
    !identity ||
    !reservation ||
    !handle ||
    !ownership?.current() ||
    !inboxProbeRumorWire(identity, session) ||
    privateStorageOwner(repository) !== ownership.owner ||
    reservation.owner !== ownership.owner ||
    artifact.owner !== reservation.owner ||
    artifact.destination !== reservation.owner ||
    artifact.command !== reservation.id ||
    artifact.rumorHash !== reservation.rumorHash ||
    artifact.createdAt !== reservation.createdAt
  )
    return undefined;
  const verified = verifyEnvelope(artifact.wire),
    event = verified.ok && verifiedEnvelopeSnapshot(verified.value),
    reservationWire = privateRecordWire(
      handle,
      reservation.owner,
      reservation.id
    );
  if (
    !event ||
    !reservationWire ||
    event.kind !== 1059 ||
    JSON.stringify(event.tags) !== JSON.stringify([['p', reservation.owner]])
  )
    return undefined;
  const record: PrivateAccessProbe = {
      ...reservation,
      revision: 1,
      self: { eventId: event.id, wire: artifact.wire },
      receipts: []
    },
    decoded = decodePrivateRecord(
      JSON.stringify(record),
      reservation.owner,
      reservation.id
    );
  if (!decoded.ok) return undefined;
  const wire = privateRecordWire(
    decoded.value,
    reservation.owner,
    reservation.id
  );
  if (!wire) return undefined;
  const frozenReservation = JSON.stringify(reservation),
    frozenArtifact = JSON.stringify(artifact);
  function current() {
    return (
      ownership!.current() &&
      !!inboxProbeRumorWire(identity!, session) &&
      privateStorageOwner(repository) === reservation!.owner &&
      JSON.stringify(inboxProbeSnapshot(identity!)) === frozenReservation &&
      JSON.stringify(inboxProbeGiftwrapSnapshot(wrap)) === frozenArtifact
    );
  }
  return current()
    ? {
        identity,
        reservation,
        reservationWire,
        record,
        handle: decoded.value,
        wire,
        current
      }
    : undefined;
}

// Native completion and a separate physical-wire observation are necessary.
// This local acknowledgement permits neither EVENT nor readiness or cleanup.
export async function commitInboxProbe(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  wrap: InboxProbeGiftwrap,
  review: unknown
): Promise<InboxProbeCommitResult> {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_inbox_probe_commit' ||
    typeof wrap !== 'object' ||
    wrap === null
  )
    return refuse('invalid');
  const prior = actions.get(wrap);
  if (prior?.pending()) return refuse('busy');
  if (prior && (prior.repository !== repository || prior.session !== session))
    return refuse('invalid_scope');
  let pending = true;
  let captured: ReturnType<typeof input>;
  try {
    actions.set(wrap, { repository, session, pending: () => pending });
    captured = input(repository, session, wrap);
    if (!captured) return refuse('invalid');
    const original = captured;
    if (!navigator.locks?.request) return refuse('unavailable');
    return await navigator.locks.request(
      'harvestcircle:owner:' + original.reservation.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock) => {
        if (!lock) return refuse('busy');
        if (!original.current()) return refuse('stopped');
        const idle = await withIdlePrivateLocalStorage(
          session,
          async (): Promise<InboxProbeCommitResult> => {
            if (!original.current()) return refuse('stopped');
            const { owner, id } = original.reservation,
              loaded = await observePrivateRecord(
                repository,
                'private_sends',
                id
              );
            if (!original.current()) return refuse('stopped');
            if (!loaded.ok) return refuse(loaded.reason);
            let status: 'prepared' | 'existing' | 'reconciled';
            if (loaded.value.wire === original.wire) {
              status = 'existing';
            } else {
              if (loaded.value.wire !== original.reservationWire)
                return refuse('conflict');
              const committed = await commitPrivateRecord(
                repository,
                original.handle,
                loaded.value.handle
              );
              if (!original.current()) return refuse('stopped');
              if (!committed.ok && committed.reason !== 'unknown_completion')
                return refuse(committed.reason);
              status = !committed.ok
                ? 'reconciled'
                : committed.value.state === 'existing'
                  ? 'existing'
                  : 'prepared';
            }
            const readback = await observePrivateRecord(
              repository,
              'private_sends',
              id
            );
            if (!original.current()) return refuse('stopped');
            if (!readback.ok) return refuse(readback.reason);
            if (readback.value.wire !== original.wire)
              return refuse('conflict');
            // The handle and physical wire have both passed complete-owner
            // validation. Their equality is checked independently of metadata
            // normalization; equivalent raw JSON changes cannot be hidden.
            if (
              privateRecordWire(readback.value.handle, owner, id) !==
              original.wire
            )
              return refuse('conflict');
            const acknowledgement = Object.freeze(
              {}
            ) as InboxProbeAcknowledgement;
            let revoked = false;
            acknowledgements.set(acknowledgement, {
              repository,
              session,
              identity: original.identity,
              record: original.record,
              wire: original.wire,
              current: () => !revoked && original.current(),
              revoke: () => {
                revoked = true;
              }
            });
            return { status, acknowledgement };
          }
        );
        return idle.ok
          ? idle.value
          : refuse(original.current() ? 'busy' : 'stopped');
      }
    );
  } catch {
    return refuse(
      captured ? (captured.current() ? 'unavailable' : 'stopped') : 'invalid'
    );
  } finally {
    pending = false;
    if (!captured && !prior) actions.delete(wrap);
  }
}

export function inboxProbeAcknowledgementSnapshot(
  acknowledgement: InboxProbeAcknowledgement
) {
  const saved = acknowledgements.get(acknowledgement);
  return saved?.current()
    ? {
        record: JSON.parse(JSON.stringify(saved.record)) as PrivateAccessProbe,
        fact: 'acknowledged_local_probe_ciphertext_only' as const
      }
    : undefined;
}

export async function verifyInboxProbeAcknowledgement(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  acknowledgement: InboxProbeAcknowledgement
): Promise<boolean> {
  const saved = acknowledgements.get(acknowledgement);
  if (
    !saved ||
    saved.repository !== repository ||
    saved.session !== session ||
    !saved.current()
  )
    return false;
  const readback = await observePrivateRecord(
    repository,
    'private_sends',
    saved.record.id
  );
  const valid =
    saved.current() &&
    readback.ok &&
    readback.value.wire === saved.wire &&
    privateRecordWire(
      readback.value.handle,
      saved.record.owner,
      saved.record.id
    ) === saved.wire;
  if (!valid) saved.revoke();
  return valid;
}
