import { normalizePublicQuery, type PublicQuery } from './query-input.ts';
import {
  headResolutionSnapshot,
  headResolverRetentionState,
  type HeadResolver,
  type HeadResolution
} from './resolve-head.ts';

export type FoodSearch =
  | Exclude<PublicQuery, { ok: true }>
  | Readonly<{
      ok: true;
      query: string;
      available: boolean;
      rows: readonly HeadResolution[];
      definitiveAbsence: false;
    }>;
// Matching follows genuine generic head/deletion/display admission. Partial
// source lookup may still yield a retained last-known row, never assured stock.
export function searchResolvedFood(
  resolver: HeadResolver,
  input: unknown
): FoodSearch {
  const query = normalizePublicQuery(input);
  if (!query.ok) return query;
  const rows = headResolutionSnapshot(resolver).filter((row) => {
    const food = row.state.food;
    if (!food || food.status !== 'active') return false;
    // Search copies only. No identifier, key, price, image URL or private text.
    const text = [food.title, food.summary, food.content, food.location]
      .map((field) => field.normalize('NFKC').toLowerCase())
      .join(' ');
    return query.terms.every((term) => text.includes(term));
  });
  const available = headResolverRetentionState(resolver).available;
  return {
    ok: true,
    query: query.text,
    available,
    rows: available ? rows : [],
    definitiveAbsence: false
  };
}
