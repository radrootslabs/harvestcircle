import { canonicalPublicKey } from '../contracts/public-key.ts';
import { canonicalRelayOrigin } from '../config/relays.ts';
import { PRIVATE_TRANSPORT_BUDGETS } from '../config/budgets.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { safeUnsignedInteger } from '../nostr/envelope-bounds.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';
import {
  privateSessionOwnership,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  decodePrivateRecord,
  privateRecordSnapshot,
  privateRecordWire,
  type PrivateRecordHandle
} from './private-records.ts';
import {
  loadPrivateRecord,
  retainPrivateReceivedRecord,
  type PrivateStorageRepository,
  type PrivateStorageFailure
} from './private-storage.ts';
declare const envelopeBrand: unique symbol;
export type InboxEnvelope = Readonly<{ [envelopeBrand]: true }>;
type Admitted = Readonly<{
  owner: string;
  id: string;
  source: string;
  record: PrivateRecordHandle;
}>;
const envelopes = new WeakMap<InboxEnvelope, Admitted>();
// Outer authentication only. Ciphertext is untrusted until later nested seal,
// rumor and sender/participant admission; this proof never grants decryption.
export function admitInboxEnvelope(
  raw: unknown,
  expectedOwner: unknown,
  observedSource: unknown,
  observedAt: unknown
): InboxEnvelope | undefined {
  const owner = canonicalPublicKey(expectedOwner),
    source = canonicalRelayOrigin(observedSource),
    at = safeUnsignedInteger(observedAt);
  if (
    !owner ||
    !source ||
    at === undefined ||
    typeof raw !== 'string' ||
    !boundedUtf8(raw, PRIVATE_TRANSPORT_BUDGETS.envelopeBytes)
  )
    return undefined;
  const verified = verifyEnvelope(raw),
    event = verified.ok && verifiedEnvelopeSnapshot(verified.value);
  if (!event || event.kind !== 1059) return undefined;
  const record = decodePrivateRecord(
    JSON.stringify({
      schema: 1,
      family: 'received_envelope',
      owner,
      id: event.id,
      revision: 0,
      outer: raw,
      observedAtMilliseconds: at,
      sources: [source],
      read: null
    }),
    owner,
    event.id
  );
  if (!record.ok) return undefined;
  const token = Object.freeze({}) as InboxEnvelope;
  envelopes.set(token, { owner, id: event.id, source, record: record.value });
  return token;
}
export function inboxEnvelopeSnapshot(token: InboxEnvelope) {
  const row = envelopes.get(token);
  return row && privateRecordSnapshot(row.record, row.owner, row.id);
}
export type InboxRetentionResult = Readonly<{
  status:
    | 'retained'
    | 'duplicate'
    | 'invalid'
    | 'stopped'
    | 'busy'
    | PrivateStorageFailure;
}>;
// Ciphertext-only bookkeeping in the original owner namespace. No SDK work in
// IDB; request success is insufficient without actual whole-wire readback.
export async function retainInboxEnvelope(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  envelope: InboxEnvelope,
  cancellation?: AbortSignal
): Promise<InboxRetentionResult> {
  const admitted = envelopes.get(envelope),
    ownership = privateSessionOwnership(session);
  if (
    !admitted ||
    !ownership?.current() ||
    ownership.owner !== admitted.owner ||
    typeof window === 'undefined' ||
    !navigator.locks?.request
  )
    return { status: 'invalid' };
  if (cancellation?.aborted) return { status: 'stopped' };
  try {
    return await navigator.locks.request(
      'harvestcircle:owner:' + admitted.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock) => {
        if (!lock) return { status: 'busy' };
        if (!ownership.current() || cancellation?.aborted)
          return { status: 'stopped' };
        const committed = await retainPrivateReceivedRecord(
          repository,
          session,
          admitted.record,
          cancellation
        );
        if (!ownership.current() || cancellation?.aborted)
          return { status: 'stopped' };
        if (!committed.ok) return { status: committed.reason };
        if (!committed.value.changed) return { status: committed.value.status };
        const readback = await loadPrivateRecord(
          repository,
          'received_envelopes',
          admitted.id
        );
        if (!ownership.current() || cancellation?.aborted)
          return { status: 'stopped' };
        if (!readback.ok) return { status: 'unknown_completion' };
        if (
          privateRecordWire(readback.value, admitted.owner, admitted.id) !==
          privateRecordWire(committed.value.record, admitted.owner, admitted.id)
        )
          return { status: 'conflict' };
        return { status: committed.value.status };
      }
    );
  } catch {
    return { status: 'unknown_completion' };
  }
}
