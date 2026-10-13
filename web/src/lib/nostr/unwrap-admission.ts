import { ExtensionSigner } from 'applesauce-signers';
import type { UnsignedEvent } from 'applesauce-core/helpers';
import { PRIVATE_TRANSPORT_BUDGETS } from '../config/budgets.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import {
  incomingGiftWrapTags,
  parseIncomingLayer,
  readIncomingPairRumor
} from './incoming-wire.ts';
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
  privateRecordWire
} from '../persistence/private-records.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from './verified-envelope.ts';
import { sealCiphertextV2 } from './seal-template.ts';
import {
  browserExtensionScheduler,
  runExtensionAction,
  callExtension,
  stopExtensionAction,
  markExtensionWaitExpired,
  type ExtensionAction
} from './extension-scheduler.ts';

const unsignedFields = [
  'id',
  'pubkey',
  'created_at',
  'kind',
  'tags',
  'content'
];
const signedFields = [...unsignedFields, 'sig'];
function outerLayer(raw: unknown, owner: string) {
  const parsed = parseIncomingLayer(
    raw,
    PRIVATE_TRANSPORT_BUDGETS.envelopeBytes
  );
  if (
    !exactLocalFields(parsed, signedFields) ||
    parsed.kind !== 1059 ||
    !incomingGiftWrapTags(parsed.tags, owner) ||
    !sealCiphertextV2(parsed.content)
  )
    return undefined;
  const verified = verifyEnvelope(raw);
  return verified.ok ? verifiedEnvelopeSnapshot(verified.value) : undefined;
}
function sealLayer(raw: unknown) {
  const parsed = parseIncomingLayer(raw, PRIVATE_TRANSPORT_BUDGETS.sealBytes);
  if (
    !exactLocalFields(parsed, signedFields) ||
    parsed.kind !== 13 ||
    !Array.isArray(parsed.tags) ||
    parsed.tags.length !== 0 ||
    !sealCiphertextV2(parsed.content)
  )
    return undefined;
  const verified = verifyEnvelope(raw);
  return verified.ok ? verifiedEnvelopeSnapshot(verified.value) : undefined;
}
function rumorLayer(
  raw: unknown,
  sender: string
): Readonly<UnsignedEvent & { id: string }> | undefined {
  return readIncomingPairRumor(raw, sender)?.original;
}
// Detached inspection only: supplied plaintext cannot prove ciphertext
// correspondence. Only the scoped actual SDK issuer below mints nested custody.
// Owner room membership and inbound/archive role are a separate admission.
export function verifyReceivedLayerData(
  outerRaw: unknown,
  sealRaw: unknown,
  rumorRaw: unknown,
  expectedOwner: unknown
): boolean {
  const owner = canonicalPublicKey(expectedOwner);
  if (!owner) return false;
  try {
    const outer = outerLayer(outerRaw, owner),
      seal = sealLayer(sealRaw);
    return !!outer && !!seal && !!rumorLayer(rumorRaw, seal.pubkey);
  } catch {
    return false;
  }
}

declare const readerBrand: unique symbol;
declare const nestedBrand: unique symbol;
export type ReceivedUnwrap = Readonly<{ [readerBrand]: true }>;
export type ReceivedNestedEnvelope = Readonly<{ [nestedBrand]: true }>;
type Snapshot = Readonly<{
  owner: string;
  outerId: string;
  outerWire: string;
  sealWire: string;
  rumorWire: string;
}>;
export type ReceivedUnwrapResult =
  | Readonly<{ status: 'authenticated'; envelope: ReceivedNestedEnvelope }>
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
  run(): Promise<ReceivedUnwrapResult>;
  stop(expired: boolean): void;
};
const readers = new WeakMap<ReceivedUnwrap, Reader>();
const nested = new WeakMap<
  ReceivedNestedEnvelope,
  { snapshot: Snapshot; session: symbol; current: () => boolean }
>();
// No common unwrap helper/cache or caller event objects are consulted. Original
// owner storage finishes each IDB transaction before serialized SDK work begins.
export function captureReceivedUnwrap(
  repository: PrivateStorageRepository,
  session: IdentitySession,
  eventId: unknown,
  review: unknown
): ReceivedUnwrap | undefined {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_inbox_unlock' ||
    typeof eventId !== 'string' ||
    !/^[0-9a-f]{64}$/u.test(eventId)
  )
    return undefined;
  const captured = identityMessagingOwnership(session);
  if (!captured?.current()) return undefined;
  const ownership = captured,
    id = eventId;
  let stopped = false,
    started = false,
    active: ExtensionAction | undefined,
    proof: ReceivedNestedEnvelope | undefined,
    unsubscribe = () => {};
  const current = () => !stopped && ownership.current();
  function stop(expired: boolean) {
    stopped = true;
    if (active) {
      if (expired) markExtensionWaitExpired(active);
      else stopExtensionAction(active);
    }
    if (proof) nested.delete(proof);
    proof = undefined;
    unsubscribe();
    unsubscribe = () => {};
  }
  function port() {
    try {
      if (!current()) return undefined;
      const cipher = new ExtensionSigner().nip44;
      if (!cipher || !current() || typeof cipher.decrypt !== 'function')
        return undefined;
      const decrypt = cipher.decrypt.bind(cipher);
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
  async function run(): Promise<ReceivedUnwrapResult> {
    if (started || !current()) return { status: 'stopped' };
    started = true;
    try {
      const loaded = await loadPrivateRecord(
        repository,
        'received_envelopes',
        id
      );
      if (!current()) return { status: 'stopped' };
      if (!loaded.ok)
        return {
          status:
            loaded.reason === 'invalid_record' ? 'lost_evidence' : loaded.reason
        };
      const record = privateRecordSnapshot(loaded.value, ownership.owner, id),
        storedWire = privateRecordWire(loaded.value, ownership.owner, id);
      if (!record || record.family !== 'received_envelope' || !storedWire)
        return { status: 'invalid_scope' };
      const outer = outerLayer(record.outer, ownership.owner);
      if (!outer || outer.id !== id) return { status: 'mismatch' };
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
        async (action): Promise<ReceivedUnwrapResult> => {
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
          const seal = sealLayer(opened.value);
          if (!seal || typeof opened.value !== 'string')
            return { status: 'mismatch' };
          const sealWire = opened.value;
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
            !rumorLayer(decrypted.value, seal.pubkey)
          )
            return { status: 'mismatch' };
          const rumorWire = decrypted.value;
          if (!(await fresh(action))) return { status: 'stopped' };
          const readback = await loadPrivateRecord(
            repository,
            'received_envelopes',
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
            privateRecordWire(readback.value, ownership.owner, id) !==
            storedWire
          )
            return { status: 'conflict' };
          // Last actual storage readback is followed by no SDK/asynchronous
          // wait before minting original retained-record custody.
          const token = Object.freeze({}) as ReceivedNestedEnvelope;
          nested.set(token, {
            session: ownership.session,
            snapshot: {
              owner: ownership.owner,
              outerId: id,
              outerWire: record.outer,
              sealWire,
              rumorWire
            },
            current
          });
          proof = token;
          return { status: 'authenticated', envelope: token };
        }
      );
      if (!current()) return { status: 'stopped' };
      if (result.status === 'busy') return { status: 'busy' };
      return result.status === 'completed' && 'value' in result && result.value
        ? result.value
        : { status: result.status === 'denied' ? 'refused' : 'stopped' };
    } catch {
      return { status: current() ? 'mismatch' : 'stopped' };
    } finally {
      active = undefined;
      if (!proof) {
        unsubscribe();
        unsubscribe = () => {};
      }
    }
  }
  const token = Object.freeze({}) as ReceivedUnwrap;
  readers.set(token, { run, stop });
  unsubscribe = subscribeIdentityInvalidation(session, () => {
    if (!ownership.current()) stop(false);
  });
  return current() ? token : undefined;
}
export function unwrapReceivedEnvelope(
  reader: ReceivedUnwrap
): Promise<ReceivedUnwrapResult> {
  return readers.get(reader)?.run() ?? Promise.resolve({ status: 'invalid' });
}
export function stopReceivedUnwrap(reader: ReceivedUnwrap): void {
  readers.get(reader)?.stop(false);
}
export function expireReceivedUnwrapWait(reader: ReceivedUnwrap): void {
  readers.get(reader)?.stop(true);
}
export function receivedNestedEnvelopeSnapshot(
  token: ReceivedNestedEnvelope
): Snapshot | undefined {
  const saved = nested.get(token);
  return saved?.current()
    ? (JSON.parse(JSON.stringify(saved.snapshot)) as Snapshot)
    : undefined;
}

// HCP093 original-generation custody port. This observation is not a Send,
// persistence, inbox-readiness or AUTH capability. Structural copies never
// replace the original nested proof in the WeakMap.
export function receivedNestedEnvelopeOwnership(
  token: ReceivedNestedEnvelope
):
  Readonly<{ owner: string; session: symbol; current(): boolean }> | undefined {
  const saved = nested.get(token);
  return saved?.current()
    ? {
        owner: saved.snapshot.owner,
        session: saved.session,
        current: saved.current
      }
    : undefined;
}
