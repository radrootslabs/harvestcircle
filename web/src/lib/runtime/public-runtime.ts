import { deploymentRelayPolicy } from '../config/deployment-relays.ts';
import type { RelayPolicy } from '../config/relays.ts';
import { qualifiedNip50Sources } from '../nostr/search-sources.ts';
import {
  getPublicPool,
  subscribePublicPool,
  closePublicPool,
  type PublicPool
} from '../nostr/public-pool.ts';
import {
  getPublicStore,
  insertPublicEnvelope,
  closePublicStore,
  publicStoreRetention,
  publicStoreKnownEvidence,
  publicStoreEnvelope,
  type PublicStore
} from '../nostr/public-store.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openPublicRequest,
  openInboxRequest,
  disposePublicRun,
  closePublicScheduler,
  publicRunSnapshot,
  type PublicScheduler,
  type PublicRun,
  type PublicRequest,
  type RequestKind,
  type RequestClock
} from '../nostr/request-scope.ts';
import type { PublicFilter } from '../nostr/exports.ts';
import { inboxPreferenceQueries } from '../nostr/inbox-queries.ts';
import {
  publicHeadEnvelope,
  publicHeadSnapshot,
  type PublicHead
} from '../catalog/heads.ts';
import type { KnownPublicEvidence } from '../catalog/retention.ts';
import type { VerifiedEnvelope } from '../nostr/verified-envelope.ts';
declare const contextBrand: unique symbol;
declare const runtimeBrand: unique symbol;
declare const viewBrand: unique symbol;
export type PublicRuntimeContext = Readonly<{ readonly [contextBrand]: true }>;
export type PublicRuntime = Readonly<{ readonly [runtimeBrand]: true }>;
export type PublicView = Readonly<{ readonly [viewBrand]: true }>;
export const PUBLIC_RUNTIME_CONTEXT = 'harvestcircle_public_runtime';
interface RuntimeOwner {
  readonly pool: PublicPool;
  readonly store: PublicStore;
  readonly scheduler: PublicScheduler;
  readonly policy: RelayPolicy;
  readonly views: Map<PublicView, () => boolean>;
  readonly closed: () => boolean;
  readonly projectionValid: (retry?: boolean) => boolean;
  readonly invalidateProjection: () => void;
  readonly close: () => void;
}
interface ContextOwner {
  readonly value: () => PublicRuntime | undefined;
  readonly mount: (
    policy: RelayPolicy,
    clock?: RequestClock
  ) => PublicRuntime | undefined;
  readonly ready: Promise<PublicRuntime | undefined>;
  readonly close: () => void;
}
interface ViewOwner {
  readonly runtime: RuntimeOwner;
  readonly begin: () => PublicRun;
  readonly current: (run: PublicRun) => boolean;
  readonly settling: () => boolean;
  readonly settleWith: (refresh: () => void) => void;
  readonly dispose: () => boolean;
}
const contexts = new WeakMap<PublicRuntimeContext, ContextOwner>(),
  runtimes = new WeakMap<PublicRuntime, RuntimeOwner>(),
  views = new WeakMap<PublicView, ViewOwner>();
let browserContext: PublicRuntimeContext | undefined;
function contextOf(token: PublicRuntimeContext): ContextOwner {
  const owner = contexts.get(token);
  if (!owner) throw new Error('public_runtime_context_invalid');
  return owner;
}
function runtimeOf(token: PublicRuntime): RuntimeOwner {
  const owner = runtimes.get(token);
  if (!owner) throw new Error('public_runtime_invalid');
  if (owner.closed()) throw new Error('public_runtime_closed');
  return owner;
}
function viewOf(token: PublicView): ViewOwner {
  const owner = views.get(token);
  if (!owner) throw new Error('public_view_invalid');
  return owner;
}
// Context construction is SSR-pure. Root onMount acquires the browser lifetime;
// children can await readiness even when their onMount runs before the root's.
export function createPublicRuntimeContext(): PublicRuntimeContext {
  let value: PublicRuntime | undefined,
    closed = false;
  let ready: (value: PublicRuntime | undefined) => void = () => {};
  let selectedClock: RequestClock | undefined;
  const promise = new Promise<PublicRuntime | undefined>((resolve) => {
    ready = resolve;
  });
  const owner: ContextOwner = {
    value: () => (closed ? undefined : value),
    ready: promise,
    mount(policy, clock) {
      if (closed) throw new Error('public_runtime_closed');
      if (typeof window === 'undefined') return undefined;
      if (value) {
        const prior = runtimes.get(value);
        if (!prior || prior.policy !== policy)
          throw new Error('public_runtime_policy_changed');
        if (clock !== selectedClock)
          throw new Error('public_runtime_clock_changed');
        return value;
      }
      if (browserContext && browserContext !== contextToken)
        throw new Error('public_runtime_owner_changed');
      const pool = getPublicPool(policy),
        store = getPublicStore();
      if (!pool || !store) return undefined;
      const scheduler = createPublicScheduler(clock),
        registered = new Map<PublicView, () => boolean>();
      let terminal = false,
        invalidated = false,
        invalidating = false;
      const token = Object.freeze({}) as PublicRuntime;
      const runtime: RuntimeOwner = {
        pool,
        store,
        scheduler,
        policy,
        views: registered,
        closed: () => terminal,
        projectionValid(retry = true) {
          const retained = publicStoreRetention(store);
          if (retained.stopped || (!terminal && retained.closed))
            invalidated = true;
          // A settler reads the terminal flag without recursively retrying its
          // own disposal. The outer cleanup loop keeps failed controls live.
          if (invalidated && retry) runtime.invalidateProjection();
          return !invalidated;
        },
        invalidateProjection() {
          invalidated = true;
          if (invalidating) return;
          invalidating = true;
          let failed = false;
          try {
            for (const dispose of registered.values())
              if (!dispose()) failed = true;
          } finally {
            invalidating = false;
          }
          if (failed) throw new Error('public_projection_close_failed');
        },
        close() {
          terminal = true;
          let failed = false;
          for (const dispose of registered.values())
            if (!dispose()) failed = true;
          // Reentrant or failed settlement cannot preserve a coherent projection
          // across store teardown. Terminal uncertainty clears every view.
          if (failed) invalidated = true;
          try {
            closePublicScheduler(scheduler);
          } catch {
            failed = true;
          }
          try {
            closePublicPool(pool);
          } catch {
            failed = true;
          }
          try {
            closePublicStore(store);
          } catch {
            failed = true;
          }
          if (failed) throw new Error('public_runtime_close_failed');
        }
      };
      runtimes.set(token, runtime);
      value = token;
      browserContext = contextToken;
      selectedClock = clock;
      ready(token);
      return token;
    },
    close() {
      closed = true;
      ready(undefined);
      if (value) {
        const runtime = runtimes.get(value);
        if (runtime) runtime.close();
      }
    }
  };
  const contextToken = Object.freeze({}) as PublicRuntimeContext;
  contexts.set(contextToken, owner);
  return contextToken;
}
export function publicRuntime(
  context: PublicRuntimeContext
): PublicRuntime | undefined {
  return contextOf(context).value();
}
export function publicRuntimeReady(
  context: PublicRuntimeContext
): Promise<PublicRuntime | undefined> {
  return contextOf(context).ready;
}
export function mountPublicRuntime(
  context: PublicRuntimeContext,
  policy: RelayPolicy = deploymentRelayPolicy,
  clock?: RequestClock
): PublicRuntime | undefined {
  return contextOf(context).mount(policy, clock);
}
export function closePublicRuntime(context: PublicRuntimeContext): void {
  contextOf(context).close();
}
export function createPublicView(runtime: PublicRuntime): PublicView {
  const shared = runtimeOf(runtime);
  if (!shared.projectionValid())
    throw new Error('public_projection_unavailable');
  const token = Object.freeze({}) as PublicView;
  let run: PublicRun | undefined,
    disposed = false;
  let generation = 0,
    settler: (() => void) | undefined,
    settling = false;
  function settle() {
    const refresh = settler;
    settler = undefined; // Detach before any caller-controlled clock reentry.
    if (!refresh) return;
    settling = true;
    try {
      refresh?.();
    } catch (error) {
      if (!settler) settler = refresh;
      throw error;
    } finally {
      settling = false;
    }
  }
  const owner: ViewOwner = {
    runtime: shared,
    begin() {
      if (disposed || shared.closed() || !shared.projectionValid())
        throw new Error('public_view_closed');
      const operation = ++generation,
        previous = run;
      // Generation reserves the transition; keep failed old controls retryable.
      try {
        settle();
      } finally {
        if (previous) disposePublicRun(previous);
      }
      if (
        operation !== generation ||
        disposed ||
        shared.closed() ||
        !shared.projectionValid()
      )
        throw new Error('public_view_run_superseded');
      const candidate = createPublicRun(shared.scheduler, shared.policy);
      // Run creation samples the injected clock. It may synchronously start a
      // newer view generation before returning; never overwrite that owner.
      if (
        operation !== generation ||
        disposed ||
        shared.closed() ||
        !shared.projectionValid()
      ) {
        disposePublicRun(candidate);
        throw new Error('public_view_run_superseded');
      }
      run = candidate;
      return run;
    },
    settling: () => settling,
    settleWith(refresh) {
      const previous = settler;
      settler = refresh;
      previous?.();
    },
    current(candidate) {
      return !disposed && !shared.closed() && run === candidate;
    },
    dispose() {
      disposed = true;
      generation++;
      if (settling) return false;
      try {
        settle();
        if (run) disposePublicRun(run);
        shared.views.delete(token);
        return true;
      } catch {
        return false;
      }
    }
  };
  views.set(token, owner);
  shared.views.set(token, () => owner.dispose());
  return token;
}
export function beginPublicViewRun(view: PublicView): PublicRun {
  return viewOf(view).begin();
}
export function publicViewRunCurrent(
  view: PublicView,
  run: PublicRun
): boolean {
  return viewOf(view).current(run) && publicRunSnapshot(run).active;
}
// Ownership survives an ordinary deadline so a foreground owner can publish
// its terminal result once. Supersession/disposal/shared closure revoke it.
export function publicViewOwnsRun(view: PublicView, run: PublicRun): boolean {
  return viewOf(view).current(run);
}
export function disposePublicView(view: PublicView): void {
  if (!viewOf(view).dispose()) throw new Error('public_view_close_failed');
}
export function subscribePublicView(
  view: PublicView,
  run: PublicRun,
  kind: RequestKind,
  filters: readonly PublicFilter[],
  onVerified: (event: VerifiedEnvelope) => void,
  sampleSource?: string
): PublicRequest {
  const owner = viewOf(view);
  if (!owner.current(run)) throw new Error('public_view_run_inactive');
  // Copy the request template once before SDK effects; caller mutations cannot
  // retarget this request. Product filter builders remain HCP038 and later.
  const frozen = JSON.stringify(filters);
  return openPublicRequest(
    run,
    kind,
    (next) =>
      subscribePublicPool(
        owner.runtime.pool,
        JSON.parse(frozen) as PublicFilter[],
        next,
        sampleSource
      ),
    (event) => {
      if (!owner.current(run)) return;
      const result = insertPublicEnvelope(owner.runtime.store, event);
      if (result === 'limit' || result === 'rejected' || result === 'closed') {
        owner.runtime.invalidateProjection();
        return;
      }
      if (
        (result === 'accepted' || result === 'duplicate') &&
        owner.current(run)
      )
        onVerified(event);
    },
    sampleSource
  );
}

export function publicViewNip50Sources(view: PublicView): readonly string[] {
  return qualifiedNip50Sources(viewOf(view).runtime.policy);
}

// Public metadata, dedicated ownership: no preference enters the food store.
// Fixed anonymous origins, clocks, scheduler and aggregate ingress stay shared.
export function subscribeInboxPreference(
  view: PublicView,
  run: PublicRun,
  author: unknown,
  onVerified: (event: VerifiedEnvelope, inboxSource?: string) => void
): PublicRequest {
  const owner = viewOf(view),
    filters = inboxPreferenceQueries(author);
  if (!owner.current(run)) throw new Error('public_view_run_inactive');
  return openInboxRequest(
    run,
    author,
    (next) => subscribePublicPool(owner.runtime.pool, filters, next),
    (event, inboxSource) => {
      if (owner.current(run)) onVerified(event, inboxSource);
    }
  );
}

// Capacity invalidation differs from ordinary teardown: last-known snapshots
// remain available on normal close, while a breached working set clears all views.
export function publicViewProjectionAvailable(view: PublicView): boolean {
  const owner = viewOf(view);
  return owner.runtime.projectionValid(!owner.settling());
}
export function publicViewKnownEvidence(
  view: PublicView,
  head: PublicHead
): KnownPublicEvidence {
  return publicStoreKnownEvidence(viewOf(view).runtime.store, head);
}
export function retainPublicViewHeads(
  view: PublicView,
  run: PublicRun,
  heads: readonly PublicHead[]
): boolean {
  const owner = viewOf(view);
  if (!owner.current(run) || !owner.runtime.projectionValid()) return false;
  const active = publicRunSnapshot(run).active;
  if (!owner.current(run) || !owner.runtime.projectionValid()) return false;
  if (!active) {
    // Inactive materialization cannot install or charge another event. Require
    // every genuine head to have already passed the shared retained cache.
    for (const head of heads) {
      if (
        !publicStoreEnvelope(owner.runtime.store, publicHeadSnapshot(head).id)
      )
        throw new Error('public_view_uncached_head');
    }
    return owner.current(run) && owner.runtime.projectionValid();
  }
  for (const head of heads) {
    const result = insertPublicEnvelope(
      owner.runtime.store,
      publicHeadEnvelope(head)
    );
    if (result !== 'accepted' && result !== 'duplicate') {
      owner.runtime.invalidateProjection();
      return false;
    }
  }
  return owner.runtime.projectionValid();
}

// Trusted model binding holds only the current projection for this view.
// Replacement/disposal settles it while retained shared lifecycle proof is live.
export function settlePublicViewProjection(
  view: PublicView,
  refresh: () => void
): void {
  viewOf(view).settleWith(refresh);
}
