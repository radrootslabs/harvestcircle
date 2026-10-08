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
      result.value !== ownership.owner ||
      !port()
    ) {
      disconnectIdentity(session);
      return false;
    }
    return current();
  }
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
          if (!record || !storedWire || record.family === 'received_envelope')
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
              if (!(await fresh(action))) return { status: 'stopped' };
              const outerPort = port();
              if (!outerPort || !current()) return { status: 'stopped' };
              const opened = await callExtension(action, 'decrypt', () =>
                outerPort.decrypt(outer.pubkey, outer.content)
              );
              if (!current()) return { status: 'stopped' };
              if (opened.status !== 'settled' || !opened.current)
                return {
                  status: opened.status === 'denied' ? 'refused' : 'stopped'
                };
              if (
                typeof opened.value !== 'string' ||
                !boundedUtf8(opened.value, PRIVATE_TRANSPORT_BUDGETS.sealBytes)
              )
                return { status: 'mismatch' };
              const sealWire = opened.value,
                verifiedSeal = verifyEnvelope(sealWire),
                seal =
                  verifiedSeal.ok &&
                  verifiedEnvelopeSnapshot(verifiedSeal.value);
              if (
                !seal ||
                seal.pubkey !== record.owner ||
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
                return {
                  status: decrypted.status === 'denied' ? 'refused' : 'stopped'
                };
              if (
                typeof decrypted.value !== 'string' ||
                !boundedUtf8(
                  decrypted.value,
                  PRIVATE_TRANSPORT_BUDGETS.rumorBytes
                )
              )
                return { status: 'mismatch' };
              const rumorWire = decrypted.value;
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
  unsubscribe = subscribeIdentityInvalidation(session, () => stop(false));
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
