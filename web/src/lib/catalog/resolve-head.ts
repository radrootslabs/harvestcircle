import { PUBLIC_QUERY_BUDGETS } from '../config/budgets.ts';
import type { PublicFilter } from '../nostr/exports.ts';
import { headResolutionQueries } from '../nostr/product-queries.ts';
import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../nostr/verified-envelope.ts';
import {
  admitDeletionRequest,
  deletionRequestEnvelope,
  deletionRequestSnapshot,
  type DeletionRequest
} from '../nostr/deletion-adapter.ts';
import {
  closePublicRequest,
  publicRequestScopeSnapshot,
  publicRunObservations,
  publicRunSnapshot,
  type PublicRun,
  type PublicRequest
} from '../nostr/request-scope.ts';
import {
  subscribePublicView,
  publicViewRunCurrent,
  type PublicView
} from '../runtime/public-runtime.ts';
import {
  createPublicHeadCandidate,
  publicHeadKey,
  publicHeadSnapshot,
  publicHeadEnvelope,
  type PublicHead
} from './heads.ts';
import {
  createFoodHeadState,
  advanceFoodHeadState,
  refreshFoodHeadState,
  foodHeadStateHead,
  foodHeadStateSnapshot,
  type FoodHeadState
} from './head-state.ts';
import { evaluatePublicHeadDeletion } from './deletions.ts';
import type { WallClock } from './clock-policy.ts';

declare const resolverBrand: unique symbol;
export type HeadResolver = Readonly<{ readonly [resolverBrand]: true }>;
// Trusted logical request factory, like openPublicRequest's transport factory.
// Production binds this to the view's genuine shared run and anonymous pool.
export type HeadSubscriber = (
  kind: 'head' | 'deletion',
  filters: readonly PublicFilter[],
  onVerified: (proof: VerifiedEnvelope) => void
) => PublicRequest;
type ScopeSnapshot = ReturnType<typeof publicRequestScopeSnapshot>;
interface Group {
  readonly head: () => ScopeSnapshot | undefined;
  readonly deletion: () => ScopeSnapshot | undefined;
  readonly complete: () => boolean;
  readonly open: () => void;
  readonly close: () => void;
}
interface Entry {
  readonly state: FoodHeadState;
  readonly group: Group;
}
interface Owner {
  readonly resolve: (heads: readonly PublicHead[]) => void;
  readonly snapshot: () => readonly HeadResolution[];
  readonly close: () => void;
}
export type HeadResolution = Readonly<{
  key: string;
  state: ReturnType<typeof foodHeadStateSnapshot>;
  deletion: ReturnType<typeof evaluatePublicHeadDeletion>;
  deletionProofs: readonly VerifiedEnvelope[];
  headSources: ScopeSnapshot | undefined;
  deletionSources: ScopeSnapshot | undefined;
  coverage: 'bounded-eose' | 'partial';
  lastKnown: boolean;
  definitiveAbsence: false;
}>;
const owners = new WeakMap<HeadResolver, Owner>();
function ownerOf(resolver: HeadResolver): Owner {
  const owner = owners.get(resolver);
  if (!owner) throw new Error('head_resolver_invalid');
  return owner;
}
function emptyRows<T>(): readonly T[] {
  return [];
}
export function createHeadResolver(
  run: PublicRun,
  subscribe: HeadSubscriber,
  clock: WallClock
): HeadResolver {
  publicRunSnapshot(run);
  const readClock = clock.nowSeconds;
  if (typeof readClock !== 'function')
    throw new Error('head_resolver_clock_invalid');
  const token = Object.freeze({}) as HeadResolver;
  const entries = new Map<string, Entry>(),
    evidence = new Map<string, DeletionRequest>();
  let groups = emptyRows<Group>();
  let closed = false;
  const active = () => !closed && publicRunSnapshot(run).active;
  function groupFor(heads: readonly PublicHead[]): Group {
    const queries = headResolutionQueries(heads),
      keys = new Map(heads.map((h) => [publicHeadKey(h), true]));
    const authors = new Map(
      heads.map((h) => [publicHeadSnapshot(h).pubkey, true])
    );
    let headRequest: PublicRequest | undefined,
      deletionRequest: PublicRequest | undefined;
    let incomplete = false;
    const snapshot = (request: PublicRequest | undefined) =>
      request === undefined ? undefined : publicRequestScopeSnapshot(request);
    const stop = () => {
      // Independent cleanup: failure of one control cannot strand the other.
      let failed = false;
      for (const request of [headRequest, deletionRequest]) {
        if (request === undefined) continue;
        try {
          closePublicRequest(request);
        } catch {
          failed = true;
        }
      }
      if (failed) throw new Error('head_resolver_close_failed');
    };
    const checked = (
      kind: 'head' | 'deletion',
      filters: readonly PublicFilter[],
      next: (proof: VerifiedEnvelope) => void
    ) => {
      const request = subscribe(kind, filters, (proof) => {
        // The request owner has already admitted this callback, including the
        // final valid delivery that exhausts ingress before request teardown.
        if (!closed) next(proof);
      });
      try {
        publicRunObservations(run, request);
        if (publicRequestScopeSnapshot(request).kind !== kind)
          throw new Error('head_resolver_request_kind');
      } catch {
        closePublicRequest(request);
        throw new Error('head_resolver_request_owner');
      }
      return request;
    };
    return {
      head: () => snapshot(headRequest),
      deletion: () => snapshot(deletionRequest),
      complete: () =>
        !incomplete &&
        snapshot(headRequest)?.state === 'eose' &&
        snapshot(deletionRequest)?.state === 'eose',
      close: () => {
        incomplete = true;
        stop();
      },
      open() {
        try {
          headRequest = checked('head', queries.head, (proof) => {
            const candidate = createPublicHeadCandidate(proof);
            const key =
              candidate === undefined ? undefined : publicHeadKey(candidate);
            if (key === undefined || !keys.has(key)) {
              incomplete = true;
              return;
            }
            const entry = entries.get(key);
            if (entry) advanceFoodHeadState(entry.state, proof);
          });
          if (!active()) {
            incomplete = true;
            stop();
            return;
          }
          deletionRequest = checked('deletion', queries.deletion, (proof) => {
            const event = verifiedEnvelopeSnapshot(proof);
            if (!event || !authors.has(event.pubkey)) {
              incomplete = true;
              return;
            }
            const request = admitDeletionRequest(proof);
            if (!request.ok) {
              incomplete = true;
              return;
            }
            const row = deletionRequestSnapshot(request.value);
            if (row) evidence.set(row.id, request.value);
          });
          if (!active()) {
            incomplete = true;
            stop();
          }
        } catch {
          incomplete = true;
          stop();
        }
      }
    };
  }
  owners.set(token, {
    resolve(heads) {
      if (!active()) throw new Error('head_resolver_inactive');
      // Validate the complete batch and total coordinate admission before any
      // external clock/request callback. Repeated coordinates never reset work.
      headResolutionQueries(heads);
      const unique = new Map<string, PublicHead>();
      for (const head of heads) unique.set(publicHeadKey(head), head);
      const fresh = Array.from(unique.values()).filter(
        (h) => !entries.has(publicHeadKey(h))
      );
      if (entries.size + fresh.length > PUBLIC_QUERY_BUDGETS.coordinatesPerRun)
        throw new Error('head_resolver_coordinate_limit');
      const group = fresh.length === 0 ? undefined : groupFor(fresh);
      if (group) groups = groups.concat(group);
      // Reserve every coordinate before caller-controlled clocks or openers.
      let prepared = false;
      let created = emptyRows<FoodHeadState>();
      for (const head of fresh) {
        const state = createFoodHeadState(head, {
          nowSeconds: () => (prepared ? readClock() : NaN)
        });
        entries.set(publicHeadKey(head), { state, group: group! });
        created = created.concat(state);
      }
      prepared = true;
      for (const state of created) refreshFoodHeadState(state);
      for (const head of heads) {
        const entry = entries.get(publicHeadKey(head));
        if (entry) advanceFoodHeadState(entry.state, publicHeadEnvelope(head));
      }
      if (group && active()) group.open();
    },
    snapshot() {
      const available = active();
      return Array.from(entries, ([key, entry]) => {
        const deletion = evaluatePublicHeadDeletion(
          foodHeadStateHead(entry.state),
          Array.from(evidence.values())
        );
        const state = foodHeadStateSnapshot(entry.state);
        const complete = available && entry.group.complete();
        return {
          key,
          state: {
            ...state,
            food: deletion.outcome === 'suppressed' ? undefined : state.food
          },
          deletion,
          deletionProofs: Array.from(evidence.values()).flatMap((r) => {
            const proof = deletionRequestEnvelope(r);
            return proof === undefined ? [] : [proof];
          }),
          headSources: entry.group.head(),
          deletionSources: entry.group.deletion(),
          coverage: complete ? 'bounded-eose' : 'partial',
          lastKnown: !complete,
          definitiveAbsence: false
        };
      });
    },
    close() {
      closed = true;
      let failed = false;
      for (const group of groups) {
        try {
          group.close();
        } catch {
          failed = true;
        }
      }
      if (failed) throw new Error('head_resolver_close_failed');
    }
  });
  return token;
}
const viewResolvers = new WeakMap<
  PublicRun,
  Readonly<{ view: PublicView; resolver: HeadResolver }>
>();
export function createPublicViewHeadResolver(
  view: PublicView,
  run: PublicRun,
  clock: WallClock
): HeadResolver {
  if (!publicViewRunCurrent(view, run))
    throw new Error('head_resolver_view_inactive');
  const current = viewResolvers.get(run);
  if (current) {
    if (current.view !== view) throw new Error('head_resolver_view_mismatch');
    return current.resolver;
  }
  const resolver = createHeadResolver(
    run,
    (kind, filters, next) =>
      subscribePublicView(view, run, kind, filters, next),
    clock
  );
  viewResolvers.set(run, { view, resolver });
  return resolver;
}
export function resolveHeads(
  resolver: HeadResolver,
  heads: readonly PublicHead[]
): void {
  ownerOf(resolver).resolve(heads);
}
export function headResolutionSnapshot(
  resolver: HeadResolver
): readonly HeadResolution[] {
  return ownerOf(resolver).snapshot();
}
export function closeHeadResolver(resolver: HeadResolver): void {
  ownerOf(resolver).close();
}
