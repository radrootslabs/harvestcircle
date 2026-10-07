import { canonicalPublicKey } from '../contracts/public-key.ts';
import { PUBLIC_QUERY_BUDGETS } from '../config/budgets.ts';
import type { PublicFilter } from './exports.ts';

export function inboxPreferenceQueries(
  author: unknown
): readonly PublicFilter[] {
  const key = canonicalPublicKey(author);
  if (!key) throw new Error('inbox_author_invalid');
  return [
    {
      kinds: [10050],
      authors: [key],
      limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay
    }
  ];
}
