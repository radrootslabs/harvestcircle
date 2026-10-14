import { getEventHash, type UnsignedEvent } from 'applesauce-core/helpers';
import { readRelayPolicy, type RelayPolicy } from '../config/relays.ts';
import {
  inboxOperatorQualificationSnapshot,
  inboxOperatorQualificationWire,
  type InboxOperatorQualification
} from '../config/inbox-operators.ts';
import {
  inboxAccessConstructionBinding,
  subscribeInboxAccessControllerClose,
  type InboxAccessController
} from '../messaging/inbox-access-controller.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  withIdlePrivateLocalStorage,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  privateStorageOwner,
  commitPrivateRecord,
  loadPrivateRecord,
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from '../persistence/private-storage.ts';
import {
  decodePrivateRecord,
  privateRecordWire,
  type PrivateAccessProbe,
  type PrivateRecordHandle
} from '../persistence/private-records.ts';
import { canonicalLocalId } from '../private-handles.ts';
import { safeUnsignedInteger } from './envelope-bounds.ts';

declare const probeIdentityBrand: unique symbol;
export type InboxProbeIdentity = Readonly<{ [probeIdentityBrand]: true }>;
export type InboxProbeReservationResult =
  | Readonly<{
      status: 'reserved' | 'existing';
      disposition: 'stored_original';
      identity: InboxProbeIdentity;
    }>
  | Readonly<{
      status: PrivateStorageFailure | 'invalid' | 'busy' | 'stopped';
      disposition: 'not_created' | 'may_exist';
    }>;
type Saved = {
  session: PrivateSession;
  record: PrivateAccessProbe;
  handle: PrivateRecordHandle;
  wire(): string | undefined;
  current(): boolean;
  stop(): void;
};
type Action = {
  repository: PrivateStorageRepository;
  session: PrivateSession;
  policy: RelayPolicy;
  operator: InboxOperatorQualification;
  pending(): boolean;
  possible(): boolean;
  result(): InboxProbeReservationResult | undefined;
};
const identities = new WeakMap<InboxProbeIdentity, Saved>();
const actions = new WeakMap<InboxAccessController, Action>();
// A fixed diagnostic, never a Message composer input or recognition/suppression
// authority. Only the separate current genuine construction owner sees its wire.
const diagnostic = 'Harvestcircle private inbox access check v1: ';

export async function reserveInboxProbeIdentity(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy,
  access: InboxAccessController,
  operator: InboxOperatorQualification,
  review: unknown
): Promise<InboxProbeReservationResult> {
  const prior = actions.get(access);
  const failure = (
    status: PrivateStorageFailure | 'invalid' | 'busy' | 'stopped'
  ): InboxProbeReservationResult => ({
    status,
    disposition: prior?.possible() ? 'may_exist' : 'not_created'
  });
  if (review !== 'reviewed_inbox_probe_reservation') return failure('invalid');
  if (prior) {
    if (
      prior.repository !== repository ||
      prior.session !== session ||
      prior.policy !== policy ||
      prior.operator !== operator
    )
      return failure('conflict');
    if (prior.pending()) return failure('busy');
    const original = prior.result();
    if (original && 'identity' in original) {
      return inboxProbeSnapshot(original.identity)
        ? { ...original, status: 'existing' }
        : failure('stopped');
    }
    return original ?? failure('stopped');
  }
  const proof = inboxAccessConstructionBinding(access, session, policy),
    owner = privateSessionOwnership(session),
    declaration = inboxOperatorQualificationSnapshot(operator),
    operatorWire = inboxOperatorQualificationWire(operator, policy);
  if (
    !proof ||
    !owner?.current() ||
    proof.owner !== owner.owner ||
    proof.session !== owner.session ||
    privateStorageOwner(repository) !== owner.owner
  )
    return failure('invalid_scope');
  if (
    !declaration ||
    !operatorWire ||
    declaration.authority === 'unconfigured' ||
    ![...proof.readTargets, ...proof.archiveTargets].every((origin) => {
      const row = declaration.relays.find((entry) => entry.origin === origin);
      return (
        row &&
        row.auth !== 'unsupported' &&
        row.recipientRead === 'supported' &&
        row.namedArchiveAck === 'supported' &&
        row.exactReadback === 'supported' &&
        row.retention === 'operator_defined'
      );
    })
  )
    return failure('unavailable');
  const binding = {
    preferenceId: proof.preferenceId,
    preferenceCreatedAt: proof.preferenceCreatedAt,
    readTargets: [...proof.readTargets],
    archiveTargets: [...proof.archiveTargets],
    policyWire: JSON.stringify(readRelayPolicy(policy)),
    operatorWire
  };
  const frozenBinding = JSON.stringify(binding);
  let pending = true,
    possible = false,
    result: InboxProbeReservationResult | undefined;
  const action: Action = {
    repository,
    session,
    policy,
    operator,
    pending: () => pending,
    possible: () => possible,
    result: () => result
  };
  actions.set(access, action);
  const refuse = (
    status: PrivateStorageFailure | 'invalid' | 'busy' | 'stopped'
  ): InboxProbeReservationResult => ({
    status,
    disposition: possible ? 'may_exist' : 'not_created'
  });
  const actorCurrent = () => {
    const actual = inboxAccessConstructionBinding(access, session, policy);
    return (
      owner.current() &&
      proof.current() &&
      !!actual &&
      actual.session === owner.session &&
      actual.owner === owner.owner &&
      JSON.stringify({
        preferenceId: actual.preferenceId,
        preferenceCreatedAt: actual.preferenceCreatedAt,
        readTargets: actual.readTargets,
        archiveTargets: actual.archiveTargets,
        policyWire: JSON.stringify(readRelayPolicy(policy)),
        operatorWire: inboxOperatorQualificationWire(operator, policy)
      }) === frozenBinding
    );
  };
  let captured: InboxProbeIdentity | undefined;
  try {
    if (typeof window === 'undefined' || !navigator.locks?.request)
      return (result = refuse('unavailable'));
    result = await navigator.locks.request(
      'harvestcircle:owner:' + owner.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock) => {
        if (!lock) return refuse('busy');
        if (!actorCurrent()) return refuse('stopped');
        const idle = await withIdlePrivateLocalStorage(
          session,
          async (): Promise<InboxProbeReservationResult> => {
            if (!actorCurrent()) return refuse('stopped');
            const milliseconds = safeUnsignedInteger(Date.now()),
              id = canonicalLocalId(crypto.randomUUID());
            if (milliseconds === undefined || !id || !actorCurrent())
              return refuse('invalid_record');
            const createdAt = Math.floor(milliseconds / 1000),
              nonceBytes = crypto.getRandomValues(new Uint8Array(32));
            let wire: string | undefined;
            try {
              const nonce = Array.from(nonceBytes, (byte) =>
                byte.toString(16).padStart(2, '0')
              ).join('');
              const rumor: UnsignedEvent = {
                pubkey: owner.owner,
                created_at: createdAt,
                kind: 14,
                tags: [['p', owner.owner]],
                content: diagnostic + nonce
              };
              wire = JSON.stringify({ id: getEventHash(rumor), ...rumor });
            } finally {
              nonceBytes.fill(0);
            }
            if (!wire || !actorCurrent()) return refuse('stopped');
            const rumorHash = (JSON.parse(wire) as { id: string }).id;
            const record: PrivateAccessProbe = {
              schema: 1,
              family: 'private_access_probe',
              owner: owner.owner,
              id,
              revision: 0,
              rumorHash,
              createdAt,
              binding,
              self: null,
              receipts: []
            };
            const handle = decodePrivateRecord(
              JSON.stringify(record),
              owner.owner,
              id
            );
            if (!handle.ok || !actorCurrent()) return refuse('invalid_record');
            const originalWire = privateRecordWire(
              handle.value,
              owner.owner,
              id
            );
            if (!originalWire) return refuse('invalid_record');
            const token = Object.freeze({}) as InboxProbeIdentity;
            let stopped = false,
              offSession = () => {},
              offAccess = () => {};
            const saved: Saved = {
              session,
              record,
              handle: handle.value,
              wire: () => wire,
              current() {
                if (stopped || !actorCurrent()) {
                  saved.stop();
                  return false;
                }
                return wire !== undefined;
              },
              stop() {
                stopped = true;
                wire = undefined;
                offSession();
                offAccess();
              }
            };
            captured = token;
            identities.set(token, saved);
            offSession = subscribePrivateSessionClose(session, () =>
              saved.stop()
            );
            offAccess = subscribeInboxAccessControllerClose(access, () =>
              saved.stop()
            );
            if (!saved.current()) {
              saved.stop();
              return refuse('stopped');
            }
            possible = true;
            const committed = await commitPrivateRecord(
              repository,
              handle.value,
              null
            );
            if (!committed.ok) {
              possible = ![
                'capacity',
                'invalid_record',
                'invalid_scope',
                'aborted'
              ].includes(committed.reason);
              saved.stop();
              return refuse(committed.reason);
            }
            if (!saved.current()) return refuse('stopped');
            const readback = await loadPrivateRecord(
              repository,
              'private_sends',
              id
            );
            if (
              !readback.ok ||
              privateRecordWire(readback.value, owner.owner, id) !==
                originalWire ||
              !saved.current()
            ) {
              saved.stop();
              return refuse(readback.ok ? 'conflict' : readback.reason);
            }
            return {
              status:
                committed.value.state === 'committed' ? 'reserved' : 'existing',
              disposition: 'stored_original',
              identity: token
            };
          }
        );
        return idle.ok
          ? idle.value
          : refuse(actorCurrent() ? 'busy' : 'stopped');
      }
    );
    return result;
  } catch {
    if (captured) identities.get(captured)?.stop();
    return (result = refuse('unavailable'));
  } finally {
    pending = false;
    if (!result || !('identity' in result)) {
      if (captured) identities.get(captured)?.stop();
    }
  }
}
export function inboxProbeSnapshot(
  identity: InboxProbeIdentity
): PrivateAccessProbe | undefined {
  const saved = identities.get(identity);
  return saved?.current()
    ? (JSON.parse(JSON.stringify(saved.record)) as PrivateAccessProbe)
    : undefined;
}
// Genuine original session association; neither detached wire nor a same-owner
// replacement session can supply the original memory-only crypto input.
export function inboxProbeRumorWire(
  identity: InboxProbeIdentity,
  session: PrivateSession
): string | undefined {
  const saved = identities.get(identity);
  return saved?.session === session && saved.current()
    ? saved.wire()
    : undefined;
}
export function inboxProbeReservationHandle(
  identity: InboxProbeIdentity
): PrivateRecordHandle | undefined {
  const saved = identities.get(identity);
  return saved?.current() ? saved.handle : undefined;
}
export function stopInboxProbeIdentity(identity: InboxProbeIdentity): void {
  identities.get(identity)?.stop();
}
