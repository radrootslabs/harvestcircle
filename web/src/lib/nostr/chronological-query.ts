import { PUBLIC_QUERY_BUDGETS } from '../config/budgets.ts';
import type { PublicFilter } from './exports.ts';

// NIP-01 until is inclusive. Never invent a cursor by subtracting a second.
export function chronologicalSearchQuery(
  until?: number
): readonly PublicFilter[] {
  if (until !== undefined && (!Number.isSafeInteger(until) || until < 0))
    throw new Error('chronological_boundary_invalid');
  return [
    {
      kinds: [30402],
      limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay,
      ...(until === undefined ? {} : { until })
    }
  ];
}
// The coordinator accepts only its own observed boundary, never caller offsets
// or a keyword-constrained chronological request. Return an independent copy.
export function requireChronologicalSearchQuery(
  filters: readonly PublicFilter[],
  until?: number
): readonly PublicFilter[] {
  const expected = chronologicalSearchQuery(until),
    filter = filters[0];
  if (
    !Array.isArray(filters) ||
    filters.length !== 1 ||
    !filter ||
    'search' in filter ||
    'q' in filter ||
    'since' in filter ||
    'authors' in filter ||
    'ids' in filter ||
    !Array.isArray(filter.kinds) ||
    filter.kinds.length !== 1 ||
    filter.kinds[0] !== 30402 ||
    filter.limit !== PUBLIC_QUERY_BUDGETS.requestedPerRelay ||
    filter.until !== until
  )
    throw new Error('food_search_chronological_filter');
  return expected;
}
