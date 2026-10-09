import { exactLocalFields } from '../contracts/local-records.ts';
import { newLocalId } from '../private-handles.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import { safeUnsignedInteger } from '../nostr/envelope-bounds.ts';
import {
  browserDatabaseState,
  browserDatabaseTransaction,
  type BrowserDatabase
} from './database.ts';
import {
  decodePrivateRecord,
  privateRecordSnapshot,
  privateRecordWire,
  type ReceivedEnvelopeRecord
} from './private-records.ts';
import {
  decodeConversationMapping,
  conversationMappingSnapshot
} from './records.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  conversationSnapshot,
  conversationOwnership,
  conversationEnvelopeIdentity,
  type AdmittedConversation
} from '../messaging/admit-conversation.ts';
import {
  unlockedSessionOwnership,
  unlockedConversationCurrent,
  readUnlockedMessages,
  type UnlockedSession
} from '../messaging/unlocked-session.ts';

export const MESSAGE_METADATA_DISCLOSURE =
  'Read/unread is local. Ciphertext and minimal owner, peer, opaque conversation identifier, verified rumor read flag, routing and use times may persist in this browser. Someone with local access may learn peers and use times; this is not full-device encryption or backup. No private body, subject or product association is indexed. No remote Seen/Read or guaranteed notification is sent.';
declare const metadataBrand: unique symbol;
export type MessageMetadata = Readonly<{ [metadataBrand]: true }>;
type Failure =
  | 'invalid'
  | 'stopped'
  | 'busy'
  | 'unavailable'
  | 'aborted'
  | 'unknown_completion'
  | 'corrupt_record'
  | 'capacity'
  | 'conflict';
type Pair = Readonly<{
  owner: string;
  id: string;
  peer: string;
  wire: string;
  bytes: number;
}>;
type Cipher = Readonly<{
  record: ReceivedEnvelopeRecord;
  wire: string;
  bytes: number;
}>;
type Rows = Readonly<{ received: readonly Cipher[]; pairs: readonly Pair[] }>;
type Result<T> =
  Readonly<{ ok: true; value: T }> | Readonly<{ ok: false; reason: Failure }>;
type Scope = Readonly<{
  database: BrowserDatabase;
  owner: string;
  session: symbol;
  unlocked: UnlockedSession;
  current(): boolean;
}>;
type Snapshot =
  | Readonly<{
      status: 'ready';
      owner: string;
      readRumors: readonly string[];
      pairs: readonly Readonly<{ id: string; peer: string }>[];
    }>
  | Readonly<{ status: Failure }>;
type Display =
  | Readonly<{
      status: 'saved' | 'existing';
      conversationId: string;
      rumorId: string;
    }>
  | Readonly<{ status: Failure }>
  | Readonly<{
      status: 'unknown_completion';
      phase: 'readback';
      reason: Failure;
    }>;
type Controller = {
  snapshot(): Promise<Snapshot>;
  display(room: AdmittedConversation, review: unknown): Promise<Display>;
  messages(): ReturnType<typeof readUnlockedMessages>;
  close(): void;
};
const controllers = new WeakMap<MessageMetadata, Controller>();
const failed = (reason: Failure): Result<never> => ({ ok: false, reason });
function cipherRow(
  value: unknown,
  owner: string,
  id: unknown
): Cipher | undefined {
  if (
    !exactLocalFields(value, ['owner', 'id', 'wire']) ||
    value.owner !== owner ||
    value.id !== id ||
    typeof value.wire !== 'string'
  )
    return;
  const decoded = decodePrivateRecord(value.wire, owner, id);
  if (!decoded.ok) return;
  const record = privateRecordSnapshot(decoded.value, owner, id);
  return record?.family === 'received_envelope' &&
    privateRecordWire(decoded.value, owner, id) === value.wire
    ? {
        record,
        wire: value.wire,
        bytes: new TextEncoder().encode(JSON.stringify(value)).length
      }
    : undefined;
}
function pairRow(value: unknown, owner: string, id: unknown): Pair | undefined {
  if (
    !exactLocalFields(value, ['owner', 'id', 'wire']) ||
    value.owner !== owner ||
    value.id !== id ||
    typeof value.wire !== 'string'
  )
    return;
  const decoded = decodeConversationMapping(value.wire, owner, id);
  if (!decoded.ok) return;
  const row = conversationMappingSnapshot(decoded.value, owner, id);
  return row
    ? {
        owner: row.owner,
        id: row.id,
        peer: row.peer,
        wire: value.wire,
        bytes: new TextEncoder().encode(JSON.stringify(value)).length
      }
    : undefined;
}
// Bounded owner-only strict scans. Existing schema/received quota unchanged;
// minimum pair mappings use the same conservative logical receive ceiling.
// No SDK/network await in any transaction; completion precedes readback credit.
function transaction<T>(
  scope: Scope,
  mode: 'readonly' | 'readwrite',
  finish: (
    rows: Rows,
    tx: IDBTransaction,
    refuse: (reason: Failure) => void
  ) => Result<T> | undefined
): Promise<Result<T>> {
  let tx: IDBTransaction;
  try {
    if (!scope.current()) return Promise.resolve(failed('stopped'));
    tx = browserDatabaseTransaction(
      scope.database,
      ['received_envelopes', 'conversations'],
      mode
    );
  } catch {
    return Promise.resolve(failed('unavailable'));
  }
  return new Promise((resolve) => {
    let result: Result<T> | undefined,
      received: readonly Cipher[] = Array.from<Cipher>([]),
      pairs: readonly Pair[] = Array.from<Pair>([]),
      pending = 2,
      receivedBytes = 0,
      pairBytes = 0;
    function refuse(reason: Failure) {
      result = failed(reason);
      try {
        tx.abort();
      } catch {
        result = failed('unknown_completion');
      }
    }
    tx.addEventListener('complete', () =>
      resolve(result ?? failed('unknown_completion'))
    );
    tx.addEventListener('abort', () =>
      resolve(
        result && !result.ok && result.reason !== 'unknown_completion'
          ? result
          : failed('aborted')
      )
    );
    function done() {
      pending--;
      if (pending !== 0) return;
      if (!scope.current()) {
        refuse('stopped');
        return;
      }
      try {
        const value = finish({ received, pairs }, tx, refuse);
        if (value) result = value;
      } catch {
        refuse('aborted');
      }
    }
    for (const store of ['received_envelopes', 'conversations'] as const) {
      try {
        const request = tx
          .objectStore(store)
          .index('by_owner')
          .openCursor(IDBKeyRange.only(scope.owner));
        request.addEventListener('success', () => {
          try {
            const cursor = request.result;
            if (!cursor) {
              done();
              return;
            }
            const key = cursor.primaryKey;
            if (
              !Array.isArray(key) ||
              key.length !== 2 ||
              key[0] !== scope.owner
            ) {
              refuse('corrupt_record');
              return;
            }
            if (store === 'received_envelopes') {
              const row = cipherRow(cursor.value, scope.owner, key[1]);
              if (!row) {
                refuse('corrupt_record');
                return;
              }
              received = received.concat(row);
              receivedBytes += row.bytes;
              if (
                received.length > LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes ||
                receivedBytes >
                  LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes
              ) {
                refuse('capacity');
                return;
              }
            } else {
              const row = pairRow(cursor.value, scope.owner, key[1]);
              if (!row || pairs.some((x) => x.peer === row.peer)) {
                refuse('corrupt_record');
                return;
              }
              pairs = pairs.concat(row);
              pairBytes += row.bytes;
              if (
                pairs.length > LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes ||
                pairBytes > LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes
              ) {
                refuse('capacity');
                return;
              }
            }
            cursor.continue();
          } catch {
            refuse('aborted');
          }
        });
      } catch {
        refuse('aborted');
      }
    }
  });
}
export function captureMessageMetadata(
  database: BrowserDatabase,
  session: PrivateSession,
  unlocked: UnlockedSession,
  review: unknown
): MessageMetadata | undefined {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_local_message_metadata'
  )
    return;
  const observation = privateSessionOwnership(session),
    cacheObservation = unlockedSessionOwnership(unlocked);
  if (
    !observation ||
    !cacheObservation ||
    observation.owner !== cacheObservation.owner ||
    observation.session !== cacheObservation.session ||
    !observation.current() ||
    !cacheObservation.current()
  )
    return;
  const observed = observation,
    cache = cacheObservation;
  const owner = observed.owner,
    generation = observed.session;
  try {
    if (browserDatabaseState(database).state !== 'ready') return;
  } catch {
    return;
  }
  let closed = false,
    off = () => {};
  const token = Object.freeze({}) as MessageMetadata;
  function close() {
    if (closed) return;
    closed = true;
    off();
    controllers.set(token, {
      snapshot: () => Promise.resolve<Snapshot>({ status: 'stopped' }),
      display: () => Promise.resolve<Display>({ status: 'stopped' }),
      messages: () => ({ status: 'closed', messages: [] }),
      close: () => {}
    });
  }
  const scope: Scope = {
    database,
    owner,
    session: generation,
    unlocked,
    current: () => {
      if (closed || !observed.current() || !cache.current()) {
        close();
        return false;
      }
      try {
        return browserDatabaseState(database).state === 'ready';
      } catch {
        return false;
      }
    }
  };
  async function snapshot(): Promise<Snapshot> {
    if (!scope.current()) return { status: 'stopped' };
    const loaded = await transaction(scope, 'readonly', (rows) => ({
      ok: true,
      value: {
        status: 'ready' as const,
        owner,
        readRumors: Array.from(
          new Set(
            rows.received.flatMap((x) =>
              x.record.read ? [x.record.read.rumorHash] : []
            )
          )
        ),
        pairs: rows.pairs.map((x) => ({ id: x.id, peer: x.peer }))
      }
    }));
    return !scope.current()
      ? { status: 'stopped' }
      : loaded.ok
        ? loaded.value
        : { status: loaded.reason };
  }
  async function display(
    room: AdmittedConversation,
    review: unknown
  ): Promise<Display> {
    if (!scope.current()) return { status: 'stopped' };
    if (
      review !== 'displayed_message' ||
      !unlockedConversationCurrent(unlocked, room)
    )
      return { status: 'invalid' };
    const data = conversationSnapshot(room),
      proof = conversationOwnership(room),
      cipher = conversationEnvelopeIdentity(room);
    if (
      !data ||
      !proof ||
      !cipher ||
      data.owner !== owner ||
      proof.owner !== owner ||
      proof.session !== generation ||
      cipher.owner !== owner
    )
      return { status: 'invalid' };
    const current = () =>
      scope.current() &&
      proof.current() &&
      unlockedConversationCurrent(unlocked, room);
    if (!current() || !navigator.locks?.request)
      return { status: 'unavailable' };
    try {
      return await navigator.locks.request(
        'harvestcircle:owner:' + owner,
        { mode: 'exclusive', ifAvailable: true },
        async (lock): Promise<Display> => {
          if (!lock) return { status: 'busy' };
          if (!current()) return { status: 'stopped' };
          const loaded = await transaction(scope, 'readonly', (rows) => ({
            ok: true,
            value: rows
          }));
          if (!current()) return { status: 'stopped' };
          if (!loaded.ok) return { status: loaded.reason };
          const row = loaded.value.received.find(
            (x) => x.record.id === cipher.outerId
          );
          if (!row || row.record.outer !== cipher.outerWire)
            return { status: 'conflict' };
          if (row.record.read && row.record.read.rumorHash !== data.rumorId)
            return { status: 'conflict' };
          let pair = loaded.value.pairs.find((x) => x.peer === data.peer);
          const existed = !!row.record.read && !!pair;
          if (!pair) {
            const id = newLocalId();
            if (!id) return { status: 'unavailable' };
            const wire = JSON.stringify({
              schema: 1,
              family: 'conversation_handle',
              owner,
              id,
              peer: data.peer
            });
            pair = pairRow({ owner, id, wire }, owner, id);
            if (!pair) return { status: 'invalid' };
          }
          const selected = pair;
          let nextWire = row.wire;
          if (!row.record.read) {
            const now = Date.now();
            if (
              safeUnsignedInteger(now) === undefined ||
              row.record.revision >= Number.MAX_SAFE_INTEGER
            )
              return { status: 'invalid' };
            nextWire = JSON.stringify({
              ...row.record,
              revision: row.record.revision + 1,
              read: { rumorHash: data.rumorId, atMilliseconds: now }
            });
            if (
              !cipherRow(
                { owner, id: row.record.id, wire: nextWire },
                owner,
                row.record.id
              )
            )
              return { status: 'invalid' };
          }
          const written = await transaction(
            scope,
            'readwrite',
            (rows, tx, refuse) => {
              if (!current()) {
                refuse('stopped');
                return;
              }
              const latest = rows.received.find(
                  (x) => x.record.id === cipher.outerId
                ),
                existing = rows.pairs.find((x) => x.peer === data.peer);
              if (
                latest?.wire !== row.wire ||
                (existing?.wire ?? null) !==
                  (loaded.value.pairs.find((x) => x.peer === data.peer)?.wire ??
                    null) ||
                rows.pairs.some(
                  (x) => x.id === selected.id && x.wire !== selected.wire
                )
              ) {
                refuse('conflict');
                return;
              }
              const nextBytes = new TextEncoder().encode(
                JSON.stringify({ owner, id: cipher.outerId, wire: nextWire })
              ).length;
              if (
                rows.received.reduce((n, x) => n + x.bytes, 0) -
                  row.bytes +
                  nextBytes >
                  LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes ||
                (!existing &&
                  (rows.pairs.length >=
                    LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes ||
                    rows.pairs.reduce((n, x) => n + x.bytes, selected.bytes) >
                      LOCAL_PERSISTENCE_BUDGETS.receivedCiphertextBytes))
              ) {
                refuse('capacity');
                return;
              }
              if (nextWire !== row.wire)
                tx.objectStore('received_envelopes').put({
                  owner,
                  id: cipher.outerId,
                  wire: nextWire
                });
              if (!existing)
                tx.objectStore('conversations').put({
                  owner,
                  id: selected.id,
                  wire: selected.wire
                });
              return { ok: true, value: true };
            }
          );
          if (!current()) return { status: 'stopped' };
          if (!written.ok) return { status: written.reason };
          const readback = await transaction(scope, 'readonly', (rows) => ({
            ok: true,
            value: rows
          }));
          if (!current()) return { status: 'stopped' };
          if (!readback.ok)
            return {
              status: 'unknown_completion',
              phase: 'readback',
              reason: readback.reason
            };
          if (
            readback.value.received.find((x) => x.record.id === cipher.outerId)
              ?.wire !== nextWire ||
            readback.value.pairs.find((x) => x.id === selected.id)?.wire !==
              selected.wire
          )
            return { status: 'conflict' };
          return {
            status: existed ? 'existing' : 'saved',
            conversationId: selected.id,
            rumorId: data.rumorId
          };
        }
      );
    } catch {
      return { status: current() ? 'unknown_completion' : 'stopped' };
    }
  }
  controllers.set(token, {
    snapshot,
    display,
    messages: () =>
      scope.current()
        ? readUnlockedMessages(unlocked)
        : { status: 'closed', messages: [] },
    close
  });
  off = subscribePrivateSessionClose(session, close);
  return scope.current() ? token : undefined;
}
export function messageMetadataSnapshot(
  scope: MessageMetadata
): Promise<Snapshot> {
  return (
    controllers.get(scope)?.snapshot() ?? Promise.resolve({ status: 'invalid' })
  );
}
// Detached projection for local count calculation only, never persistence proof.
export function messageMetadataMessages(scope: MessageMetadata) {
  return (
    controllers.get(scope)?.messages() ?? {
      status: 'rejected' as const,
      messages: []
    }
  );
}
export function markDisplayedMessage(
  scope: MessageMetadata,
  room: AdmittedConversation,
  review: unknown
): Promise<Display> {
  return (
    controllers.get(scope)?.display(room, review) ??
    Promise.resolve({ status: 'invalid' })
  );
}
