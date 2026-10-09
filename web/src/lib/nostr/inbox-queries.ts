import { canonicalPublicKey } from '../contracts/public-key.ts';
import {
  PRIVATE_TRANSPORT_BUDGETS,
  PUBLIC_QUERY_BUDGETS
} from '../config/budgets.ts';
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

// No inner-time cursor or throttle; live limit0 excludes initial history, not
// future backdated messages. Actual relay semantics require separate Q.
export function privateInboxQueries(
  owner: unknown,
  mode: 'live' | 'backfill'
): readonly PublicFilter[] {
  const key = canonicalPublicKey(owner);
  if (!key) throw Error('inbox_owner_invalid');
  if (mode !== 'live' && mode !== 'backfill') throw Error('inbox_mode_invalid');
  return [
    {
      kinds: [1059],
      '#p': [key],
      limit: mode === 'live' ? 0 : PRIVATE_TRANSPORT_BUDGETS.requestedPerRelay
    }
  ];
}
