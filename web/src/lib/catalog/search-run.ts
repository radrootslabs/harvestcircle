import {
  PUBLIC_QUERY_BUDGETS,
  PUBLIC_SEARCH_RUN_BUDGETS
} from '../config/budgets.ts';
import type { PublicFilter } from '../nostr/exports.ts';
import { headResolutionQueries } from '../nostr/product-queries.ts';
import {
  publicRunSnapshot,
  publicRequestScopeSnapshot,
  closePublicRequest,
  cancelPublicRun,
  type PublicRun,
  type PublicRequest
} from '../nostr/request-scope.ts';
import {
  beginPublicViewRun,
  publicViewRunCurrent,
  publicViewProjectionAvailable,
  publicViewNip50Sources,
  subscribePublicView,
  disposePublicView,
  type PublicView
} from '../runtime/public-runtime.ts';
import {
  createPublicHeadCandidate,
  publicHeadKey,
  publicHeadSnapshot,
  selectPublicHead,
  type PublicHead
} from './heads.ts';
import { normalizePublicQuery } from './query-input.ts';
import {
  createPublicViewHeadResolver,
  resolveHeads,
  resolveKnownHeads,
  headResolutionSnapshot,
  type HeadResolver
} from './resolve-head.ts';
import { searchResolvedFood } from './search-match.ts';
import {
  searchRefreshState,
  type SearchScope,
  type SearchSnapshot
} from './search-state.ts';
import type { WallClock } from './clock-policy.ts';
import { nip50SearchQuery } from '../nostr/search-query.ts';
declare const coordinatorBrand: unique symbol;
declare const generationBrand: unique symbol;
export type FoodSearchCoordinator = Readonly<{
  readonly [coordinatorBrand]: true;
}>;
export type FoodSearchRun = Readonly<{ readonly [generationBrand]: true }>;
interface Coordinator {
  readonly view: PublicView;
  readonly clock: WallClock;
  readonly current: () => FoodSearchRun | undefined;
  readonly reserve: (token: FoodSearchRun) => void;
  readonly closed: () => boolean;
  readonly close: () => void;
}
interface Session {
  readonly coordinator: Coordinator;
  readonly token: FoodSearchRun;
  readonly query: string;
  readonly heads: () => readonly PublicHead[];
  readonly pending: () => readonly PublicHead[];
  readonly resolved: (heads: readonly PublicHead[]) => void;
  readonly remember: (head: PublicHead) => boolean;
  readonly requests: () => readonly PublicRequest[];
  readonly track: (request: PublicRequest) => void;
  readonly reserveSource: () => void;
  readonly reserveSample: () => void;
  readonly run: () => PublicRun | undefined;
  readonly bindRun: (run: PublicRun) => void;
  readonly model: () => HeadResolver | undefined;
  readonly bindModel: (model: HeadResolver) => void;
  readonly flags: () => Readonly<{
    cancelled: boolean;
    failed: boolean;
    capped: boolean;
  }>;
  readonly fail: () => void;
  readonly cancel: () => void;
}
const coordinators = new WeakMap<FoodSearchCoordinator, Coordinator>();
const viewCoordinators = new WeakMap<
  PublicView,
  Readonly<{ clock: WallClock; token: FoodSearchCoordinator }>
>();
const sessions = new WeakMap<FoodSearchRun, Session>();
function coordinatorOf(token: FoodSearchCoordinator): Coordinator {
  const owner = coordinators.get(token);
  if (!owner) throw new Error('food_search_coordinator_invalid');
  return owner;
}
function sessionOf(token: FoodSearchRun): Session {
  const session = sessions.get(token);
  if (!session) throw new Error('food_search_invalid');
  return session;
}
function current(session: Session): boolean {
  return (
    !session.coordinator.closed() &&
    session.coordinator.current() === session.token
  );
}
function requireCurrent(session: Session): void {
  if (!current(session)) throw new Error('food_search_superseded');
}
// All mutation stays in lexical plain-model owners; callers get opaque handles
// and detached snapshots, never map-returned mutable records or SDK objects.
function createSession(
  coordinator: Coordinator,
  token: FoodSearchRun,
  query: string,
  seed: readonly PublicHead[],
  fallback: HeadResolver | undefined
): Session {
  const heads = new Map(seed.map((head) => [publicHeadKey(head), head]));
  const resolved = new Map<string, true>();
  const requests = new Map<PublicRequest, true>();
  let run: PublicRun | undefined,
    model = fallback;
  let cancelled = false,
    failed = false,
    capped = false;
  let attempts = 0;
  let sampleRounds = 0;
  return {
    coordinator,
    token,
    query,
    heads: () => Array.from(heads.values()),
    pending: () =>
      Array.from(heads.values()).filter(
        (head) => !resolved.has(publicHeadKey(head))
      ),
    resolved: (values) => {
      for (const head of values) resolved.set(publicHeadKey(head), true);
    },
    remember(head) {
      const key = publicHeadKey(head);
      if (
        !heads.has(key) &&
        heads.size >= PUBLIC_QUERY_BUDGETS.coordinatesPerRun
      ) {
        capped = true;
        return false;
      }
      heads.set(key, selectPublicHead(heads.get(key), head).head);
      return true;
    },
    requests: () => Array.from(requests.keys()),
    track: (request) => {
      requests.set(request, true);
    },
    reserveSource() {
      if (attempts >= PUBLIC_SEARCH_RUN_BUDGETS.primaryScopes) {
        capped = true;
        throw new Error('food_search_primary_limit');
      }
      attempts++;
    },
    reserveSample() {
      if (sampleRounds >= PUBLIC_SEARCH_RUN_BUDGETS.nip50Rounds)
        throw new Error('food_search_sample_round_limit');
      sampleRounds++;
    },
    run: () => run,
    bindRun: (value) => {
      run = value;
    },
    model: () => model,
    bindModel: (value) => {
      model = value;
    },
    flags: () => ({ cancelled, failed, capped }),
    fail: () => {
      failed = true;
    },
    cancel() {
      cancelled = true;
      if (run) cancelPublicRun(run);
    }
  };
}
export function createFoodSearchCoordinator(
  view: PublicView,
  clock: WallClock
): FoodSearchCoordinator {
  publicViewProjectionAvailable(view);
  if (typeof clock.nowSeconds !== 'function')
    throw new Error('food_search_clock_invalid');
  const existing = viewCoordinators.get(view);
  if (existing) {
    if (existing.clock !== clock) throw new Error('food_search_clock_changed');
    return existing.token;
  }
  const token = Object.freeze({}) as FoodSearchCoordinator;
  let generation: FoodSearchRun | undefined,
    closed = false;
  coordinators.set(token, {
    view,
    clock,
    current: () => generation,
    reserve: (value) => {
      generation = value;
    },
    closed: () => closed,
    close() {
      closed = true;
      // Preserve failed cleanup controls; repeated disposal retries the view.
      disposePublicView(view);
      generation = undefined;
    }
  });
  viewCoordinators.set(view, { clock, token });
  return token;
}
export function beginFoodSearch(
  coordinator: FoodSearchCoordinator,
  input: unknown,
  known: readonly PublicHead[] = []
): FoodSearchRun {
  const owner = coordinatorOf(coordinator);
  if (owner.closed()) throw new Error('food_search_closed');
  const query = normalizePublicQuery(input);
  if (!query.ok) throw new Error(query.error);
  // Complete seed validation precedes clocks, cache and run effects.
  headResolutionQueries(known);
  const previousToken = owner.current();
  const previous =
    previousToken === undefined ? undefined : sessionOf(previousToken);
  const heads = new Map(
    previous?.heads().map((head) => [publicHeadKey(head), head])
  );
  for (const head of known) {
    const key = publicHeadKey(head);
    heads.set(key, selectPublicHead(heads.get(key), head).head);
  }
  if (heads.size > PUBLIC_QUERY_BUDGETS.coordinatesPerRun)
    throw new Error('head_resolver_coordinate_limit');
  const token = Object.freeze({}) as FoodSearchRun;
  const seed = Array.from(heads.values());
  const session = createSession(
    owner,
    token,
    query.text,
    seed,
    previous?.model()
  );
  sessions.set(token, session);
  owner.reserve(token); // Reserve before previous settlement or injected clocks.
  try {
    const run = beginPublicViewRun(owner.view);
    session.bindRun(run);
    requireCurrent(session);
    const model = createPublicViewHeadResolver(owner.view, run, owner.clock);
    session.bindModel(model);
    requireCurrent(session);
    session.resolved(seed);
    if (seed.length) resolveHeads(model, seed);
    requireCurrent(session);
    return token;
  } catch {
    if (!current(session)) throw new Error('food_search_superseded');
    session.fail();
    const run = session.run();
    if (run) {
      try {
        cancelPublicRun(run);
      } catch {
        /* Still owned, retryable. */
      }
    }
    throw new Error('food_search_start_failed');
  }
}
export function foodSearchRequestOwner(token: FoodSearchRun): PublicRun {
  const session = sessionOf(token);
  requireCurrent(session);
  const run = session.run();
  if (!run || !publicViewRunCurrent(session.coordinator.view, run))
    throw new Error('food_search_inactive');
  requireCurrent(session);
  return run;
}
// Filter strategies remain separate; primary sources and all auxiliaries share
// one genuine anonymous run rather than creating transport or budget owners.
export function subscribeFoodSearch(
  token: FoodSearchRun,
  filters: readonly PublicFilter[]
): PublicRequest {
  if (filters.some((filter) => 'search' in filter))
    throw new Error('food_search_sample_required');
  return subscribeSearchSource(token, filters);
}
function subscribeSearchSource(
  token: FoodSearchRun,
  filters: readonly PublicFilter[],
  sampleSource?: string
): PublicRequest {
  const session = sessionOf(token),
    run = foodSearchRequestOwner(token);
  session.reserveSource(); // Reservation precedes SDK effects and never refunds.
  let request: PublicRequest;
  try {
    request = subscribePublicView(
      session.coordinator.view,
      run,
      'search',
      filters,
      (proof) => {
        // Admission already includes a valid inclusive tail that stopped ingress.
        if (!current(session)) return;
        const head = createPublicHeadCandidate(proof);
        if (!head || publicHeadSnapshot(head).kind !== 30402) return;
        if (!session.remember(head)) cancelPublicRun(run);
      },
      sampleSource
    );
  } catch {
    if (current(session)) session.fail();
    throw new Error('food_search_request_failed');
  }
  session.track(request);
  if (!current(session)) {
    closePublicRequest(request);
    throw new Error('food_search_superseded');
  }
  return request;
}

export function sampleFoodSearch(
  token: FoodSearchRun
): readonly PublicRequest[] {
  const session = sessionOf(token);
  requireCurrent(session);
  const filters = nip50SearchQuery(session.query);
  if (!filters.length) return [];
  const sources = publicViewNip50Sources(session.coordinator.view);
  if (!sources.length) return [];
  session.reserveSample(); // Failed attempts cannot recreate the relevance round.
  const samples = new Map<PublicRequest, true>();
  for (const source of sources) {
    requireCurrent(session);
    try {
      samples.set(subscribeSearchSource(token, filters, source), true);
    } catch {
      // A refused/failed source cannot discard other candidates or the fallback.
      requireCurrent(session);
      session.fail();
    }
  }
  return Array.from(samples.keys());
}
export function foodSearchSnapshot(
  coordinator: FoodSearchCoordinator
): SearchSnapshot<FoodSearchRun> | undefined {
  const owner = coordinatorOf(coordinator),
    token = owner.current();
  if (owner.closed() || token === undefined) return undefined;
  const session = sessionOf(token),
    model = session.model(),
    ownedRun = session.run();
  if (ownedRun && model) {
    const active = publicViewRunCurrent(owner.view, ownedRun);
    if (!current(session)) return undefined;
    const pending = session.pending();
    if (pending.length) {
      try {
        if (active) resolveHeads(model, pending);
        else resolveKnownHeads(model, pending);
        session.resolved(pending);
      } catch {
        session.fail();
      }
    }
  }
  const raw = model ? headResolutionSnapshot(model) : [];
  const matched = model ? searchResolvedFood(model, session.query) : undefined;
  const scopes = new Map<object, SearchScope>();
  for (const request of session.requests()) {
    const scope = publicRequestScopeSnapshot(request);
    scopes.set(scope.result.context, scope);
  }
  for (const row of raw)
    for (const scope of [row.headSources, row.deletionSources])
      if (scope) scopes.set(scope.result.context, scope);
  const available =
    publicViewProjectionAvailable(owner.view) &&
    (matched?.ok ? matched.available : true);
  const run = ownedRun ? publicRunSnapshot(ownedRun) : undefined;
  // Model/run clocks may synchronously supersede or dispose this generation.
  if (!current(session)) return undefined;
  const sources = Array.from(scopes.values()),
    flags = session.flags();
  return {
    generation: token,
    query: session.query,
    refresh: searchRefreshState(
      available,
      run,
      sources,
      flags.cancelled,
      flags.failed,
      flags.capped
    ),
    available,
    rows: available && matched?.ok ? matched.rows : [],
    scopes: sources,
    run,
    definitiveAbsence: false
  };
}
export function cancelFoodSearch(token: FoodSearchRun): void {
  const session = sessionOf(token);
  requireCurrent(session);
  session.cancel();
}
export function closeFoodSearchCoordinator(
  coordinator: FoodSearchCoordinator
): void {
  coordinatorOf(coordinator).close();
}
