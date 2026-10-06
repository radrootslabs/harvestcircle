import {
  RELAY_BUDGETS,
  PUBLIC_QUERY_BUDGETS,
  PUBLIC_SEARCH_BUDGETS,
  PUBLIC_SEARCH_RUN_BUDGETS
} from '../config/budgets.ts';
import { publicHeadSnapshot, type PublicHead } from './heads.ts';
export type ChronologicalCoverage =
  'unchecked' | 'checking' | 'bounded-eose' | 'partial';
export type ChronologicalProgress = Readonly<{
  windows: number;
  active: boolean;
  until: number | undefined;
  gap: boolean;
  coverage: ChronologicalCoverage;
  observed: number;
  definitiveAbsence: false;
}>;
export type ChronologicalSource = Readonly<{
  source: string;
  saturated: boolean;
  eventIds: readonly string[];
}>;
export interface ChronologicalWindow {
  readonly until: number | undefined;
  readonly observe: (head: PublicHead) => void;
  readonly finish: (
    saturated: boolean,
    coverage: 'bounded-eose' | 'partial',
    sources?: readonly ChronologicalSource[]
  ) => void;
}
export interface ChronologicalContinuation {
  readonly begin: () => ChronologicalWindow;
  readonly snapshot: () => ChronologicalProgress;
  readonly restart: () => ChronologicalContinuation;
}
// Pure bookkeeping, held only in the coordinator's private session. The
// coordinator alone associates verified chronological callbacks and real scope
// outcomes; sample/auxiliary deliveries never reach this owner.
function continuation(
  seedUntil?: number,
  seedIds: readonly string[] = [],
  seedPartial = false
): ChronologicalContinuation {
  let until = seedUntil,
    windows = 0,
    active = false,
    gap = false,
    partial = seedPartial,
    observed = 0;
  let boundary = new Map(seedIds.map((id) => [id, true] as const));
  const owner: ChronologicalContinuation = {
    begin() {
      if (active) throw new Error('food_search_chronological_pending');
      if (gap) throw new Error('food_search_chronological_gap');
      if (windows >= PUBLIC_SEARCH_RUN_BUDGETS.chronologicalWindows)
        throw new Error('food_search_chronological_window_limit');
      windows++;
      active = true;
      const requestedUntil = until,
        ids = new Map<string, number>();
      let settled = false;
      return {
        until: requestedUntil,
        observe(head) {
          if (settled)
            throw new Error('food_search_chronological_window_closed');
          const value = publicHeadSnapshot(head);
          if (
            value.kind !== 30402 ||
            (requestedUntil !== undefined && value.created_at > requestedUntil)
          )
            return;
          if (
            !ids.has(value.id) &&
            ids.size >= PUBLIC_QUERY_BUDGETS.coordinatesPerRun
          ) {
            gap = true;
            partial = true;
            return;
          }
          ids.set(value.id, value.created_at);
        },
        finish(saturated, coverage, sources) {
          if (settled) return;
          const facts = sources ?? [
            { source: '', saturated, eventIds: Array.from(ids.keys()) }
          ];
          if (
            facts.length > RELAY_BUDGETS.public ||
            facts.some(
              (source) =>
                source.eventIds.length > PUBLIC_QUERY_BUDGETS.coordinatesPerRun
            )
          )
            throw new Error('food_search_chronological_source_limit');
          settled = true;
          active = false;
          partial = partial || coverage === 'partial';
          observed = ids.size;
          const times = Array.from(ids.values());
          let oldest = times.length
            ? times.reduce((minimum, value) => Math.min(minimum, value))
            : undefined;
          let protectedUntil: number | undefined;
          for (const source of facts) {
            if (!source.saturated) continue;
            const sourceTimes = source.eventIds
              .filter((id) => ids.has(id))
              .map((id) => ids.get(id)!);
            const sourceOldest = sourceTimes.length
              ? sourceTimes.reduce((minimum, value) => Math.min(minimum, value))
              : undefined;
            if (sourceOldest === undefined) {
              gap = true;
              partial = true;
              continue;
            }
            protectedUntil =
              protectedUntil === undefined
                ? sourceOldest
                : Math.max(protectedUntil, sourceOldest);
            const progress =
              requestedUntil === undefined ||
              sourceOldest < requestedUntil ||
              source.eventIds.some(
                (id) =>
                  ids.get(id) === sourceOldest &&
                  !boundary.has(source.source + '\0' + id)
              );
            if (!progress) {
              gap = true;
              partial = true;
            }
          }
          // Every saturated source's unresolved boundary is protected. Another
          // source's older item must never move a shared cursor below it.
          oldest =
            gap && requestedUntil !== undefined
              ? requestedUntil
              : (protectedUntil ?? oldest);
          if (oldest !== undefined) {
            if (oldest !== requestedUntil) boundary = new Map();
            for (const source of facts)
              for (const id of source.eventIds) {
                if (ids.get(id) !== oldest) continue;
                const key = source.source + '\0' + id;
                if (
                  !boundary.has(key) &&
                  boundary.size >= PUBLIC_QUERY_BUDGETS.coordinatesPerRun
                ) {
                  gap = true;
                  partial = true;
                  break;
                }
                boundary.set(key, true);
              }
            until = oldest;
          }
        }
      };
    },
    snapshot: () => ({
      windows,
      active,
      until,
      gap,
      coverage: active
        ? 'checking'
        : partial || gap
          ? 'partial'
          : windows
            ? 'bounded-eose'
            : 'unchecked',
      observed,
      definitiveAbsence: false
    }),
    restart() {
      if (active) throw new Error('food_search_chronological_pending');
      if (gap) throw new Error('food_search_chronological_gap');
      if (until === undefined)
        throw new Error('food_search_chronological_boundary_missing');
      return continuation(until, Array.from(boundary.keys()), partial);
    }
  };
  return owner;
}
export function createChronologicalContinuation(): ChronologicalContinuation {
  return continuation();
}

// Local pages preserve the caller's observed ordering. No clocks or transport.
export function localSearchPage<T>(
  rows: readonly T[],
  pages: number
): Readonly<{ rows: readonly T[]; hasMore: boolean }> {
  if (
    !Number.isSafeInteger(pages) ||
    pages < 1 ||
    pages >
      Math.ceil(
        PUBLIC_QUERY_BUDGETS.coordinatesPerRun / PUBLIC_SEARCH_BUDGETS.pageRows
      )
  )
    throw new Error('food_search_local_page_invalid');
  const count = pages * PUBLIC_SEARCH_BUDGETS.pageRows;
  return { rows: rows.slice(0, count), hasMore: rows.length > count };
}
