import { canonicalPublicKey } from '../contracts/public-key.ts';
import {
  verifiedEnvelopeWire,
  type VerifiedEnvelope
} from '../nostr/verified-envelope.ts';
import {
  readInboxPreference,
  inboxPreferenceSnapshot,
  type InboxPreference
} from '../nostr/inbox-preferences.ts';
import {
  closePublicRequest,
  publicRequestScopeSnapshot,
  publicRunObservations,
  publicRunSnapshot,
  type PublicRun,
  type PublicRequest
} from '../nostr/request-scope.ts';
import {
  subscribeInboxPreference,
  publicViewOwnsRun,
  publicViewRunCurrent,
  type PublicView
} from '../runtime/public-runtime.ts';

declare const resolverBrand: unique symbol;
export type InboxResolver = Readonly<{ readonly [resolverBrand]: true }>;
export type InboxResolution = Readonly<{
  author: string;
  status: 'ready' | 'missing' | 'unsupported' | 'inconclusive';
  head: InboxPreference | undefined;
  knownBase:
    Readonly<{ author: string; id: string; createdAt: number }> | undefined;
  request: PublicRequest;
  sources: (ReturnType<
    typeof publicRequestScopeSnapshot
  >['result']['sources'][number] & {
    head?: Readonly<{ id: string; createdAt: number }>;
  })[];
  coverage: 'bounded-eose' | 'partial';
  definitiveAbsence: false;
  wireProvenance: 'decoded-sdk-json';
}>;
interface Owner {
  readonly snapshot: () => InboxResolution;
  readonly close: () => void;
}
const owners = new WeakMap<InboxResolver, Owner>();
function ownerOf(token: InboxResolver): Owner {
  const owner = owners.get(token);
  if (!owner) throw new Error('inbox_resolver_invalid');
  return owner;
}
// Trusted request factory, like catalog resolvers. Production binds this to the
// current view and real anonymous pool. UI cannot manufacture request/proof tokens.
export function createInboxResolver(
  run: PublicRun,
  author: unknown,
  subscribe: (onVerified: (event: VerifiedEnvelope) => void) => PublicRequest,
  current: () => boolean = () => true
): InboxResolver {
  const key = canonicalPublicKey(author);
  if (!key) throw new Error('inbox_author_invalid');
  if (!publicRunSnapshot(run).active || !current())
    throw new Error('inbox_resolver_inactive');
  let head: InboxPreference | undefined,
    closed = false,
    rejected = false;
  const request = subscribe((proof) => {
    if (closed || !current()) return;
    const result = readInboxPreference(verifiedEnvelopeWire(proof), key);
    if (result.status !== 'supported' && result.status !== 'unsupported') {
      rejected = true;
      return;
    }
    const candidate = inboxPreferenceSnapshot(result.value)!,
      previous = head && inboxPreferenceSnapshot(head);
    if (
      !previous ||
      candidate.createdAt > previous.createdAt ||
      (candidate.createdAt === previous.createdAt && candidate.id < previous.id)
    )
      head = result.value;
  });
  try {
    publicRunObservations(run, request);
    const snapshot = publicRequestScopeSnapshot(request);
    if (snapshot.kind !== 'inbox' || snapshot.result.inbox?.author !== key)
      throw new Error('inbox_resolver_request_owner');
  } catch {
    closePublicRequest(request);
    throw new Error('inbox_resolver_request_owner');
  }
  const token = Object.freeze({}) as InboxResolver;
  owners.set(token, {
    snapshot() {
      // Sample the external clock first, then recheck generation ownership.
      // Clock reentry may supersede this run before it returns an active value.
      const active = publicRunSnapshot(run).active;
      const available = !closed && current() && active;
      const scope = publicRequestScopeSnapshot(request),
        saved = head && inboxPreferenceSnapshot(head);
      const complete =
        available &&
        !rejected &&
        scope.state === 'eose' &&
        scope.inboxSettled === true &&
        scope.result.coverage === 'bounded-eose';
      const status = !available
        ? 'inconclusive'
        : saved?.status === 'unsupported'
          ? 'unsupported'
          : !complete
            ? 'inconclusive'
            : saved
              ? 'ready'
              : 'missing';
      return {
        author: key,
        status,
        head,
        knownBase: saved && {
          author: key,
          id: saved.id,
          createdAt: saved.createdAt
        },
        request,
        sources: scope.result.sources.map((row) => {
          const sourceHead = scope.result.inbox?.heads.find(
            (h) => h.source === row.source
          );
          return {
            ...row,
            ...(sourceHead
              ? { head: { id: sourceHead.id, createdAt: sourceHead.createdAt } }
              : {})
          };
        }),
        coverage: complete ? 'bounded-eose' : 'partial',
        definitiveAbsence: false,
        wireProvenance: 'decoded-sdk-json'
      };
    },
    close() {
      closed = true;
      closePublicRequest(request);
    }
  });
  return token;
}
// One recipient resolver per run prevents repeated lookups from resetting work.
const viewResolvers = new WeakMap<
  PublicRun,
  Readonly<{ view: PublicView; author: string; resolver?: InboxResolver }>
>();
export function resolveInboxPreference(
  view: PublicView,
  run: PublicRun,
  author: unknown
): InboxResolver {
  const key = canonicalPublicKey(author);
  if (!key) throw new Error('inbox_author_invalid');
  const prior = viewResolvers.get(run);
  if (prior) {
    if (prior.view !== view || prior.author !== key)
      throw new Error('inbox_resolver_owner_changed');
    if (!prior.resolver) throw new Error('inbox_resolver_opening');
    return prior.resolver;
  }
  // Reserve before clock/subscription acquisition can synchronously reenter.
  viewResolvers.set(run, { view, author: key });
  try {
    if (!publicViewRunCurrent(view, run))
      throw new Error('inbox_resolver_inactive');
    const resolver = createInboxResolver(
      run,
      key,
      (next) => subscribeInboxPreference(view, run, key, next),
      () => publicViewOwnsRun(view, run)
    );
    viewResolvers.set(run, { view, author: key, resolver });
    return resolver;
  } catch {
    viewResolvers.delete(run);
    throw new Error('inbox_resolver_open_failed');
  }
}
export function inboxResolutionSnapshot(token: InboxResolver): InboxResolution {
  return ownerOf(token).snapshot();
}
export function closeInboxResolver(token: InboxResolver): void {
  ownerOf(token).close();
}
