import { getProfileContent } from 'applesauce-core/helpers';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import {
  PUBLIC_QUERY_BUDGETS,
  PUBLIC_SEARCH_BUDGETS
} from '../config/budgets.ts';
import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from './verified-envelope.ts';
import type { PublicFilter } from './exports.ts';
export function publisherProfileQuery(
  authors: readonly string[]
): readonly PublicFilter[] {
  if (authors.length > PUBLIC_SEARCH_BUDGETS.publisherAuthorsPerPage)
    throw new Error('publisher_page_limit');
  const unique = new Map<string, true>();
  for (const author of authors) {
    if (canonicalPublicKey(author) === undefined)
      throw new Error('publisher_key_invalid');
    unique.set(author, true);
  }
  return unique.size
    ? [
        {
          kinds: [0],
          authors: Array.from(unique.keys()),
          limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay
        }
      ]
    : [];
}
// Parse a fresh genuine envelope through the qualified generic SDK. Cached SDK
// symbols, profile URLs and claimed identity fields never become authority.
export function assertedProfileName(
  proof: VerifiedEnvelope
): string | undefined {
  const event = verifiedEnvelopeSnapshot(proof);
  if (!event || event.kind !== 0) return undefined;
  try {
    const profile = getProfileContent(event);
    if (!profile || typeof profile !== 'object' || Array.isArray(profile))
      return undefined;
    for (const value of [
      profile.display_name,
      profile.displayName,
      profile.name
    ])
      if (
        typeof value === 'string' &&
        value.trim() !== '' &&
        value.isWellFormed()
      )
        return value;
  } catch {
    /* Unsupported metadata leaves the selected head at key fallback. */
  }
  return undefined;
}
