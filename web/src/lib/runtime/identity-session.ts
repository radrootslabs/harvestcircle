import {
  createExtensionAdapter,
  extensionSnapshot,
  connectExtensionAdapter,
  recheckExtensionAdapter,
  probeExtensionAdapter,
  disconnectExtensionAdapter,
  extensionOwnershipCapture,
  extensionMessagingOwnershipCapture,
  subscribeExtensionInvalidation,
  signApprovedExtensionAdapter,
  markApprovedSigningWaitExpired,
  type ApprovedSignResult,
  type ExtensionAdapter,
  type ExtensionSnapshot,
  type ExtensionGuestReason
} from '../nostr/extension.ts';
import { approveCapturedPublicSigning } from '../nostr/approved-signing.ts';
import {
  publicRecordSnapshot,
  publicRecordWire,
  type PublicRecordHandle
} from '../persistence/records.ts';
import {
  stopPublicEffect,
  type PublicEffectLease,
  type PublicEffectCapture
} from './effect-ownership.ts';
import {
  bindLatePublicResponse,
  latePublicResultIdentity,
  reviewLatePublicArtifact,
  type LatePublicArtifact
} from './late-results.ts';
import {
  capturedArtifactSnapshot,
  type CapturedArtifact
} from '../persistence/artifact-records.ts';

declare const sessionBrand: unique symbol;
export type IdentitySession = Readonly<{ [sessionBrand]: true }>;
export type IdentitySnapshot = (
  | Readonly<{ state: 'guest'; reason: ExtensionGuestReason }>
  | Readonly<{ state: 'pending'; action: 'connect' | 'recheck' | 'probe' }>
  | Readonly<{
      state: 'connected' | 'signing_only' | 'messaging_capable';
      publicKey: string;
      messaging: 'not_probed' | 'unsupported' | 'refused' | 'capable';
    }>
) &
  Readonly<{ admission?: 'busy' }>;
// Per-client opaque session, never a module-global server user. A public key is
// an extension observation, not installed-account or publishing authorization.
const sessions = new WeakMap<IdentitySession, ExtensionAdapter>();
type ViewController = { stamp(): symbol; invalidate(): void };
const views = new WeakMap<IdentitySession, ViewController>();
declare const operationBrand: unique symbol;
export type IdentityPublicOperation = Readonly<{ [operationBrand]: true }>;
type OperationController = {
  session: IdentitySession;
  ownership(): PublicEffectCapture | undefined;
  sign(lease: PublicEffectLease): Promise<ApprovedSignResult>;
  stop(expired: boolean): void;
  snapshot(): Readonly<{
    owner: string;
    id: string;
    revision: number;
    hash: string;
    current: boolean;
    lateAvailable: boolean;
    outcome: ApprovedSignResult['status'] | 'not_started' | 'pending';
  }>;
  review(
    record: PublicRecordHandle,
    review: unknown
  ): CapturedArtifact | undefined;
};
const operations = new WeakMap<IdentityPublicOperation, OperationController>();
export function createIdentitySession(): IdentitySession {
  const session = Object.freeze({}) as IdentitySession;
  sessions.set(session, createExtensionAdapter());
  let stamp = Symbol();
  views.set(session, {
    stamp: () => stamp,
    invalidate() {
      stamp = Symbol();
    }
  });
  return session;
}
function mapIdentitySnapshot(snapshot: ExtensionSnapshot): IdentitySnapshot {
  if (snapshot.state !== 'connected') return { ...snapshot };
  const state: IdentitySnapshot = {
    state:
      snapshot.messaging === 'capable' && snapshot.signingCandidate
        ? 'messaging_capable'
        : snapshot.signingCandidate
          ? 'signing_only'
          : 'connected',
    publicKey: snapshot.publicKey,
    messaging: snapshot.messaging
  };
  return snapshot.admission === 'busy'
    ? { ...state, admission: 'busy' }
    : state;
}
export function identitySessionSnapshot(
  session: IdentitySession
): IdentitySnapshot {
  const adapter = sessions.get(session);
  return adapter
    ? mapIdentitySnapshot(extensionSnapshot(adapter))
    : { state: 'guest', reason: 'unavailable' };
}
export async function connectIdentity(
  session: IdentitySession
): Promise<IdentitySnapshot> {
  const adapter = sessions.get(session);
  if (adapter)
    return mapIdentitySnapshot(await connectExtensionAdapter(adapter));
  return identitySessionSnapshot(session);
}
export async function recheckIdentityOwner(
  session: IdentitySession
): Promise<IdentitySnapshot> {
  const adapter = sessions.get(session);
  if (adapter)
    return mapIdentitySnapshot(await recheckExtensionAdapter(adapter));
  return identitySessionSnapshot(session);
}
export async function probeIdentityMessaging(
  session: IdentitySession,
  review: unknown
): Promise<IdentitySnapshot> {
  const adapter = sessions.get(session);
  if (adapter)
    return mapIdentitySnapshot(await probeExtensionAdapter(adapter, review));
  return identitySessionSnapshot(session);
}
export function disconnectIdentity(session: IdentitySession): void {
  views.get(session)?.invalidate();
  const adapter = sessions.get(session);
  if (adapter) disconnectExtensionAdapter(adapter);
}
// Internal messaging lifecycle ports. Captured observation is not an inbox
// configuration, AUTH, private store or publication permission.
export function identityMessagingOwnership(
  session: IdentitySession
): PublicEffectCapture | undefined {
  const adapter = sessions.get(session);
  return adapter && extensionMessagingOwnershipCapture(adapter);
}
export function subscribeIdentityInvalidation(
  session: IdentitySession,
  listener: () => void
): () => void {
  const adapter = sessions.get(session);
  return adapter ? subscribeExtensionInvalidation(adapter, listener) : () => {};
}
// Explicit route/input lifecycle event. No draft, DB, reconnect or SDK effect.
export function invalidateIdentityOperations(session: IdentitySession): void {
  views.get(session)?.invalidate();
}
// Internal public operation capability, never an unrestricted UI template signer.
// Current-head/inbox/IDB preparation and publication retain their typed owners.
export function captureIdentityPublicOperation(
  session: IdentitySession,
  record: PublicRecordHandle,
  id: unknown,
  review: unknown
): IdentityPublicOperation | undefined {
  const adapter = sessions.get(session),
    view = views.get(session);
  const captured = adapter && extensionOwnershipCapture(adapter);
  if (!adapter || !view || !captured) return undefined;
  const row = publicRecordSnapshot(record, captured.owner, id),
    recordWire = publicRecordWire(record, captured.owner, id);
  const approval = approveCapturedPublicSigning(
    record,
    captured.owner,
    id,
    review
  );
  if (
    !row ||
    !recordWire ||
    !approval ||
    row.family === 'public_draft' ||
    row.revision !== 0
  )
    return undefined;
  const owner = row.owner,
    command = row.id,
    epoch = view.stamp(),
    originalSession = captured.session;
  let stopped = false,
    pending = false;
  let lease: PublicEffectLease | undefined,
    late: LatePublicArtifact | undefined;
  let outcome: ApprovedSignResult['status'] | 'not_started' | 'pending' =
    'not_started';
  const current = () =>
    !stopped && view.stamp() === epoch && captured.current();
  const token = Object.freeze({}) as IdentityPublicOperation;
  operations.set(token, {
    session,
    ownership: () =>
      current() ? { owner, session: originalSession, current } : undefined,
    async sign(originalLease) {
      if (pending) return { status: 'busy' };
      if (!current()) return { status: 'stale' };
      pending = true;
      lease = originalLease;
      outcome = 'pending';
      try {
        let result = await signApprovedExtensionAdapter(
          adapter,
          approval,
          originalLease
        );
        // The adapter's promise can settle before this continuation runs.
        // Recheck this view before disclosing a normal signed result.
        if (result.status === 'signed' && !current()) {
          const artifact = capturedArtifactSnapshot(result.artifact);
          const originalLate =
            artifact &&
            bindLatePublicResponse(
              approval,
              JSON.parse(artifact.wire),
              originalSession
            );
          result = originalLate
            ? { status: 'unknown', late: originalLate }
            : { status: 'unknown' };
        }
        outcome = result.status;
        if (result.status === 'unknown' && result.late) {
          const original = latePublicResultIdentity(result.late);
          if (
            original?.owner === owner &&
            original.id === command &&
            original.recordWire === recordWire &&
            original.session === originalSession
          )
            late = result.late;
        }
        // Only disclosure/explicit original-operation review can obtain a late
        // artifact. A normal signing result never returns it after invalidation.
        return result.status === 'unknown' ? { status: 'unknown' } : result;
      } finally {
        pending = false;
        lease = undefined;
      }
    },
    stop(expired) {
      stopped = true;
      if (lease) {
        if (expired) markApprovedSigningWaitExpired(adapter, approval, lease);
        stopPublicEffect(lease);
      }
    },
    snapshot: () => ({
      owner,
      id: command,
      revision: row.revision,
      hash: row.capture.hash,
      current: current(),
      lateAvailable: late !== undefined,
      outcome
    }),
    review(original, consent) {
      const observed = extensionSnapshot(adapter);
      return late &&
        observed.state === 'connected' &&
        observed.publicKey === owner
        ? reviewLatePublicArtifact(late, original, owner, command, consent)
        : undefined;
    }
  });
  return token;
}
function operationOf(session: IdentitySession, token: IdentityPublicOperation) {
  const operation = operations.get(token);
  return operation?.session === session ? operation : undefined;
}
export function identityPublicOperationOwnership(
  session: IdentitySession,
  operation: IdentityPublicOperation
): PublicEffectCapture | undefined {
  return operationOf(session, operation)?.ownership();
}
export function identityPublicOperationSnapshot(
  session: IdentitySession,
  operation: IdentityPublicOperation
) {
  return operationOf(session, operation)?.snapshot();
}
export function signIdentityPublicOperation(
  session: IdentitySession,
  operation: IdentityPublicOperation,
  lease: PublicEffectLease
): Promise<ApprovedSignResult> {
  return (
    operationOf(session, operation)?.sign(lease) ??
    Promise.resolve({ status: 'invalid_approval' })
  );
}
export function stopIdentityPublicOperation(
  session: IdentitySession,
  operation: IdentityPublicOperation
): void {
  operationOf(session, operation)?.stop(false);
}
// UI supplies expiry; existing scheduler owns its actual pending promise.
export function expireIdentityPublicOperationWait(
  session: IdentitySession,
  operation: IdentityPublicOperation
): void {
  operationOf(session, operation)?.stop(true);
}
// Pure same-author original review, not fresh author/CAS/resume/publish proof.
export function reviewIdentityLatePublicArtifact(
  session: IdentitySession,
  operation: IdentityPublicOperation,
  original: PublicRecordHandle,
  review: unknown
): CapturedArtifact | undefined {
  return operationOf(session, operation)?.review(original, review);
}
