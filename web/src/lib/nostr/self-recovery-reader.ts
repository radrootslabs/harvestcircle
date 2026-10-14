import type { RelayPolicy } from '../config/relays.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  inboxProbeReadbackAttemptEvidence,
  type InboxProbeReadbackAttemptResult
} from './private-pool.ts';
import {
  inboxProbePublicationBinding,
  type InboxProbeAcknowledgement
} from '../persistence/inbox-probes.ts';
import { inboxProbeRumorWire } from './inbox-access.ts';
import { inboxProbeReadbackMatches } from '../persistence/private-records.ts';
import { ExtensionSigner } from 'applesauce-signers';
import { PRIVATE_TRANSPORT_BUDGETS } from '../config/budgets.ts';
import { canonicalLocalId } from '../private-handles.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import {
  identityMessagingOwnership,
  subscribeIdentityInvalidation,
  disconnectIdentity,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  loadPrivateRecord,
  observePrivateRecord,
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from '../persistence/private-storage.ts';
import {
  privateRecordSnapshot,
  privateRecordWire,
  type PrivateSendOperation
} from '../persistence/private-records.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from './verified-envelope.ts';
import { verifyOutboundLayerData } from './outbound-envelope-layers.ts';
import { sealCiphertextV2 } from './seal-template.ts';
import {
  browserExtensionScheduler,
  runExtensionAction,
  callExtension,
  stopExtensionAction,
  markExtensionWaitExpired,
  type ExtensionAction
} from './extension-scheduler.ts';
// Module-private strict self-layer SDK engine. Only the two factory-owned reader
// scopes below provide ownership/currentness; no caller cipher/event port exists.
function strictSelfDecryptor(
  identity: IdentitySession,
  owner: string,
  current: () => boolean
) {
  function port() {
    try {
      if (!current()) return undefined;
      const cipher = new ExtensionSigner().nip44;
      if (!current() || !cipher) return undefined;
      const method = cipher.decrypt;
      if (!current() || typeof method !== 'function') return undefined;
      const decrypt = method.bind(cipher);
      return current() ? { decrypt } : undefined;
    } catch {
      return undefined;
    }
  }
  async function fresh(action: ExtensionAction) {
    if (!current()) return false;
    const result = await callExtension(action, 'key', () =>
      new ExtensionSigner().getPublicKey()
    );
    if (!current()) return false;
    if (
      result.status !== 'settled' ||
      !result.current ||
      result.value !== owner
    ) {
      disconnectIdentity(identity);
      return false;
    }
    const cipher = port();
    // A callback may revoke only this scope. That cannot grant authority to
    // disconnect an otherwise current identity and its independent consumers.
    if (!current()) return false;
    if (!cipher) {
      disconnectIdentity(identity);
      return false;
    }
    return current();
  }
  async function open(
    action: ExtensionAction,
    outer: import('applesauce-core/helpers').NostrEvent
  ): Promise<
    | Readonly<{ status: 'opened'; rumorWire: string; sealWire: string }>
    | Readonly<{ status: 'stopped' | 'refused' | 'mismatch' }>
  > {
    if (!(await fresh(action))) return { status: 'stopped' };
    const outerPort = port();
    if (!outerPort || !current()) return { status: 'stopped' };
    const opened = await callExtension(action, 'decrypt', () =>
      outerPort.decrypt(outer.pubkey, outer.content)
    );
    if (!current()) return { status: 'stopped' };
    if (opened.status !== 'settled' || !opened.current)
      return { status: opened.status === 'denied' ? 'refused' : 'stopped' };
    if (
      typeof opened.value !== 'string' ||
      !boundedUtf8(opened.value, PRIVATE_TRANSPORT_BUDGETS.sealBytes)
    )
      return { status: 'mismatch' };
    const sealWire = opened.value,
      verified = verifyEnvelope(sealWire),
      seal = verified.ok && verifiedEnvelopeSnapshot(verified.value);
    if (
      !seal ||
      seal.pubkey !== owner ||
      seal.kind !== 13 ||
      seal.tags.length !== 0 ||
      !exactLocalFields(seal, [
        'id',
        'pubkey',
        'created_at',
        'kind',
        'tags',
        'content',
        'sig'
      ]) ||
      JSON.stringify(JSON.parse(sealWire)) !== sealWire ||
      !sealCiphertextV2(seal.content)
    )
      return { status: 'mismatch' };
    if (!(await fresh(action))) return { status: 'stopped' };
    const innerPort = port();
    if (!innerPort || !current()) return { status: 'stopped' };
    const decrypted = await callExtension(action, 'decrypt', () =>
      innerPort.decrypt(seal.pubkey, seal.content)
    );
    if (!current()) return { status: 'stopped' };
    if (decrypted.status !== 'settled' || !decrypted.current)
      return { status: decrypted.status === 'denied' ? 'refused' : 'stopped' };
    if (
      typeof decrypted.value !== 'string' ||
      !boundedUtf8(decrypted.value, PRIVATE_TRANSPORT_BUDGETS.rumorBytes)
    )
      return { status: 'mismatch' };
    return { status: 'opened', rumorWire: decrypted.value, sealWire };
  }
  return { fresh, open };
}

declare const readerBrand: unique symbol;
declare const recoveredBrand: unique symbol;
export type SelfRecoveryReader = Readonly<{ [readerBrand]: true }>;
export type RecoveredSelfEnvelope = Readonly<{ [recoveredBrand]: true }>;
type Snapshot = Readonly<{
  record: PrivateSendOperation;
  storedWire: string;
  rumorWire: string;
  sealWire: string;
}>;
export type SelfRecoveryReadResult =
  | Readonly<{ status: 'recovered'; envelope: RecoveredSelfEnvelope }>
  | Readonly<{
      status:
        | PrivateStorageFailure
        | 'invalid'
        | 'stopped'
        | 'busy'
        | 'refused'
        | 'mismatch'
        | 'lost_evidence';
    }>;
type Reader = {
  run(): Promise<SelfRecoveryReadResult>;
  stop(expired: boolean): void;
};
const readers = new WeakMap<SelfRecoveryReader, Reader>(),
  recovered = new WeakMap<
    RecoveredSelfEnvelope,
    { snapshot: Snapshot; current(): boolean }
  >();
// Actual owner-scoped storage and guarded SDK decryption are the only issuer.
// Detached JSON, metadata, SDK cache symbols and snapshots cannot mint this
// memory-only authenticated recovery proof or authorize a private relay effect.
export function captureSelfRecoveryReader(
  repository: PrivateStorageRepository,
  session: IdentitySession,
  localId: unknown,
  review: unknown
): SelfRecoveryReader | undefined {
  if (typeof window === 'undefined' || review !== 'reviewed_self_decryption')
    return undefined;
  const inputId = canonicalLocalId(localId),
    inputOwnership = identityMessagingOwnership(session);
  if (!inputId || !inputOwnership?.current()) return undefined;
  const id = inputId,
    ownership = inputOwnership;
  let stopped = false,
    started = false,
    active: ExtensionAction | undefined,
    proof: RecoveredSelfEnvelope | undefined,
    unsubscribe = () => {};
  const current = () => !stopped && ownership.current();
  function stop(expired: boolean) {
    stopped = true;
    if (active) {
      if (expired) markExtensionWaitExpired(active);
      else stopExtensionAction(active);
    }
    if (proof) recovered.delete(proof);
    proof = undefined;
    unsubscribe();
    unsubscribe = () => {};
  }
  const decryptor = strictSelfDecryptor(session, ownership.owner, current);
  const fresh = (action: ExtensionAction) => decryptor.fresh(action);
  async function run(): Promise<SelfRecoveryReadResult> {
    if (started || !current()) return { status: 'stopped' };
    started = true;
    if (!navigator.locks?.request) return { status: 'unavailable' };
    try {
      return await navigator.locks.request(
        'harvestcircle:owner:' + ownership.owner,
        { mode: 'exclusive', ifAvailable: true },
        async (lock): Promise<SelfRecoveryReadResult> => {
          if (!lock) return { status: 'busy' };
          if (!current()) return { status: 'stopped' };
          // Complete the readonly transaction before any extension await.
          const loaded = await loadPrivateRecord(
            repository,
            'private_sends',
            id
          );
          if (!current()) return { status: 'stopped' };
          if (!loaded.ok)
            return {
              status:
                loaded.reason === 'invalid_record'
                  ? 'lost_evidence'
                  : loaded.reason
            };
          const record = privateRecordSnapshot(
              loaded.value,
              ownership.owner,
              id
            ),
            storedWire = privateRecordWire(loaded.value, ownership.owner, id);
          if (
            !record ||
            !storedWire ||
            (record.family !== 'private_send_reservation' &&
              record.family !== 'private_send_operation')
          )
            return { status: 'invalid_scope' };
          if (record.family !== 'private_send_operation')
            return { status: 'lost_evidence' };
          const verified = verifyEnvelope(record.self.wire),
            outer = verified.ok && verifiedEnvelopeSnapshot(verified.value);
          if (
            !outer ||
            outer.id !== record.self.eventId ||
            outer.kind !== 1059 ||
            JSON.stringify(outer.tags) !==
              JSON.stringify([['p', record.owner]]) ||
            !exactLocalFields(outer, [
              'id',
              'pubkey',
              'created_at',
              'kind',
              'tags',
              'content',
              'sig'
            ]) ||
            !sealCiphertextV2(outer.content)
          )
            return { status: 'mismatch' };
          const scheduler = browserExtensionScheduler();
          if (!scheduler) return { status: 'unavailable' };
          const result = await runExtensionAction(
            scheduler,
            {
              owner: ownership.owner,
              session: ownership.session,
              operation: Symbol()
            },
            current,
            async (action): Promise<SelfRecoveryReadResult> => {
              active = action;
              const opened = await decryptor.open(action, outer);
              if (!current()) return { status: 'stopped' };
              if (opened.status !== 'opened') return opened;
              const { rumorWire, sealWire } = opened;
              if (
                !verifyOutboundLayerData(
                  rumorWire,
                  rumorWire,
                  sealWire,
                  record.self.wire,
                  record.owner,
                  record.peer,
                  'self'
                )
              )
                return { status: 'mismatch' };
              const rumor = JSON.parse(rumorWire) as {
                id: string;
                created_at: number;
              };
              if (
                JSON.stringify(JSON.parse(rumorWire)) !== rumorWire ||
                rumor.id !== record.rumorHash ||
                rumor.created_at !== record.createdAt
              )
                return { status: 'mismatch' };
              if (!(await fresh(action))) return { status: 'stopped' };
              const readback = await loadPrivateRecord(
                repository,
                'private_sends',
                id
              );
              if (!current()) return { status: 'stopped' };
              if (!readback.ok)
                return {
                  status:
                    readback.reason === 'invalid_record'
                      ? 'lost_evidence'
                      : readback.reason
                };
              if (
                privateRecordWire(readback.value, record.owner, id) !==
                storedWire
              )
                return { status: 'conflict' };
              if (!(await fresh(action))) return { status: 'stopped' };
              const token = Object.freeze({}) as RecoveredSelfEnvelope;
              recovered.set(token, {
                snapshot: { record, storedWire, rumorWire, sealWire },
                current
              });
              proof = token;
              return { status: 'recovered', envelope: token };
            }
          );
          if (!current()) return { status: 'stopped' };
          if (result.status === 'busy') return { status: 'busy' };
          return result.status === 'completed' &&
            'value' in result &&
            result.value
            ? result.value
            : { status: result.status === 'denied' ? 'refused' : 'stopped' };
        }
      );
    } catch {
      return { status: current() ? 'refused' : 'stopped' };
    } finally {
      active = undefined;
    }
  }
  const token = Object.freeze({}) as SelfRecoveryReader;
  readers.set(token, { run, stop });
  unsubscribe = subscribeIdentityInvalidation(session, () => {
    if (!ownership.current()) stop(false);
  });
  return current() ? token : undefined;
}
export function readSelfRecoveryEnvelope(
  reader: SelfRecoveryReader
): Promise<SelfRecoveryReadResult> {
  return readers.get(reader)?.run() ?? Promise.resolve({ status: 'invalid' });
}
export function stopSelfRecoveryReader(reader: SelfRecoveryReader): void {
  readers.get(reader)?.stop(false);
}
export function expireSelfRecoveryReaderWait(reader: SelfRecoveryReader): void {
  readers.get(reader)?.stop(true);
}
export function recoveredSelfEnvelopeSnapshot(
  token: RecoveredSelfEnvelope
): Snapshot | undefined {
  const saved = recovered.get(token);
  return saved?.current()
    ? (JSON.parse(JSON.stringify(saved.snapshot)) as Snapshot)
    : undefined;
}

declare const probeNonceReaderBrand: unique symbol;
export type InboxProbeNonceReader = Readonly<{ [probeNonceReaderBrand]: true }>;
declare const probeNonceProofBrand: unique symbol;
export type InboxProbeNonceProof = Readonly<{ [probeNonceProofBrand]: true }>;
export type InboxProbeNonceResult =
  | Readonly<{ status: 'nonce_verified'; proof: InboxProbeNonceProof }>
  | Readonly<{
      status:
        | PrivateStorageFailure
        | 'invalid'
        | 'stopped'
        | 'busy'
        | 'refused'
        | 'mismatch'
        | 'lost_evidence';
    }>;
type ProbeReader = {
  run(): Promise<InboxProbeNonceResult>;
  stop(expired: boolean): void;
};
const probeReaders = new WeakMap<InboxProbeNonceReader, ProbeReader>();
const probeObservationReaders = new WeakMap<
  InboxProbeReadbackAttemptResult,
  Readonly<{
    repository: PrivateStorageRepository;
    identity: IdentitySession;
    session: PrivateSession;
    policy: RelayPolicy;
    reader: InboxProbeNonceReader;
  }>
>();
const probeNonceProofs = new WeakMap<
  InboxProbeNonceProof,
  Readonly<{
    repository: PrivateStorageRepository;
    session: PrivateSession;
    policy: RelayPolicy;
    observation: InboxProbeReadbackAttemptResult;
    acknowledgement: InboxProbeAcknowledgement;
    original: string;
    current(): boolean;
  }>
>();
function immutableProbe(
  record: import('../persistence/private-records.ts').PrivateAccessProbe
) {
  return JSON.stringify({
    owner: record.owner,
    id: record.id,
    rumorHash: record.rumorHash,
    createdAt: record.createdAt,
    binding: record.binding,
    self: record.self
  });
}
// Genuine positive source transport, actual identity/private session, original
// nonce and native custody are all required. Capture never calls the provider.
export function captureInboxProbeNonceReader(
  repository: PrivateStorageRepository,
  identity: IdentitySession,
  session: PrivateSession,
  policy: RelayPolicy,
  observation: InboxProbeReadbackAttemptResult,
  review: unknown
): InboxProbeNonceReader | undefined {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_inbox_probe_nonce_decryption'
  )
    return undefined;
  try {
    const effect = inboxProbeReadbackAttemptEvidence(observation);
    if (!effect || effect.repository !== repository) return undefined;
    const acknowledgement = effect.acknowledgement,
      binding = inboxProbePublicationBinding(
        repository,
        session,
        acknowledgement,
        policy
      );
    if (!binding?.record.self) return undefined;
    const identityOwner = identityMessagingOwnership(identity);
    if (
      !identityOwner?.current() ||
      binding.record.owner !== identityOwner.owner
    )
      return undefined;
    const privateOwner = privateSessionOwnership(session);
    if (
      !privateOwner?.current() ||
      identityOwner.owner !== privateOwner.owner ||
      identityOwner.session !== privateOwner.session
    )
      return undefined;
    const fact = JSON.parse(
      effect.receiptWire
    ) as import('../persistence/private-records.ts').PrivateAccessProbeReceipt;
    if (
      fact.role !== 'access_readback' ||
      fact.status !== 'readback' ||
      fact.eventId !== binding.record.self.eventId ||
      !binding.record.binding.readTargets.includes(fact.origin) ||
      !inboxProbeReadbackMatches(
        binding.record.self,
        binding.record.owner,
        fact.readbackWire
      ) ||
      !inboxProbeRumorWire(binding.identity, session)
    )
      return undefined;
    const previous = probeObservationReaders.get(observation);
    if (previous)
      return previous.repository === repository &&
        previous.identity === identity &&
        previous.session === session &&
        previous.policy === policy
        ? previous.reader
        : undefined;
    const original = immutableProbe(binding.record),
      token = Object.freeze({}) as InboxProbeNonceReader;
    let stopped = false,
      started = false,
      pending = false,
      active: ExtensionAction | undefined,
      proof: InboxProbeNonceProof | undefined,
      last: InboxProbeNonceResult | undefined;
    const listeners = new Map<symbol, () => void>();
    function current() {
      const fresh = inboxProbePublicationBinding(
        repository,
        session,
        acknowledgement,
        policy
      );
      return (
        !stopped &&
        identityOwner!.current() &&
        privateOwner!.current() &&
        !!fresh &&
        fresh.identity === binding!.identity &&
        immutableProbe(fresh.record) === original &&
        !!inboxProbeRumorWire(fresh.identity, session)
      );
    }
    function stop(expired: boolean) {
      stopped = true;
      if (active) {
        if (expired) markExtensionWaitExpired(active);
        else stopExtensionAction(active);
      }
      if (proof) probeNonceProofs.delete(proof);
      proof = undefined;
      for (const [id, off] of listeners) {
        off();
        listeners.delete(id);
      }
    }
    function retain(register: () => () => void) {
      const off = register();
      if (stopped) off();
      else listeners.set(Symbol(), off);
    }
    const decryptor = strictSelfDecryptor(
      identity,
      identityOwner.owner,
      current
    );
    async function run(): Promise<InboxProbeNonceResult> {
      if (!current()) {
        stop(false);
        return { status: 'stopped' };
      }
      if (pending) return { status: 'busy' };
      if (started) return last ?? { status: 'stopped' };
      started = true;
      pending = true;
      try {
        if (!navigator.locks?.request)
          return (last = { status: 'unavailable' });
        last = await navigator.locks.request(
          'harvestcircle:owner:' + identityOwner!.owner,
          { mode: 'exclusive', ifAvailable: true },
          async (lock): Promise<InboxProbeNonceResult> => {
            if (!lock) return { status: 'busy' };
            if (!current()) return { status: 'stopped' };
            const captured = inboxProbePublicationBinding(
              repository,
              session,
              acknowledgement,
              policy
            );
            if (!captured?.record.self) return { status: 'stopped' };
            const loaded = await observePrivateRecord(
              repository,
              'private_sends',
              captured.record.id
            );
            if (!current()) return { status: 'stopped' };
            if (!loaded.ok) return { status: loaded.reason };
            if (
              loaded.value.wire !== captured.wire ||
              privateRecordWire(
                loaded.value.handle,
                captured.record.owner,
                captured.record.id
              ) !== captured.wire ||
              inboxProbePublicationBinding(
                repository,
                session,
                acknowledgement,
                policy
              )?.wire !== captured.wire
            )
              return { status: 'conflict' };
            const checked = verifyEnvelope(captured.record.self.wire),
              outer = checked.ok && verifiedEnvelopeSnapshot(checked.value);
            if (
              !outer ||
              outer.kind !== 1059 ||
              outer.id !== captured.record.self.eventId ||
              JSON.stringify(outer.tags) !==
                JSON.stringify([['p', captured.record.owner]]) ||
              !exactLocalFields(outer, [
                'id',
                'pubkey',
                'created_at',
                'kind',
                'tags',
                'content',
                'sig'
              ]) ||
              !sealCiphertextV2(outer.content)
            )
              return { status: 'mismatch' };
            const scheduler = browserExtensionScheduler();
            if (!scheduler) return { status: 'unavailable' };
            const result = await runExtensionAction(
              scheduler,
              {
                owner: identityOwner!.owner,
                session: identityOwner!.session,
                operation: Symbol()
              },
              current,
              async (action): Promise<InboxProbeNonceResult> => {
                active = action;
                const opened = await decryptor.open(action, outer);
                if (!current()) return { status: 'stopped' };
                if (opened.status !== 'opened') return opened;
                const expected = inboxProbeRumorWire(
                  captured.identity,
                  session
                );
                if (!expected || opened.rumorWire !== expected)
                  return { status: 'mismatch' };
                const rumor = JSON.parse(opened.rumorWire) as {
                  id: string;
                  created_at: number;
                  pubkey: string;
                };
                if (
                  rumor.id !== captured.record.rumorHash ||
                  rumor.created_at !== captured.record.createdAt ||
                  rumor.pubkey !== captured.record.owner
                )
                  return { status: 'mismatch' };
                // All callback-capable provider/cipher work finishes before the
                // final physical read. No extension callback follows that read.
                if (!(await decryptor.fresh(action)))
                  return { status: 'stopped' };
                const after = await observePrivateRecord(
                  repository,
                  'private_sends',
                  captured.record.id
                );
                if (!current()) return { status: 'stopped' };
                if (!after.ok) return { status: after.reason };
                if (
                  after.value.wire !== captured.wire ||
                  privateRecordWire(
                    after.value.handle,
                    captured.record.owner,
                    captured.record.id
                  ) !== captured.wire ||
                  inboxProbePublicationBinding(
                    repository,
                    session,
                    acknowledgement,
                    policy
                  )?.wire !== captured.wire
                )
                  return { status: 'conflict' };
                if (
                  !current() ||
                  inboxProbeRumorWire(captured.identity, session) !== expected
                )
                  return { status: 'stopped' };
                const issued = Object.freeze({}) as InboxProbeNonceProof;
                probeNonceProofs.set(issued, {
                  repository,
                  session,
                  policy,
                  observation,
                  acknowledgement,
                  original,
                  current
                });
                proof = issued;
                return { status: 'nonce_verified', proof: issued };
              }
            );
            if (!current()) return { status: 'stopped' };
            if (result.status === 'busy') return { status: 'busy' };
            return result.status === 'completed' &&
              'value' in result &&
              result.value
              ? result.value
              : { status: result.status === 'denied' ? 'refused' : 'stopped' };
          }
        );
        return last;
      } catch {
        return (last = { status: current() ? 'refused' : 'stopped' });
      } finally {
        active = undefined;
        pending = false;
      }
    }
    probeReaders.set(token, { run, stop });
    // Reserve before callback-capable subscriptions; reentrant capture reuses
    // this exact original reader and cannot reset or start another SDK action.
    probeObservationReaders.set(observation, {
      repository,
      identity,
      session,
      policy,
      reader: token
    });
    retain(() =>
      subscribeIdentityInvalidation(identity, () => {
        if (!identityOwner.current()) stop(false);
      })
    );
    retain(() => subscribePrivateSessionClose(session, () => stop(false)));
    return current() ? token : undefined;
  } catch {
    return undefined;
  }
}
export function readInboxProbeNonce(
  reader: InboxProbeNonceReader
): Promise<InboxProbeNonceResult> {
  return (
    probeReaders.get(reader)?.run() ?? Promise.resolve({ status: 'invalid' })
  );
}
export function stopInboxProbeNonceReader(reader: InboxProbeNonceReader) {
  probeReaders.get(reader)?.stop(false);
}
export function expireInboxProbeNonceReaderWait(reader: InboxProbeNonceReader) {
  probeReaders.get(reader)?.stop(true);
}
export function inboxProbeNonceProofBinding(
  proof: InboxProbeNonceProof,
  repository: PrivateStorageRepository,
  session: PrivateSession,
  policy: RelayPolicy
) {
  const saved = probeNonceProofs.get(proof);
  if (
    !saved ||
    saved.repository !== repository ||
    saved.session !== session ||
    saved.policy !== policy ||
    !saved.current()
  )
    return undefined;
  const binding = inboxProbePublicationBinding(
    repository,
    session,
    saved.acknowledgement,
    policy
  );
  if (!binding || immutableProbe(binding.record) !== saved.original)
    return undefined;
  return {
    acknowledgement: saved.acknowledgement,
    observation: saved.observation,
    identity: binding.identity,
    owner: binding.record.owner,
    command: binding.record.id,
    rumorHash: binding.record.rumorHash,
    eventId: binding.record.self!.eventId,
    current: () => saved.current()
  };
}
