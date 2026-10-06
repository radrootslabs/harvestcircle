import type { ChronologicalProgress } from './continuation.ts';
import type {
  publicRunSnapshot,
  publicRequestScopeSnapshot
} from '../nostr/request-scope.ts';
import type { HeadResolution } from './resolve-head.ts';
export type SearchScope = ReturnType<typeof publicRequestScopeSnapshot>;
export type SearchRefresh =
  | 'idle'
  | 'refreshing'
  | 'bounded-eose'
  | 'partial'
  | 'cancelled'
  | 'deadline'
  | 'limit'
  | 'error'
  | 'unavailable';
export type SearchSnapshot<Generation> = Readonly<{
  generation: Generation;
  query: string;
  refresh: SearchRefresh;
  available: boolean;
  rows: readonly HeadResolution[];
  hasMore: boolean;
  continuation: ChronologicalProgress;
  scopes: readonly SearchScope[];
  run: ReturnType<typeof publicRunSnapshot> | undefined;
  definitiveAbsence: false;
}>;
// A completed bounded set never proves global completeness or absence.
export function searchRefreshState(
  available: boolean,
  run: SearchSnapshot<unknown>['run'],
  scopes: readonly SearchScope[],
  cancelled: boolean,
  failed: boolean,
  capped: boolean
): SearchRefresh {
  if (!available) return 'unavailable';
  if (capped || run?.ingress.stopped) return 'limit';
  if (cancelled) return 'cancelled';
  if (failed) return 'error';
  if (run?.activeRequests) return 'refreshing';
  if (scopes.some((scope) => scope.state === 'deadline')) return 'deadline';
  if (scopes.some((scope) => scope.state !== 'eose')) return 'partial';
  if (scopes.length > 0) return 'bounded-eose';
  return run?.active ? 'idle' : 'partial';
}
