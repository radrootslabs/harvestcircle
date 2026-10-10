import { PUBLIC_QUERY_BUDGETS } from '../config/budgets.ts';
import {
  decodeProductReference,
  type ProductReference
} from '../nostr/references.ts';
import {
  beginPublicViewRun,
  disposePublicView,
  publicViewProjectionAvailable,
  publicViewRunCurrent,
  publicViewOwnsRun,
  publicViewKnownEvidence,
  subscribePublicView,
  type PublicView
} from '../runtime/public-runtime.ts';
import {
  publicRequestScopeSnapshot,
  publicRunSnapshot,
  closePublicRequest,
  disposePublicRun,
  type PublicRequest
} from '../nostr/request-scope.ts';
import {
  createPublicHeadCandidate,
  publicHeadSnapshot,
  publicHeadEnvelope,
  selectPublicHead,
  type PublicHead
} from './heads.ts';
import {
  createPublicViewHeadResolver,
  headResolutionSnapshot,
  resolveHeads,
  closeHeadResolver
} from './resolve-head.ts';
import {
  createPublisherViews,
  readPublisherPage,
  publisherSnapshot,
  type PublisherView
} from './publishers.ts';
import type { FoodProjection } from '../contracts/food-availability-v1/read.ts';
import type { VerifiedEnvelope } from '../nostr/verified-envelope.ts';
import type { WallClock } from './clock-policy.ts';
import type { ForegroundSchedule } from './search-view.ts';
export type ProductViewSnapshot = Readonly<{
  reference: ProductReference;
  outcome:
    | 'checking'
    | 'unobserved'
    | 'unavailable'
    | 'active'
    | 'sold'
    | 'withdrawn'
    | 'unsupported'
    | 'future_quarantined'
    | 'clock_unavailable';
  food: FoodProjection | undefined;
  eventId: string | undefined;
  createdAt: number | undefined;
  publisher: PublisherView;
  lastKnown: boolean;
  sources: readonly ReturnType<typeof publicRequestScopeSnapshot>[];
}>;
declare const productBrand: unique symbol;
export type ProductView = Readonly<{ readonly [productBrand]: true }>;
declare const acknowledgementBrand: unique symbol;
export type ProductStatusAcknowledgement = Readonly<{
  readonly [acknowledgementBrand]: true;
}>;
const acknowledgements = new WeakMap<
  ProductStatusAcknowledgement,
  { view: ProductView; current(this: void): boolean }
>();
interface Owner {
  snapshot: () => ProductViewSnapshot;
  listing: (
    acknowledgement?: ProductStatusAcknowledgement
  ) => ProductListingOwnership | undefined;
  acknowledge: (review: unknown) => ProductStatusAcknowledgement | undefined;
  close: () => void;
}
export type ProductListingOwnership = Readonly<{
  proof: VerifiedEnvelope;
  available: () => boolean;
  current: () => boolean;
}>;
const owners = new WeakMap<ProductView, Owner>();
function ownerOf(token: ProductView): Owner {
  const owner = owners.get(token);
  if (!owner) throw Error('product_view_invalid');
  return owner;
}
function foreground(callback: () => void, delay: number): () => void {
  const timer = setTimeout(() => {
    callback();
  }, delay);
  return () => {
    clearTimeout(timer);
  };
}
// One explicit route read, one coordinate, shared run/request budgets. No forged
// seed, independent cache, relay hints, automatic refresh or additional deadline.
export function createProductView(
  view: PublicView,
  value: unknown,
  clock: WallClock,
  onUpdate: (value: ProductViewSnapshot) => void,
  schedule: ForegroundSchedule = foreground
): ProductView {
  const decoded = decodeProductReference(value);
  if (!decoded) throw Error('product_reference_invalid');
  const reference = decoded;
  const run = beginPublicViewRun(view),
    resolver = createPublicViewHeadResolver(view, run, clock),
    publishers = createPublisherViews(view, clock);
  let closed = false,
    failed = false,
    head: PublicHead | undefined,
    cancel: (() => void) | undefined;
  const requests = new Map<PublicRequest, true>();
  const current = () => !closed && publicViewRunCurrent(view, run);
  function snapshot(): ProductViewSnapshot {
    const available =
      !closed &&
      publicViewProjectionAvailable(view) &&
      publicViewOwnsRun(view, run);
    const row = available ? headResolutionSnapshot(resolver)[0] : undefined;
    let scopes = Array.from(requests.keys(), publicRequestScopeSnapshot);
    for (const scope of [row?.headSources, row?.deletionSources])
      if (
        scope &&
        !scopes.some((s) => s.result.context === scope.result.context)
      )
        scopes = scopes.concat(scope);
    const complete =
      !failed &&
      scopes.length > 0 &&
      scopes.every(
        (s) => s.state === 'eose' && s.result.coverage === 'bounded-eose'
      );
    let outcome: ProductViewSnapshot['outcome'];
    if (!available) outcome = 'unavailable';
    else if (row?.deletion.outcome === 'suppressed') outcome = 'withdrawn';
    else if (
      row?.state.display === 'future_quarantined' ||
      row?.state.display === 'clock_unavailable'
    )
      outcome = row.state.display;
    else if (row?.state.display === 'unsupported') outcome = 'unsupported';
    else if (row?.state.food) outcome = row.state.food.status;
    else if (
      !row &&
      (failed ||
        (scopes.length > 0 &&
          scopes.every(
            (scope) =>
              scope.result.sources.length > 0 &&
              scope.result.sources.every(
                (source) =>
                  source.state === 'error' || source.state === 'closed'
              )
          )))
    )
      outcome = 'unavailable';
    else
      outcome =
        current() && scopes.some((s) => s.state === 'active')
          ? 'checking'
          : 'unobserved';
    const publisher = publisherSnapshot(publishers, reference.pubkey);
    const stillAvailable =
      !closed &&
      publicViewProjectionAvailable(view) &&
      publicViewOwnsRun(view, run);
    return {
      reference: { ...reference },
      outcome: stillAvailable ? outcome : 'unavailable',
      food: stillAvailable ? row?.state.food : undefined,
      eventId: stillAvailable ? row?.state.head.id : undefined,
      createdAt: stillAvailable ? row?.state.head.created_at : undefined,
      publisher: stillAvailable
        ? publisher
        : {
            pubkey: reference.pubkey,
            label: reference.pubkey,
            assertedName: false
          },
      lastKnown: !stillAvailable || !complete || (row?.lastKnown ?? true),
      sources: scopes
    };
  }
  function publish() {
    const value = snapshot();
    if (!closed && publicViewOwnsRun(view, run)) onUpdate(value);
  }
  function queue() {
    if (!current() || publicRunSnapshot(run).activeRequests === 0) return;
    const next = schedule(() => {
      if (closed || !publicViewOwnsRun(view, run)) return;
      cancel = undefined;
      publish();
      queue();
    }, 250);
    if (current()) cancel = next;
    else next();
  }
  const token = Object.freeze({}) as ProductView;
  function coverageKey(observed: ProductViewSnapshot) {
    return JSON.stringify({
      runCurrent: publicViewRunCurrent(view, run),
      outcome: observed.outcome,
      eventId: observed.eventId,
      lastKnown: observed.lastKnown,
      sources: observed.sources.map((scope) => ({
        context: scope.result.context,
        state: scope.state,
        coverage: scope.result.coverage,
        sources: scope.result.sources.map(({ source, state }) => ({
          source,
          state
        }))
      }))
    });
  }
  owners.set(token, {
    snapshot,
    acknowledge(review) {
      const observed = snapshot();
      if (
        review !== 'reviewed_last_known_listing' ||
        observed.outcome !== 'active' ||
        !observed.lastKnown ||
        !observed.eventId ||
        !head ||
        closed
      )
        return;
      const key = coverageKey(observed);
      const consent = Object.freeze({}) as ProductStatusAcknowledgement;
      acknowledgements.set(consent, {
        view: token,
        current: () =>
          !closed &&
          publicViewOwnsRun(view, run) &&
          publicViewProjectionAvailable(view) &&
          coverageKey(snapshot()) === key
      });
      return consent;
    },
    listing(acknowledgement) {
      const observed = snapshot();
      const capturedConsent =
        acknowledgement && acknowledgements.get(acknowledgement);
      const consent =
        capturedConsent?.view === token ? capturedConsent.current : undefined;
      if (
        observed.outcome !== 'active' ||
        (observed.lastKnown && !consent?.()) ||
        !head
      )
        return;
      const selected = publicViewKnownEvidence(view, head).head;
      const metadata = publicHeadSnapshot(selected);
      if (
        metadata.id !== observed.eventId ||
        metadata.pubkey !== reference.pubkey ||
        metadata.identifier !== reference.identifier
      )
        return;
      return {
        proof: publicHeadEnvelope(selected),
        available: () =>
          !closed &&
          publicViewOwnsRun(view, run) &&
          publicViewProjectionAvailable(view),
        current() {
          const actual = snapshot();
          return (
            !closed &&
            actual.outcome === 'active' &&
            (!actual.lastKnown || !!consent?.()) &&
            actual.eventId === metadata.id
          );
        }
      };
    },
    close() {
      const notifyTerminal = !closed;
      closed = true;
      const previous = cancel;
      cancel = undefined;
      previous?.();
      let error = false;
      try {
        closeHeadResolver(resolver);
      } catch {
        error = true;
      }
      try {
        if (publicViewOwnsRun(view, run)) disposePublicView(view);
        else disposePublicRun(run);
      } catch {
        error = true;
      }
      if (notifyTerminal)
        try {
          onUpdate(snapshot());
        } catch {
          /* Terminal observer cannot prevent original revocation/cleanup. */
        }
      if (error) throw Error('product_view_close_failed');
    }
  });
  try {
    const filter =
      reference.identifier === ''
        ? {
            kinds: [30402],
            authors: [reference.pubkey],
            limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay
          }
        : {
            kinds: [30402],
            authors: [reference.pubkey],
            '#d': [reference.identifier],
            limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay
          };
    const request = subscribePublicView(
      view,
      run,
      'head',
      [filter],
      (proof) => {
        if (!current()) return;
        const candidate = createPublicHeadCandidate(proof);
        if (!candidate) return;
        const next = publicHeadSnapshot(candidate);
        if (
          next.kind !== 30402 ||
          next.pubkey !== reference.pubkey ||
          next.identifier !== reference.identifier
        )
          return;
        head = selectPublicHead(head, candidate).head;
        try {
          resolveHeads(resolver, [head]);
          if (current())
            for (const request of readPublisherPage(publishers, run, [
              reference.pubkey
            ]))
              requests.set(request, true);
        } catch {
          failed = true;
        }
      }
    );
    requests.set(request, true);
    if (publicRequestScopeSnapshot(request).result.sources.length === 0) {
      failed = true;
      closePublicRequest(request);
    }
  } catch {
    failed = true;
  }
  publish();
  queue();
  return token;
}
export function productViewSnapshot(token: ProductView): ProductViewSnapshot {
  return ownerOf(token).snapshot();
}
export function closeProductView(token: ProductView): void {
  ownerOf(token).close();
}
// Only the original current view resolves the observed signed proof. Detached
// UI snapshots cannot create this witness or select another author/version.
export function productViewListingOwnership(
  token: ProductView,
  acknowledgement?: ProductStatusAcknowledgement
): ProductListingOwnership | undefined {
  return owners.get(token)?.listing(acknowledgement);
}
export function acknowledgeProductViewUncertainty(
  token: ProductView,
  review: unknown
): ProductStatusAcknowledgement | undefined {
  return owners.get(token)?.acknowledge(review);
}
