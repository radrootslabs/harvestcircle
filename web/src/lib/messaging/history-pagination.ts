import {
  inboxSyncHistoryOwnership,
  type InboxSync,
  type InboxHistoryOwnership
} from './inbox-sync.ts';
import {
  initialOuterCursor,
  advanceOuterPage,
  type OuterCursor
} from './inbox-state.ts';
import {
  readRetainedOuterMetadata,
  type RetainedOuterMetadata
} from '../persistence/history-metadata-repository.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import {
  createPrivateHistoryRun,
  subscribePrivateSourcePage,
  type PrivatePageMessage
} from '../nostr/private-pool.ts';
import {
  admitInboxEnvelope,
  inboxEnvelopeOuterMetadata,
  retainInboxEnvelope
} from '../persistence/inbox-envelope-repository.ts';
import {
  subscribePrivateSessionClose,
  privateSessionOwnership
} from '../runtime/private-session.ts';
declare const olderBrand: unique symbol;
export type OlderInbox = Readonly<{ [olderBrand]: true }>;
type View = Readonly<{
  ownerGenerationScoped: true;
  state: string;
  reason: string | null;
  historyComplete: false;
  cleanupRequired: boolean;
  sources: readonly OuterCursor[];
}>;
type Controller = {
  start(review: unknown): Promise<boolean>;
  stop(): void;
  snapshot(): View;
};
const controllers = new WeakMap<OlderInbox, Controller>(),
  syncs = new WeakMap<InboxSync, OlderInbox>();
export function captureOlderInbox(
  sync: InboxSync,
  review: unknown
): OlderInbox | undefined {
  if (review !== 'reviewed_load_older' || syncs.has(sync)) return undefined;
  const ownership = inboxSyncHistoryOwnership(sync);
  if (!ownership?.current()) return undefined;
  const original: InboxHistoryOwnership = ownership;
  const token = Object.freeze({}) as OlderInbox;
  const cancellation = new AbortController();
  let stopped = false,
    busy = false,
    cleanupRequired = false,
    state = 'ready',
    reason: string | null = null;
  let cursors = Array.from<OuterCursor>([]),
    release = () => {},
    off = () => {};
  // Genuine excluded deliveries still have signed outer cursor facts. They
  // are bounded observed-page memory, never retained rows or exclusion proof.
  const excludedOuter = new Map<string, RetainedOuterMetadata>();
  let retainedOuter: readonly RetainedOuterMetadata[] =
    Array.from<RetainedOuterMetadata>([]);
  function mergedOuter(
    retained: readonly RetainedOuterMetadata[],
    extra?: RetainedOuterMetadata
  ): RetainedOuterMetadata[] | undefined {
    const rows = new Map<string, RetainedOuterMetadata>(),
      ids = new Map<string, true>();
    for (const row of [
      ...retained,
      ...excludedOuter.values(),
      ...(extra ? [extra] : [])
    ]) {
      ids.set(row.outerId, true);
      if (ids.size > LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes)
        return undefined;
      const key = JSON.stringify([row.source, row.outerId]),
        previous = rows.get(key);
      if (previous && previous.outerTime !== row.outerTime) return undefined;
      if (!previous) rows.set(key, row);
    }
    return Array.from(rows.values());
  }
  const current = () => {
    const valid = !stopped && original.current();
    if (!valid) {
      // These are only transient cursor facts. Observed generation loss must
      // discard them even when the next explicit action is refused at entry.
      excludedOuter.clear();
      retainedOuter = [];
      cursors = [];
    }
    return valid;
  };
  function stop() {
    stopped = true;
    cancellation.abort();
    state = 'stopped';
    let failure = false;
    try {
      off();
    } catch {
      failure = true;
    }
    try {
      release();
    } catch {
      failure = true;
    }
    cursors = [];
    excludedOuter.clear();
    retainedOuter = [];
    cleanupRequired = failure;
    if (!failure && syncs.get(sync) === token) syncs.delete(sync);
    if (failure) throw Error('private_history_cleanup_required');
  }
  async function page(
    source: string,
    until: number | undefined,
    run: ReturnType<typeof createPrivateHistoryRun>
  ): Promise<{ reason: string; deliveries: number; ids: readonly string[] }> {
    return await new Promise((resolve) => {
      let queue = Promise.resolve(),
        finished = false,
        failure: string | undefined;
      const ids = new Map<string, true>();
      const pageRetained = new Map<string, RetainedOuterMetadata>();
      let acceptedDeliveries = 0;
      function receive(message: PrivatePageMessage) {
        if (finished) return;
        if (message.type === 'end') {
          finished = true;
          void queue.then(() =>
            resolve({
              reason:
                failure ??
                (message.reason === 'complete' &&
                acceptedDeliveries < (message.deliveries ?? acceptedDeliveries)
                  ? 'unsupported'
                  : message.reason),
              deliveries: message.deliveries ?? ids.size,
              ids: Array.from(ids.keys())
            })
          );
          return;
        }
        if (!current()) {
          stop();
          return;
        }
        const origin = message.from.endsWith('/')
          ? message.from.slice(0, -1)
          : message.from;
        // Obtain original admitted owner's namespace from the genuine sync session,
        // never the random outer author. The actual repository enforces it too.
        queue = queue
          .then(async () => {
            if (!current()) return;
            if (failure === 'capacity') return;
            const owner = privateOwner();
            const admitted =
              owner &&
              admitInboxEnvelope(message.wire, owner, origin, Date.now());
            if (!admitted || origin !== source) {
              failure = 'unsupported';
              return;
            }
            const retained = await retainInboxEnvelope(
              original.repository,
              original.session,
              admitted,
              cancellation.signal
            );
            if (!current()) return;
            if (retained.status === 'probe_excluded') {
              const signed = inboxEnvelopeOuterMetadata(admitted);
              if (!signed) {
                failure = 'unsupported';
                return;
              }
              const fact = {
                source,
                outerId: signed.id,
                outerTime: signed.createdAt
              };
              if (
                !mergedOuter([...retainedOuter, ...pageRetained.values()], fact)
              ) {
                failure = 'capacity';
                return;
              }
              const key = JSON.stringify([source, signed.id]);
              if (!excludedOuter.has(key)) excludedOuter.set(key, fact);
              acceptedDeliveries++;
              ids.set(signed.id, true);
              return;
            }
            if (
              retained.status !== 'retained' &&
              retained.status !== 'duplicate'
            ) {
              failure = retained.status;
              return;
            }
            const signed = inboxEnvelopeOuterMetadata(admitted);
            if (!signed) {
              failure = 'unsupported';
              return;
            }
            const fact = {
              source,
              outerId: signed.id,
              outerTime: signed.createdAt
            };
            if (
              !mergedOuter([...retainedOuter, ...pageRetained.values()], fact)
            ) {
              failure = 'capacity';
              return;
            }
            pageRetained.set(signed.id, fact);
            acceptedDeliveries++;
            ids.set(signed.id, true);
          })
          .catch(() => {
            failure = 'unknown_completion';
          });
      }
      try {
        release = subscribePrivateSourcePage(
          original.pool,
          run,
          source,
          until,
          receive
        );
      } catch {
        resolve({ reason: 'error', deliveries: 0, ids: [] });
      }
    });
  }
  function privateOwner() {
    return privateSessionOwnership(original.session)?.owner;
  }
  controllers.set(token, {
    snapshot: () => {
      current();
      return {
        ownerGenerationScoped: true,
        state,
        reason,
        historyComplete: false,
        cleanupRequired,
        sources: cursors.map((cursor) => ({
          ...cursor,
          seen: cursor.seen.slice()
        }))
      };
    },
    stop,
    async start(reviewed) {
      if (
        reviewed !== 'reviewed_load_older' ||
        busy ||
        !current() ||
        cleanupRequired
      )
        return false;
      busy = true;
      state = 'loading';
      reason = null;
      try {
        const initial = await readRetainedOuterMetadata(
          original.repository,
          original.session
        );
        if (!current()) {
          stop();
          return false;
        }
        if (!initial.ok) {
          state = 'needs_action';
          reason = initial.reason;
          return false;
        }
        const observed = mergedOuter(initial.rows);
        if (!observed) {
          state = 'needs_action';
          reason = 'capacity';
          return false;
        }
        retainedOuter = initial.rows;
        cursors = original.targets.map((source) => {
          const fresh = initialOuterCursor(
            source,
            observed.filter((row) => row.source === source)
          );
          const previous = cursors.find((cursor) => cursor.source === source);
          return previous && previous.until === fresh.until
            ? { ...fresh, state: previous.state, partial: previous.partial }
            : fresh;
        });
        const run = createPrivateHistoryRun(original.pool);
        let next = Array.from<OuterCursor>([]);
        for (const cursor of cursors) {
          if (!current()) {
            stop();
            break;
          }
          if (cursor.state === 'saturated') {
            next = next.concat(cursor);
            continue;
          }
          const result = await page(cursor.source, cursor.until, run);
          if (!current()) {
            stop();
            break;
          }
          const stored = await readRetainedOuterMetadata(
            original.repository,
            original.session
          );
          if (!current()) {
            stop();
            break;
          }
          if (!stored.ok) {
            state = 'needs_action';
            reason = stored.reason;
            next = next.concat(cursor);
            break;
          }
          const observed = mergedOuter(stored.rows);
          if (!observed) {
            state = 'needs_action';
            reason = 'capacity';
            next = next.concat(cursor);
            break;
          }
          retainedOuter = stored.rows;
          const rows = observed.filter(
            (row) =>
              row.source === cursor.source && result.ids.includes(row.outerId)
          );
          next = next.concat(
            advanceOuterPage(cursor, rows, result.deliveries, result.reason)
          );
          if (
            result.reason === 'budget' ||
            result.reason === 'elapsed' ||
            result.reason === 'capacity'
          ) {
            state = 'needs_action';
            reason = result.reason;
            break;
          }
        }
        if (!stopped) {
          cursors = next.concat(
            cursors.filter(
              (cursor) => !next.some((row) => row.source === cursor.source)
            )
          );
          if (state !== 'needs_action')
            state = cursors.some((cursor) => cursor.partial)
              ? 'partial'
              : 'older_available';
        }
        return current();
      } catch {
        if (!stopped) {
          state = 'needs_action';
          reason = 'unknown_completion';
        }
        return false;
      } finally {
        busy = false;
      }
    }
  });
  off = subscribePrivateSessionClose(original.session, stop);
  syncs.set(sync, token);
  return token;
}
export function loadOlderInbox(
  token: OlderInbox,
  review: unknown
): Promise<boolean> {
  return controllers.get(token)?.start(review) ?? Promise.resolve(false);
}
export function stopOlderInbox(token: OlderInbox): void {
  controllers.get(token)?.stop();
}
export function olderInboxSnapshot(token: OlderInbox): View | undefined {
  return controllers.get(token)?.snapshot();
}
