import { planDecryptBatch } from './inbox-state.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  identityMessagingOwnership,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  getPrivateVisibilityScope,
  privateVisibilitySnapshot
} from '../runtime/dispose.ts';
import {
  listPrivateReceivedRecords,
  type PrivateStorageRepository
} from '../persistence/private-storage.ts';
import { privateRecordIdentity } from '../persistence/private-records.ts';
import {
  captureReceivedUnwrap,
  unwrapReceivedEnvelope,
  stopReceivedUnwrap,
  expireReceivedUnwrapWait,
  type ReceivedUnwrap
} from '../nostr/unwrap-admission.ts';
import {
  admitReceivedConversation,
  conversationSnapshot,
  type AdmittedConversation
} from './admit-conversation.ts';
import {
  unlockedSessionOwnership,
  subscribeUnlockedSessionClose,
  acceptUnlockedConversation,
  type UnlockedSession
} from './unlocked-session.ts';
declare const queueBrand: unique symbol;
export type DecryptionQueue = Readonly<{ [queueBrand]: true }>;
type View = Readonly<{
  state: string;
  reason: string | null;
  queued: number;
  attempted: number;
  blocked: number;
  admitted: number;
  invalid: number;
  busy: boolean;
}>;
type Controller = {
  refresh(): Promise<boolean>;
  run(review: unknown, blocked: readonly string[]): Promise<boolean>;
  snapshot(): View;
  rooms(): readonly AdmittedConversation[];
  stop(): void;
  expire(): void;
};
const controllers = new WeakMap<DecryptionQueue, Controller>(),
  sessions = new WeakMap<PrivateSession, DecryptionQueue>();
// Only original current owner-generation and actual explicit Messages unlock
// admit a controller. Construction/refresh never invoke an extension operation.
export function captureDecryptionQueue(
  repository: PrivateStorageRepository,
  session: PrivateSession,
  identity: IdentitySession,
  unlocked: UnlockedSession,
  review: unknown
): DecryptionQueue | undefined {
  if (typeof window === 'undefined' || review !== 'reviewed_inbox_unlock')
    return undefined;
  const observed = privateSessionOwnership(session),
    owner = identityMessagingOwnership(identity),
    cache = unlockedSessionOwnership(unlocked);
  if (
    !observed?.current() ||
    !owner?.current() ||
    !cache?.current() ||
    observed.owner !== owner.owner ||
    observed.owner !== cache.owner ||
    observed.session !== owner.session ||
    observed.session !== cache.session
  )
    return undefined;
  const capture = observed,
    identityOwner = owner,
    unlockedOwner = cache;
  let visibility;
  try {
    visibility = getPrivateVisibilityScope(session);
  } catch {
    return undefined;
  }
  if (!visibility || !privateVisibilitySnapshot(visibility).active)
    return undefined;
  const visible = visibility;
  const prior = sessions.get(session);
  if (prior) return prior;
  const token = Object.freeze({}) as DecryptionQueue;
  let stopped = false,
    busy = false,
    paused = false,
    state = 'ready',
    reason: string | null = null,
    attempted = 0,
    blocked = 0,
    admitted = 0,
    invalid = 0;
  let pending = Array.from<string>([]),
    processed = Array.from<string>([]),
    accepted = Array.from<AdmittedConversation>([]),
    readers = Array.from<ReceivedUnwrap>([]);
  let active: ReceivedUnwrap | undefined,
    off = () => {},
    unlockOff = () => {};
  function current() {
    return (
      !stopped &&
      capture.current() &&
      identityOwner.current() &&
      unlockedOwner.current() &&
      privateVisibilitySnapshot(visible).active
    );
  }
  function stop() {
    if (stopped) return;
    stopped = true;
    state = 'stopped';
    reason = 'stopped';
    if (active) stopReceivedUnwrap(active);
    for (const reader of readers) stopReceivedUnwrap(reader);
    readers = [];
    accepted = [];
    pending = [];
    processed = [];
    off();
    unlockOff();
    if (sessions.get(session) === token) sessions.delete(session);
    // Actual pending SDK promise retains the shared slot until its settlement.
  }
  function expire() {
    if (stopped || !busy) return;
    paused = true;
    state = 'paused';
    reason = 'wait_expired';
    if (active) expireReceivedUnwrapWait(active);
  }
  function pause(why: string) {
    paused = true;
    state = 'paused';
    reason = why;
  }
  async function load() {
    if (!current()) {
      stop();
      return false;
    }
    const result = await listPrivateReceivedRecords(repository);
    if (!current()) {
      stop();
      return false;
    }
    if (!result.ok) {
      pause(result.reason);
      return false;
    }
    let ids = Array.from<string>([]);
    for (const handle of result.value) {
      const record = privateRecordIdentity(handle);
      if (record?.owner !== capture.owner) {
        pause('invalid_scope');
        return false;
      }
      if (!processed.includes(record.id)) ids = ids.concat(record.id);
    }
    pending = ids;
    return true;
  }
  const controller: Controller = {
    snapshot: () => ({
      state,
      reason,
      queued: pending.length,
      attempted,
      blocked,
      admitted,
      invalid,
      busy
    }),
    rooms: () => (current() ? accepted.slice() : []),
    stop,
    expire,
    async refresh() {
      if (busy || !current()) return false;
      try {
        return await load();
      } catch {
        pause('unavailable');
        return false;
      }
    },
    async run(reviewed, blockedPeers) {
      if (reviewed !== 'reviewed_decrypt_batch' || busy || !current())
        return false;
      // Detached block values can only suppress an already authenticated inner
      // peer's local projection; they never bypass envelope cost or confer SDK access.
      if (
        !Array.isArray(blockedPeers) ||
        blockedPeers.length > LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes
      )
        return false;
      const exclusions = blockedPeers.slice();
      for (const peer of exclusions)
        if (canonicalPublicKey(peer) !== peer) return false;
      busy = true;
      paused = false;
      state = 'running';
      reason = null;
      attempted = 0;
      blocked = 0;
      admitted = 0;
      invalid = 0;
      try {
        if (!(await load()) || paused || !current()) return false;
        const plan = planDecryptBatch(pending);
        for (const id of plan.selected) {
          if (!current()) {
            stop();
            return false;
          }
          if (paused) return false;
          // Cost reservation occurs before unwrap/admission/dedup/blocked sender.
          attempted++;
          active = captureReceivedUnwrap(
            repository,
            identity,
            id,
            'reviewed_inbox_unlock'
          );
          if (!active) {
            pause('unavailable');
            return false;
          }
          const reader = active,
            result = await unwrapReceivedEnvelope(reader);
          if (!current()) {
            stopReceivedUnwrap(reader);
            stop();
            return false;
          }
          if (paused) {
            stopReceivedUnwrap(reader);
            return false;
          }
          if (result.status === 'authenticated') {
            const room =
              admitReceivedConversation(result.envelope, 'inbound') ??
              admitReceivedConversation(result.envelope, 'self_archive');
            const message = room && conversationSnapshot(room);
            if (!room || !message) {
              invalid++;
              stopReceivedUnwrap(reader);
            } else if (exclusions.includes(message.peer)) {
              blocked++;
              stopReceivedUnwrap(reader);
            } else {
              const projection = acceptUnlockedConversation(unlocked, room);
              if (projection === 'added') {
                accepted = accepted.concat(room);
                readers = readers.concat(reader);
                admitted++;
              } else if (projection === 'duplicate') {
                admitted++;
                stopReceivedUnwrap(reader);
              } else {
                stopReceivedUnwrap(reader);
                pause(projection);
                return false;
              }
            }
          } else if (
            result.status === 'mismatch' ||
            result.status === 'invalid'
          ) {
            invalid++;
            stopReceivedUnwrap(reader);
          } else {
            stopReceivedUnwrap(reader);
            pause(result.status);
            return false;
          }
          active = undefined;
          if (!current() || paused) return false;
          processed = processed.concat(id);
          pending = pending.filter((next) => next !== id);
        }
        if (!(await load()) || paused || !current()) return false;
        state = pending.length ? 'needs_action' : 'idle';
        return true;
      } catch {
        if (current() && !paused) pause('unavailable');
        return false;
      } finally {
        active = undefined;
        busy = false;
      }
    }
  };
  controllers.set(token, controller);
  sessions.set(session, token);
  off = subscribePrivateSessionClose(session, stop);
  unlockOff = subscribeUnlockedSessionClose(unlocked, stop);
  if (!current()) {
    stop();
    return undefined;
  }
  return token;
}
export function refreshDecryptionQueue(
  token: DecryptionQueue
): Promise<boolean> {
  return controllers.get(token)?.refresh() ?? Promise.resolve(false);
}
export function runDecryptionBatch(
  token: DecryptionQueue,
  review: unknown,
  blockedPeers: readonly string[] = []
): Promise<boolean> {
  return (
    controllers.get(token)?.run(review, blockedPeers) ?? Promise.resolve(false)
  );
}
export function decryptionQueueSnapshot(token: DecryptionQueue): View {
  return (
    controllers.get(token)?.snapshot() ?? {
      state: 'stopped',
      reason: 'invalid',
      queued: 0,
      attempted: 0,
      blocked: 0,
      admitted: 0,
      invalid: 0,
      busy: false
    }
  );
}
export function decryptionQueueConversations(
  token: DecryptionQueue
): readonly AdmittedConversation[] {
  return controllers.get(token)?.rooms() ?? [];
}
export function stopDecryptionQueue(token: DecryptionQueue): void {
  controllers.get(token)?.stop();
}
export function expireDecryptionQueueWait(token: DecryptionQueue): void {
  controllers.get(token)?.expire();
}
