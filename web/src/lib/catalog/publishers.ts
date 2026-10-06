import {
  PUBLIC_QUERY_BUDGETS,
  PUBLIC_SEARCH_BUDGETS
} from '../config/budgets.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import {
  publisherProfileQuery,
  assertedProfileName
} from '../nostr/profile-query.ts';
import type { PublicRun, PublicRequest } from '../nostr/request-scope.ts';
import {
  subscribePublicView,
  publicViewProjectionAvailable,
  publicViewRunCurrent,
  publicViewKnownEvidence,
  type PublicView
} from '../runtime/public-runtime.ts';
import {
  createPublicHeadCandidate,
  publicHeadEnvelope,
  publicHeadSnapshot,
  selectPublicHead,
  type PublicHead
} from './heads.ts';
import { evaluatePublicHeadDeletion } from './deletions.ts';
import { assessFutureTimestamp, type WallClock } from './clock-policy.ts';
declare const publisherBrand: unique symbol;
export type PublisherViews = Readonly<{ readonly [publisherBrand]: true }>;
export type PublisherView = Readonly<{
  pubkey: string;
  label: string;
  assertedName: boolean;
}>;
interface Owner {
  readonly view: PublicView;
  readonly clock: WallClock;
  readonly heads: () => readonly PublicHead[];
  readonly remember: (head: PublicHead) => void;
  readonly reserve: (
    run: PublicRun,
    authors: readonly string[],
    page: number
  ) => readonly string[];
}
const owners = new WeakMap<PublisherViews, Owner>();
function ownerOf(token: PublisherViews): Owner {
  const owner = owners.get(token);
  if (!owner) throw new Error('publisher_owner_invalid');
  return owner;
}
export function createPublisherViews(
  view: PublicView,
  clock: WallClock
): PublisherViews {
  publicViewProjectionAvailable(view);
  if (typeof clock.nowSeconds !== 'function')
    throw new Error('publisher_clock_invalid');
  const heads = new Map<string, PublicHead>(),
    attempted = new WeakMap<
      PublicRun,
      Readonly<{
        seen: Map<string, true>;
        pages: Map<number, Map<string, true>>;
      }>
    >();
  const token = Object.freeze({}) as PublisherViews;
  owners.set(token, {
    view,
    clock,
    heads: () => Array.from(heads.values()),
    remember(head) {
      const key = publicHeadSnapshot(head).pubkey;
      if (
        !heads.has(key) &&
        heads.size >= PUBLIC_QUERY_BUDGETS.coordinatesPerRun
      )
        return;
      heads.set(key, selectPublicHead(heads.get(key), head).head);
    },
    reserve(run, authors, page) {
      let state = attempted.get(run);
      if (!state) {
        state = { seen: new Map(), pages: new Map() };
        attempted.set(run, state);
      }
      const seen = state.seen;
      let pageAuthors = state.pages.get(page);
      if (!pageAuthors) {
        pageAuthors = new Map();
        state.pages.set(page, pageAuthors);
      }
      const fresh = new Map<string, true>();
      for (const key of authors)
        if (!seen.has(key)) {
          if (pageAuthors.size >= PUBLIC_SEARCH_BUDGETS.publisherAuthorsPerPage)
            continue;
          if (seen.size >= PUBLIC_QUERY_BUDGETS.coordinatesPerRun)
            throw new Error('publisher_run_limit');
          pageAuthors.set(key, true);
          seen.set(key, true);
          fresh.set(key, true);
        }
      return Array.from(fresh.keys());
    }
  });
  return token;
}
// Caller selects one current local page, never every discovered publisher.
// Page reservations are cumulative across reordered rows. Admission precedes
// effects and is not refunded on failure. New explicit
// generations can refresh metadata; duplicate page reads cannot retry blindly.
export function readPublisherPage(
  token: PublisherViews,
  run: PublicRun,
  authors: readonly string[],
  page = 1
): readonly PublicRequest[] {
  if (
    !Number.isSafeInteger(page) ||
    page < 1 ||
    page >
      Math.ceil(
        PUBLIC_QUERY_BUDGETS.coordinatesPerRun / PUBLIC_SEARCH_BUDGETS.pageRows
      )
  )
    throw new Error('publisher_page_invalid');
  const filters = publisherProfileQuery(authors),
    owner = ownerOf(token);
  if (
    !publicViewProjectionAvailable(owner.view) ||
    !publicViewRunCurrent(owner.view, run)
  )
    throw new Error('publisher_run_inactive');
  const fresh = owner.reserve(run, filters[0]?.authors ?? [], page);
  if (!fresh.length) return [];
  const selected = new Map(fresh.map((key) => [key, true]));
  const request = subscribePublicView(
    owner.view,
    run,
    'profile',
    publisherProfileQuery(fresh),
    (proof) => {
      const head = createPublicHeadCandidate(proof);
      if (!head) return;
      const value = publicHeadSnapshot(head);
      if (value.kind === 0 && selected.has(value.pubkey)) owner.remember(head);
    }
  );
  return [request];
}
export function publisherSnapshot(
  token: PublisherViews,
  pubkey: string
): PublisherView {
  if (canonicalPublicKey(pubkey) === undefined)
    throw new Error('publisher_key_invalid');
  const owner = ownerOf(token),
    fallback = { pubkey, label: pubkey, assertedName: false };
  if (!publicViewProjectionAvailable(owner.view)) return fallback;
  const retained = owner
    .heads()
    .find((head) => publicHeadSnapshot(head).pubkey === pubkey);
  if (!retained) return fallback;
  const known = publicViewKnownEvidence(owner.view, retained),
    head = known.head ?? retained;
  owner.remember(head);
  let now: number;
  try {
    now = owner.clock.nowSeconds();
  } catch {
    return fallback;
  }
  const settled = publicViewKnownEvidence(owner.view, head);
  if (
    settled.head &&
    publicHeadSnapshot(settled.head).id !== publicHeadSnapshot(head).id
  )
    return fallback;
  if (
    !publicViewProjectionAvailable(owner.view) ||
    assessFutureTimestamp(publicHeadSnapshot(head).created_at, now) !==
      'within_policy' ||
    evaluatePublicHeadDeletion(head, settled.requests).outcome === 'suppressed'
  )
    return fallback;
  const name = assertedProfileName(publicHeadEnvelope(head));
  return name === undefined
    ? fallback
    : { pubkey, label: name, assertedName: true };
}
