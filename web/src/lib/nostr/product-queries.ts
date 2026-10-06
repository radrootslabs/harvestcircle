import { PUBLIC_QUERY_BUDGETS } from '../config/budgets.ts';
import {
  publicHeadKey,
  publicHeadSnapshot,
  type PublicHead
} from '../catalog/heads.ts';
import type { PublicFilter } from './exports.ts';

export function headResolutionQueries(heads: readonly PublicHead[]): Readonly<{
  head: readonly PublicFilter[];
  deletion: readonly PublicFilter[];
}> {
  if (heads.length > PUBLIC_QUERY_BUDGETS.coordinatesPerRun)
    throw new Error('head_query_input_limit');
  const coordinates = new Map<string, PublicHead>(),
    authors = new Map<string, true>();
  for (const head of heads) {
    const row = publicHeadSnapshot(head);
    if (row.kind !== 30402) throw new Error('head_query_kind_invalid');
    coordinates.set(publicHeadKey(head), head);
    authors.set(row.pubkey, true);
  }
  const limit = PUBLIC_QUERY_BUDGETS.requestedPerRelay;
  return {
    head: Array.from(coordinates.values(), (head) => {
      const row = publicHeadSnapshot(head);
      // A missing/malformed first d has the generic empty coordinate. An
      // author-only bounded query discovers these, with exact reduction local.
      return row.identifier === ''
        ? { kinds: [30402], authors: [row.pubkey], limit }
        : {
            kinds: [30402],
            authors: [row.pubkey],
            '#d': [row.identifier!],
            limit
          };
    }),
    // Author lookups also cover e-only requests for a newer, initially unknown
    // head ID. Advisory k and request time cannot restrict deletion authority.
    deletion: Array.from(authors, ([author]) => ({
      kinds: [5],
      authors: [author],
      limit
    }))
  };
}
