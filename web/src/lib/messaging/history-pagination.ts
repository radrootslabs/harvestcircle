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
import { readRetainedOuterMetadata } from '../persistence/history-metadata-repository.ts';
import {
  createPrivateHistoryRun,
  subscribePrivateSourcePage,
  type PrivatePageMessage
} from '../nostr/private-pool.ts';
import {
  admitInboxEnvelope,
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
  let stopped = false,
    busy = false,
    cleanupRequired = false,
    state = 'ready',
    reason: string | null = null;
  let cursors = Array.from<OuterCursor>([]),
    release = () => {},
    off = () => {};
  const current = () => !stopped && original.current();
  function stop() {
    stopped = true;
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
    cleanupRequired = failure;
    if (!failure) syncs.delete(sync);
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
      let ids = Array.from<string>([]),
        acceptedDeliveries = 0;
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
              deliveries: message.deliveries ?? ids.length,
              ids: ids.slice()
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
              admitted
            );
            if (!current()) return;
            if (
              retained.status !== 'retained' &&
              retained.status !== 'duplicate'
            ) {
              failure = retained.status;
              return;
            }
            acceptedDeliveries++;
            const parsed = JSON.parse(message.wire) as { id: string };
            if (!ids.includes(parsed.id)) ids = ids.concat(parsed.id);
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
    snapshot: () => ({
      ownerGenerationScoped: true,
      state,
      reason,
      historyComplete: false,
      cleanupRequired,
      sources: cursors.map((cursor) => ({
        ...cursor,
        seen: cursor.seen.slice()
      }))
    }),
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
        cursors = original.targets.map((source) => {
          const fresh = initialOuterCursor(
            source,
            initial.rows.filter((row) => row.source === source)
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
          const rows = stored.rows.filter(
            (row) =>
              row.source === cursor.source && result.ids.includes(row.outerId)
          );
          next = next.concat(
            advanceOuterPage(cursor, rows, result.deliveries, result.reason)
          );
          if (result.reason === 'budget' || result.reason === 'elapsed') {
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
        return true;
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
