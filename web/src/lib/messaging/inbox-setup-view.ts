import {
  readRelayPolicy,
  inboxRelayTargets,
  type RelayPolicy
} from '../config/relays.ts';
import { deploymentRelayPolicy } from '../config/deployment-relays.ts';
import {
  identityMessagingOwnership,
  subscribeIdentityInvalidation,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  createPrivateSession,
  closePrivateSession,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  identityViewSession,
  type IdentityViewContext
} from '../runtime/view-context.ts';
import {
  createPublicView,
  beginPublicViewRun,
  disposePublicView,
  type PublicRuntime
} from '../runtime/public-runtime.ts';
import {
  resolveInboxPreference,
  inboxResolutionSnapshot,
  closeInboxResolver,
  type InboxResolver
} from './resolve-inbox.ts';
import {
  inboxPreferenceSnapshot,
  inboxPreferenceWire
} from '../nostr/inbox-preferences.ts';
import {
  reviewInboxSetup,
  inboxSetupPreview,
  type InboxSetupReview
} from './inbox-setup.ts';
import {
  beginInboxPreferenceOperation,
  runInboxPreferenceOperation,
  stopInboxPreferenceOperation,
  type InboxPreferenceAction
} from './inbox-setup-operation.ts';
import {
  openBrowserDatabase,
  closeBrowserDatabase,
  type BrowserDatabase
} from '../persistence/database.ts';
import {
  createPublicQuotaRepository,
  loadPreferenceOperation,
  type PublicQuotaRepository
} from '../persistence/quota.ts';
import {
  publicRecordSnapshot,
  type PublicRecordHandle
} from '../persistence/records.ts';
import {
  resolveInboxReadback,
  inboxReadbackSnapshot,
  closeInboxReadback,
  type InboxReadback
} from '../nostr/inbox-readback.ts';
import {
  existingInboxSetupSnapshot,
  verifyInboxSetupOperation
} from './inbox-setup-verification.ts';
import type { ObserveInboxAccess } from './readiness.ts';
declare const viewBrand: unique symbol;
export type InboxSetupView = Readonly<{ [viewBrand]: true }>;
export type InboxSetupViewSnapshot = Readonly<{
  status: string;
  busy: boolean;
  owner?: string;
  currentWire: string | null;
  existingInboxes: string[];
  availableInboxes: string[];
  preview?: NonNullable<ReturnType<typeof inboxSetupPreview>>;
  accepted: string[];
  readback: string[];
  setupComplete: boolean;
}>;
// Internal trusted orchestration ports require real opaque discovery/readback
// owners. Neither displayed facts nor caller booleans admit a preference effect.
type Inputs = Readonly<{
  identity: IdentitySession;
  policy: RelayPolicy;
  lookup(): Promise<InboxResolver>;
  readback?(
    record: PublicRecordHandle,
    owner: string,
    id: string
  ): InboxReadback | undefined;
  observeAccess?: ObserveInboxAccess;
  now?(): number;
  close?(): void;
}>;
type Controller = {
  ownership():
    | Readonly<{
        identity: IdentitySession;
        policy: RelayPolicy;
        session: PrivateSession | undefined;
        resolver: InboxResolver | undefined;
        observeAccess: ObserveInboxAccess | undefined;
        current(): boolean;
      }>
    | undefined;
  observe(): void;
  snapshot(): InboxSetupViewSnapshot;
  subscribe(listener: (state: InboxSetupViewSnapshot) => void): () => void;
  check(): Promise<void>;
  review(
    selected: string[],
    removeTags: number[],
    removeFields: string[]
  ): Promise<void>;
  enable(): Promise<void>;
  unlock(): Promise<void>;
  readback(): Promise<void>;
  stop(): void;
  close(): void;
};
const views = new WeakMap<InboxSetupView, Controller>();
const unavailable = (): InboxSetupViewSnapshot => ({
  status: 'unavailable',
  busy: false,
  currentWire: null,
  existingInboxes: [],
  availableInboxes: [],
  accepted: [],
  readback: [],
  setupComplete: false
});
export function createInboxSetupView(
  input: Inputs
): InboxSetupView | undefined {
  if (typeof window === 'undefined') return undefined;
  const initial = identityMessagingOwnership(input.identity);
  if (!initial || !initial.current()) return undefined;
  const capture = initial;
  const manifest = readRelayPolicy(input.policy);
  const available = manifest.inbox
    .filter(
      ({ read, write, origin }) =>
        read && write && !manifest.operatorDenylist.includes(origin)
    )
    .map((row) => row.origin);
  const token = Object.freeze({}) as InboxSetupView;
  let state = {
      ...unavailable(),
      status: 'not_checked',
      owner: capture.owner,
      availableInboxes: available
    },
    epoch = 0,
    closed = false;
  let pendingClose: (() => void)[] | undefined;
  let resolver: InboxResolver | undefined,
    review: InboxSetupReview | undefined,
    action: InboxPreferenceAction | undefined,
    database: BrowserDatabase | undefined,
    repository: PublicQuotaRepository | undefined,
    session: PrivateSession | undefined,
    reader: InboxReadback | undefined,
    command: string | undefined;
  const listeners = new Map<symbol, (state: InboxSetupViewSnapshot) => void>();
  const current = (generation = epoch) =>
    !closed && generation === epoch && capture.current();
  const snapshot = () => {
    if (closed) return { ...unavailable(), status: 'closed' };
    if (!capture.current()) return unavailable();
    return JSON.parse(JSON.stringify(state)) as InboxSetupViewSnapshot;
  };
  function notify() {
    for (const listener of listeners.values()) {
      try {
        listener(snapshot());
      } catch {
        /* Isolate presentation observers. */
      }
    }
  }
  function update(status: string, busy = state.busy) {
    state = { ...state, status, busy };
    notify();
  }
  function inspect() {
    if (!resolver || !current()) return;
    const observed = inboxResolutionSnapshot(resolver);
    if (!current() || observed.author !== capture.owner) return;
    const head = observed.head && inboxPreferenceSnapshot(observed.head);
    state = {
      ...state,
      currentWire: observed.head
        ? (inboxPreferenceWire(observed.head) ?? null)
        : null,
      existingInboxes: head?.relays.slice() ?? [],
      setupComplete: false
    };
    if (
      observed.coverage !== 'bounded-eose' ||
      observed.status === 'inconclusive'
    ) {
      update('lookup_incomplete');
      return;
    }
    const compatible =
      head?.status === 'supported' &&
      inboxRelayTargets(input.policy, head.relays, 'read').length > 0 &&
      inboxRelayTargets(input.policy, head.relays, 'write').length > 0;
    update(
      compatible
        ? 'compatible'
        : observed.status === 'missing'
          ? 'not_observed'
          : 'needs_review'
    );
  }
  async function work(
    status: string,
    perform: (generation: number) => Promise<void>
  ) {
    if (!current() || state.busy) return;
    const generation = epoch;
    update(status, true);
    if (!current(generation)) return;
    try {
      await perform(generation);
    } catch {
      if (current(generation)) update('unavailable');
    } finally {
      if (current(generation)) {
        state = { ...state, busy: false };
        notify();
      }
    }
  }
  function stop() {
    epoch++;
    if (action) stopInboxPreferenceOperation(action);
    review = undefined;
    state = { ...state, preview: undefined, setupComplete: false };
    if (reader) closeInboxReadback(reader);
    reader = undefined;
    if (resolver) closeInboxResolver(resolver);
    resolver = undefined;
    update('paused', false);
  }
  const invalidate = subscribeIdentityInvalidation(input.identity, () => {
    if (!capture.current()) stop();
  });
  views.set(token, {
    ownership: () =>
      current()
        ? {
            identity: input.identity,
            policy: input.policy,
            session,
            resolver,
            observeAccess: input.observeAccess,
            current: () => current()
          }
        : undefined,
    observe() {
      if (current() && !state.busy && state.status === 'lookup_incomplete')
        inspect();
    },
    snapshot,
    subscribe(listener) {
      const id = Symbol();
      listeners.set(id, listener);
      try {
        listener(snapshot());
      } catch {
        /* Isolate presentation observers. */
      }
      return () => {
        listeners.delete(id);
      };
    },
    check: () =>
      work('checking', async (generation) => {
        if (resolver) closeInboxResolver(resolver);
        const found = await input.lookup();
        if (!current(generation)) {
          closeInboxResolver(found);
          return;
        }
        resolver = found;
        review = undefined;
        state = { ...state, preview: undefined };
        inspect();
      }),
    review: (selected, removeTags, removeFields) =>
      work('reviewing', async (generation) => {
        if (!resolver) {
          update('lookup_incomplete');
          return;
        }
        inspect();
        if (!current(generation) || state.status === 'compatible') return;
        const result = await reviewInboxSetup(
          input.identity,
          resolver,
          input.policy,
          JSON.stringify({
            selectedInboxes: selected,
            removeTagIndices: removeTags,
            removeExtraFields: removeFields,
            createdAt: (input.now ?? (() => Math.floor(Date.now() / 1000)))()
          }),
          input.now
        );
        if (!current(generation)) return;
        if (result.status !== 'review') {
          update(result.reason);
          return;
        }
        review = result.review;
        state = { ...state, preview: inboxSetupPreview(review) };
        update('review');
      }),
    enable: () =>
      work('approval_wait', async (generation) => {
        if (
          !review ||
          state.preview === undefined ||
          !manifest.postingEnabled ||
          !manifest.messagingEnabled
        ) {
          update('unavailable');
          return;
        }
        const opened = await openBrowserDatabase();
        if (!current(generation)) {
          if (opened.state === 'ready') closeBrowserDatabase(opened.owner);
          return;
        }
        if (opened.state !== 'ready') {
          update('storage_unavailable');
          return;
        }
        if (database) closeBrowserDatabase(database);
        database = opened.owner;
        repository = createPublicQuotaRepository(database, capture.owner);
        if (!repository) {
          update('storage_unavailable');
          return;
        }
        command = crypto.randomUUID();
        const prepared = await beginInboxPreferenceOperation(
          repository,
          input.identity,
          input.policy,
          review,
          command,
          input.lookup,
          'reviewed_global_inbox_replacement'
        );
        if (!current(generation)) {
          if (prepared.status === 'prepared')
            stopInboxPreferenceOperation(prepared.action);
          return;
        }
        if (prepared.status !== 'prepared') {
          update(prepared.status);
          return;
        }
        action = prepared.action;
        const result = await runInboxPreferenceOperation(action);
        if (!current(generation)) return;
        const stored = await loadPreferenceOperation(repository, command);
        if (!current(generation)) return;
        const row =
          stored.ok &&
          publicRecordSnapshot(stored.value, capture.owner, command);
        state = {
          ...state,
          accepted:
            row && row.family === 'preference_operation'
              ? row.receipts
                  .filter(
                    (fact) =>
                      fact.status === 'accepted' &&
                      fact.eventId === row.artifact?.eventId
                  )
                  .map((fact) => fact.origin)
              : [],
          setupComplete: false
        };
        review = undefined;
        state = { ...state, preview: undefined };
        update(
          result.status === 'completed' ? 'awaiting_readback' : result.status
        );
      }),
    unlock: () =>
      work('approval_wait', async (generation) => {
        const acquired = await createPrivateSession(
          input.identity,
          'reviewed_private_session'
        );
        if (!current(generation)) {
          if (acquired) closePrivateSession(acquired);
          return;
        }
        session = acquired;
        if (!session || !resolver) {
          update('unavailable');
          return;
        }
        const result = existingInboxSetupSnapshot(
          input.identity,
          session,
          input.policy,
          resolver,
          input.observeAccess
        );
        if (!current(generation)) return;
        state = { ...state, setupComplete: result.setupComplete };
        update(result.status);
      }),
    readback: () =>
      work('checking_readback', async (generation) => {
        if (!repository || !command || !input.readback) {
          update('awaiting_readback');
          return;
        }
        if (!reader) {
          const stored = await loadPreferenceOperation(repository, command);
          if (!current(generation)) return;
          if (!stored.ok) {
            update('storage_unavailable');
            return;
          }
          reader = input.readback(stored.value, capture.owner, command);
        }
        const observation = reader && inboxReadbackSnapshot(reader);
        if (!observation || observation.status === 'pending') {
          update('awaiting_readback');
          return;
        }
        state = { ...state, readback: observation.sources.slice() };
        const result = await verifyInboxSetupOperation(
          repository,
          input.identity,
          session,
          input.policy,
          reader!,
          input.observeAccess
        );
        if (!current(generation)) return;
        state = { ...state, setupComplete: result.setupComplete };
        closeInboxReadback(reader!);
        reader = undefined;
        update(result.status);
      }),
    stop,
    close() {
      if (!pendingClose) {
        closed = true;
        const originalResolver = resolver,
          originalReader = reader,
          originalSession = session,
          originalDatabase = database;
        pendingClose = [
          stop,
          invalidate,
          () => {
            if (originalReader) closeInboxReadback(originalReader);
          },
          () => {
            if (originalResolver) closeInboxResolver(originalResolver);
          },
          () => {
            if (originalSession && !closePrivateSession(originalSession))
              throw Error('private_cleanup_required');
          },
          () => {
            if (originalDatabase) closeBrowserDatabase(originalDatabase);
          },
          () => input.close?.(),
          () => listeners.clear()
        ];
      }
      let unfinished = Array.from<() => void>([]);
      for (const cleanup of pendingClose) {
        try {
          cleanup();
        } catch {
          unfinished = unfinished.concat(cleanup);
        }
      }
      pendingClose = unfinished;
      // Closed admission and completed disposal are separate facts. A failed
      // original cleanup remains owned and retryable after invalidation.
      if (unfinished.length > 0) throw Error('inbox_setup_cleanup_required');
    }
  });
  return token;
}
// Production egress always comes from the root's governed runtime and fixed
// deployment manifest. No fixture, arbitrary relay, signing template or sender.
export function createRuntimeInboxSetupView(
  context: IdentityViewContext,
  runtime: PublicRuntime
): InboxSetupView | undefined {
  const identity = identityViewSession(context);
  if (!identity) return undefined;
  const view = createPublicView(runtime);
  try {
    const controller = createInboxSetupView({
      identity,
      policy: deploymentRelayPolicy,
      lookup: () => {
        const owner = identityMessagingOwnership(identity);
        if (!owner) throw new Error('inbox_owner_unavailable');
        return Promise.resolve(
          resolveInboxPreference(view, beginPublicViewRun(view), owner.owner)
        );
      },
      readback: (record, owner, id) =>
        resolveInboxReadback(
          view,
          beginPublicViewRun(view),
          record,
          owner,
          id,
          deploymentRelayPolicy
        ),
      close: () => disposePublicView(view)
    });
    if (!controller) disposePublicView(view);
    return controller;
  } catch {
    disposePublicView(view);
    return undefined;
  }
}
export function inboxSetupViewSnapshot(
  view: InboxSetupView | undefined
): InboxSetupViewSnapshot {
  return (view && views.get(view)?.snapshot()) || unavailable();
}
export function subscribeInboxSetupView(
  view: InboxSetupView | undefined,
  listener: (state: InboxSetupViewSnapshot) => void
): () => void {
  return (view && views.get(view)?.subscribe(listener)) || (() => {});
}
export async function checkInboxSetupView(
  view: InboxSetupView | undefined
): Promise<void> {
  await (view && views.get(view)?.check());
}
export async function reviewInboxSetupView(
  view: InboxSetupView | undefined,
  selected: string[],
  removeTags: number[],
  removeFields: string[]
): Promise<void> {
  await (view && views.get(view)?.review(selected, removeTags, removeFields));
}
export async function enableInboxSetupView(
  view: InboxSetupView | undefined
): Promise<void> {
  await (view && views.get(view)?.enable());
}
export async function unlockInboxSetupView(
  view: InboxSetupView | undefined
): Promise<void> {
  await (view && views.get(view)?.unlock());
}
export async function readbackInboxSetupView(
  view: InboxSetupView | undefined
): Promise<void> {
  await (view && views.get(view)?.readback());
}
export function stopInboxSetupView(view: InboxSetupView | undefined): void {
  if (view) views.get(view)?.stop();
}
export function closeInboxSetupView(view: InboxSetupView | undefined): void {
  if (view) views.get(view)?.close();
}

// Detached original-token observation for trusted page orchestration. Neither
// its scalar fields nor mutation of this fresh object changes stored custody.
export function inboxSetupViewOwnership(view: InboxSetupView | undefined) {
  return view && views.get(view)?.ownership();
}
export function observeInboxSetupView(view: InboxSetupView | undefined): void {
  if (view) views.get(view)?.observe();
}
