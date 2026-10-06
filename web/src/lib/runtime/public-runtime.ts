import { deploymentRelayPolicy } from '../config/deployment-relays.ts';
import type { RelayPolicy } from '../config/relays.ts';
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
  type PublicStore
} from '../nostr/public-store.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openPublicRequest,
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
      let terminal = false;
      const token = Object.freeze({}) as PublicRuntime;
      const runtime: RuntimeOwner = {
        pool,
        store,
        scheduler,
        policy,
        views: registered,
        closed: () => terminal,
        close() {
          terminal = true;
          let failed = false;
          for (const dispose of registered.values())
            if (!dispose()) failed = true;
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
  const shared = runtimeOf(runtime),
    token = Object.freeze({}) as PublicView;
  let run: PublicRun | undefined,
    disposed = false;
  const owner: ViewOwner = {
    runtime: shared,
    begin() {
      if (disposed || shared.closed()) throw new Error('public_view_closed');
      if (run) disposePublicRun(run);
      run = createPublicRun(shared.scheduler, shared.policy);
      return run;
    },
    current(candidate) {
      return !disposed && !shared.closed() && run === candidate;
    },
    dispose() {
      disposed = true;
      try {
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
export function disposePublicView(view: PublicView): void {
  if (!viewOf(view).dispose()) throw new Error('public_view_close_failed');
}
export function subscribePublicView(
  view: PublicView,
  run: PublicRun,
  kind: RequestKind,
  filters: readonly PublicFilter[],
  onVerified: (event: VerifiedEnvelope) => void
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
        next
      ),
    (event) => {
      if (!owner.current(run)) return;
      insertPublicEnvelope(owner.runtime.store, event);
      if (owner.current(run)) onVerified(event);
    }
  );
}
