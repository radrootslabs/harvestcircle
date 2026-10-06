import type { PublicView } from '../runtime/public-runtime.ts';
import type { PublicHead } from './heads.ts';
import type { WallClock } from './clock-policy.ts';
import type { SearchSnapshot } from './search-state.ts';
import {
  createFoodSearchCoordinator,
  beginFoodSearch,
  sampleFoodSearch,
  chronologicalFoodSearch,
  foodSearchSnapshot,
  resolveFoodSearchPublishers,
  showMoreFoodSearch,
  searchOlderFoodSearch,
  closeFoodSearchCoordinator,
  type FoodSearchRun
} from './search-run.ts';
export type SearchViewSnapshot = SearchSnapshot<FoodSearchRun>;
export type ForegroundSchedule = (
  callback: () => void,
  delay: number
) => () => void;
declare const viewBrand: unique symbol;
export type SearchView = Readonly<{ readonly [viewBrand]: true }>;
interface Owner {
  readonly start: (query: unknown, known: readonly PublicHead[]) => void;
  readonly more: () => void;
  readonly older: () => void;
  readonly snapshot: () => SearchViewSnapshot | undefined;
  readonly close: () => void;
}
const owners = new WeakMap<SearchView, Owner>();
function ownerOf(view: SearchView): Owner {
  const owner = owners.get(view);
  if (!owner) throw Error('search_view_invalid');
  return owner;
}
function foreground(callback: () => void, delay: number): () => void {
  const timer = setTimeout(() => {
    callback();
  }, delay);
  return () => {
    clearTimeout(timer);
  };
}
// This owner observes a finite public run; it never creates a background poll,
// transport, shared cache or new deadline. Explicit actions alone restart runs.
export function createSearchView(
  view: PublicView,
  clock: WallClock,
  onUpdate: (value: SearchViewSnapshot) => void,
  schedule: ForegroundSchedule = foreground
): SearchView {
  const coordinator = createFoodSearchCoordinator(view, clock);
  let generation: FoodSearchRun | undefined,
    cancel: (() => void) | undefined,
    closed = false;
  function stop() {
    const previous = cancel;
    cancel = undefined;
    previous?.();
  }
  function current(token: FoodSearchRun) {
    return !closed && generation === token;
  }
  function requireOpen() {
    if (closed) throw Error('search_view_closed');
  }
  function publish(token: FoodSearchRun) {
    const value = foodSearchSnapshot(coordinator);
    if (value && current(token) && value.generation === token) onUpdate(value);
    return value;
  }
  function queue(token: FoodSearchRun) {
    if (!current(token)) return;
    const value = foodSearchSnapshot(coordinator);
    if (!value?.run?.active || !current(token)) return;
    const pending = schedule(() => {
      if (!current(token)) return;
      cancel = undefined;
      observe(token);
    }, 250);
    if (current(token)) cancel = pending;
    else pending();
  }
  function observe(token: FoodSearchRun) {
    if (!current(token)) return;
    const value = foodSearchSnapshot(coordinator);
    if (!value || !current(token)) return;
    if (value.run?.active && value.available && value.listings.length) {
      try {
        resolveFoodSearchPublishers(token);
      } catch {
        /* Failed metadata retains key fallback and source uncertainty. */
      }
    }
    if (!current(token)) return;
    const updated = publish(token);
    if (updated?.run?.active && updated.run.activeRequests > 0) queue(token);
  }
  const token = Object.freeze({}) as SearchView;
  owners.set(token, {
    start(query, known) {
      requireOpen();
      // Coordinator validates the complete query and seeds before any effects;
      // a rejected edit cannot cancel a previously accepted foreground observer.
      const next = beginFoodSearch(coordinator, query, known);
      stop();
      generation = next;
      try {
        sampleFoodSearch(next);
      } catch {
        /* Retain ordinary chronological fallback. */
      }
      if (!current(next)) return;
      try {
        chronologicalFoodSearch(next);
      } catch {
        /* Snapshot reports the actual failed source; known rows survive. */
      }
      if (current(next)) {
        publish(next);
        queue(next);
      }
    },
    more() {
      requireOpen();
      if (!generation) throw Error('search_view_inactive');
      const next = generation;
      showMoreFoodSearch(next);
      stop();
      publish(next);
      queue(next);
    },
    older() {
      requireOpen();
      const next = searchOlderFoodSearch(coordinator);
      stop();
      generation = next;
      publish(next);
      queue(next);
    },
    snapshot: () => foodSearchSnapshot(coordinator),
    close() {
      closed = true;
      stop();
      closeFoodSearchCoordinator(coordinator);
      generation = undefined;
    }
  });
  return token;
}
export function startSearchView(
  view: SearchView,
  query: unknown,
  known: readonly PublicHead[] = []
): void {
  ownerOf(view).start(query, known);
}
export function moreSearchView(view: SearchView): void {
  ownerOf(view).more();
}
export function olderSearchView(view: SearchView): void {
  ownerOf(view).older();
}
export function searchViewSnapshot(
  view: SearchView
): SearchViewSnapshot | undefined {
  return ownerOf(view).snapshot();
}
export function closeSearchView(view: SearchView): void {
  ownerOf(view).close();
}
