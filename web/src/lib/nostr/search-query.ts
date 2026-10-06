import { PUBLIC_NIP50_BUDGETS } from '../config/budgets.ts';
import { normalizePublicQuery } from '../catalog/query-input.ts';
import type { PublicFilter } from './exports.ts';

// Relevance sampling supplies no timestamp cursor and no completeness claim.
export function nip50SearchQuery(input: unknown): readonly PublicFilter[] {
  const query = normalizePublicQuery(input);
  if (!query.ok) throw new Error(query.error);
  return query.text === ''
    ? []
    : [
        {
          kinds: [30402],
          search: query.text,
          limit: PUBLIC_NIP50_BUDGETS.requestedPerRelay
        }
      ];
}
