import { canonicalPublicKey } from '../contracts/public-key.ts';
import { canonicalRelayOrigin } from '../config/relays.ts';
import { PRIVATE_TRANSPORT_BUDGETS, RELAY_BUDGETS } from '../config/budgets.ts';
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
  commitPrivateRecord,
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
  envelope: InboxEnvelope
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
  try {
    return await navigator.locks.request(
      'harvestcircle:owner:' + admitted.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock) => {
        if (!lock) return { status: 'busy' };
        if (!ownership.current()) return { status: 'stopped' };
        const loaded = await loadPrivateRecord(
          repository,
          'received_envelopes',
          admitted.id
        );
        if (!ownership.current()) return { status: 'stopped' };
        let next = admitted.record,
          base: PrivateRecordHandle | null = null,
          duplicate = false;
        if (loaded.ok) {
          const row = privateRecordSnapshot(
            loaded.value,
            admitted.owner,
            admitted.id
          );
          if (!row || row.family !== 'received_envelope')
            return { status: 'conflict' };
          // Keep the first valid signed outer and first observation. Equal outer
          // IDs are deduplication, not trusted inner-message identity/display.
          duplicate = true;
          base = loaded.value;
          if (row.sources.includes(admitted.source))
            return { status: 'duplicate' };
          if (
            row.sources.length >= RELAY_BUDGETS.inbox ||
            row.revision === Number.MAX_SAFE_INTEGER
          )
            return { status: 'capacity' };
          const decoded = decodePrivateRecord(
            JSON.stringify({
              ...row,
              revision: row.revision + 1,
              sources: [...row.sources, admitted.source]
            }),
            admitted.owner,
            admitted.id
          );
          if (!decoded.ok) return { status: 'invalid' };
          next = decoded.value;
        } else if (loaded.reason !== 'invalid_record')
          return { status: loaded.reason };
        if (!ownership.current()) return { status: 'stopped' };
        const committed = await commitPrivateRecord(repository, next, base);
        if (!committed.ok) return { status: committed.reason };
        const readback = await loadPrivateRecord(
          repository,
          'received_envelopes',
          admitted.id
        );
        if (!ownership.current()) return { status: 'stopped' };
        if (!readback.ok) return { status: 'unknown_completion' };
        if (
          privateRecordWire(readback.value, admitted.owner, admitted.id) !==
          privateRecordWire(next, admitted.owner, admitted.id)
        )
          return { status: 'conflict' };
        return { status: duplicate ? 'duplicate' : 'retained' };
      }
    );
  } catch {
    return { status: 'unknown_completion' };
  }
}
