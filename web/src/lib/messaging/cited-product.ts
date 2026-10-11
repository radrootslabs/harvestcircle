import { includedProductVersion } from '../nostr/enquiry-references.ts';
import { PUBLIC_QUERY_BUDGETS } from '../config/budgets.ts';
import { decodeProductReference } from '../nostr/references.ts';
import {
  inboxMessageSnapshot,
  inboxReplyOwnership,
  inboxThreadSnapshot,
  subscribeInboxView,
  type InboxReplyOwnership,
  type InboxView
} from './inbox-view.ts';
import {
  createPublicView,
  beginPublicViewRun,
  disposePublicView,
  publicViewRunCurrent,
  publicViewOwnsRun,
  publicViewProjectionAvailable,
  subscribePublicView,
  type PublicView,
  type PublicRuntime
} from '../runtime/public-runtime.ts';
import {
  publicRequestScopeSnapshot,
  type PublicRequest
} from '../nostr/request-scope.ts';
import {
  createPublicHeadCandidate,
  publicHeadSnapshot,
  selectPublicHead,
  type PublicHead
} from '../catalog/heads.ts';
import {
  createPublicViewHeadResolver,
  headResolutionSnapshot,
  resolveHeads,
  closeHeadResolver
} from '../catalog/resolve-head.ts';
import { subscribePrivateSessionClose } from '../runtime/private-session.ts';
import { subscribeIdentityInvalidation } from '../runtime/identity-session.ts';
import type { FoodProjection } from '../contracts/food-availability-v1/read.ts';

declare const citedBrand: unique symbol;
export type CitedProduct = Readonly<{ [citedBrand]: true }>;
type Snapshot = Readonly<{
  outcome: 'checking' | 'unresolved' | 'active' | 'sold' | 'withdrawn';
  food?: FoodProjection;
  lastKnownFood?: FoodProjection;
  includedVersion: 'none' | 'unverified' | 'matched' | 'different';
}>;
const owners = new WeakMap<
  CitedProduct,
  { snapshot(): Snapshot | undefined; close(): void }
>();

export function conversationOptionsSnapshot(
  view: InboxView,
  id: string
): Readonly<{ peer: string }> | undefined {
  const thread = inboxThreadSnapshot(view, id);
  for (const row of thread.messages) {
    const original = inboxReplyOwnership(view, id, row.data.rumorId);
    if (original?.current()) return { peer: original.peer };
  }
  return undefined;
}
export function requestConversationOption(
  view: InboxView,
  id: string,
  action: 'block' | 'report',
  invoke: (peer: string) => void,
  review: unknown
): boolean {
  if (
    review !== 'reviewed_conversation_option' ||
    (action !== 'block' && action !== 'report')
  )
    return false;
  const actual = conversationOptionsSnapshot(view, id);
  if (!actual) return false;
  invoke(actual.peer);
  return true;
}

// Neither a copied row nor a body pointer is authority. Resolve the selector
// against the current original private room before allocating an anonymous view.
export function captureCitedProduct(
  inbox: InboxView,
  id: string,
  rumor: string,
  value: unknown,
  runtime: PublicRuntime,
  review: unknown
): CitedProduct | undefined {
  if (typeof window === 'undefined' || review !== 'reviewed_product_citation')
    return undefined;
  const original = inboxReplyOwnership(inbox, id, rumor),
    message = inboxMessageSnapshot(inbox, id, rumor),
    decoded = decodeProductReference(value);
  if (
    !original?.current() ||
    !message ||
    !decoded ||
    !message.citations.some((row) => row.naddr === decoded.naddr) ||
    ![original.owner, original.peer].includes(decoded.pubkey)
  )
    return undefined;
  const reference = decoded;
  const privateIds = inboxThreadSnapshot(inbox, id).messages.flatMap((row) => [
    row.data.rumorId,
    ...(row.data.replyTo ? [row.data.replyTo] : [])
  ]);
  if (privateIds.includes(reference.identifier)) return undefined;
  let versions = Array.from<string>([]);
  for (const match of message.data.content.matchAll(
    /\b(nevent1[023456789acdefghjklmnpqrstuvwxyz]+)\b/gi
  )) {
    const version = includedProductVersion(match[1], reference.pubkey);
    if (version && !privateIds.includes(version) && !versions.includes(version))
      versions = versions.concat(version);
  }
  const exactId = versions.length === 1 ? versions[0] : undefined;
  let view: PublicView;
  try {
    view = createPublicView(runtime);
  } catch {
    return undefined;
  }
  const run = beginPublicViewRun(view),
    resolver = createPublicViewHeadResolver(view, run, {
      nowSeconds: () => Math.floor(Date.now() / 1000)
    });
  let closed = false,
    failed = false,
    head: PublicHead | undefined,
    exact: string | undefined;
  let privateScope: InboxReplyOwnership | undefined = original;
  const requests = new Map<PublicRequest, true>();
  let offPrivate = () => {},
    offInbox = () => {},
    offIdentity = () => {};
  function close() {
    if (closed) return;
    closed = true;
    privateScope = undefined;
    offPrivate();
    offInbox();
    offIdentity();
    try {
      closeHeadResolver(resolver);
    } finally {
      disposePublicView(view);
    }
  }
  const owned = () =>
    !closed &&
    !!privateScope?.current() &&
    !!inboxMessageSnapshot(inbox, id, rumor) &&
    publicViewProjectionAvailable(view) &&
    publicViewOwnsRun(view, run);
  const current = () => owned() && publicViewRunCurrent(view, run);
  const token = Object.freeze({}) as CitedProduct;
  owners.set(token, {
    close,
    snapshot() {
      if (!owned()) {
        close();
        return undefined;
      }
      const row = headResolutionSnapshot(resolver)[0],
        scopes = Array.from(requests.keys(), publicRequestScopeSnapshot);
      const pending =
        current() &&
        [
          ...scopes,
          ...(row?.headSources ? [row.headSources] : []),
          ...(row?.deletionSources ? [row.deletionSources] : [])
        ].some((scope) => scope.state === 'active');
      const suppressed = row?.deletion.outcome === 'suppressed';
      // A retained signed head is not a completed coordinate/withdrawal read.
      // Original source coverage must remain honest after ordinary run expiry.
      const complete =
        row?.coverage === 'bounded-eose' &&
        !row.lastKnown &&
        scopes.length > 0 &&
        scopes.every(
          (scope) =>
            scope.state === 'eose' && scope.result.coverage === 'bounded-eose'
        );
      const food =
        !suppressed && !pending && !failed && complete
          ? row?.state.food
          : undefined;
      return {
        outcome: suppressed
          ? 'withdrawn'
          : pending
            ? 'checking'
            : food
              ? food.status
              : 'unresolved',
        food,
        lastKnownFood:
          !suppressed && !pending && !food ? row?.state.food : undefined,
        includedVersion: !exactId
          ? 'none'
          : !exact || !food
            ? 'unverified'
            : exact === row?.state.head.id
              ? 'matched'
              : 'different'
      };
    }
  });
  offPrivate = subscribePrivateSessionClose(original.session, close);
  offIdentity = subscribeIdentityInvalidation(original.identity, close);
  offInbox = subscribeInboxView(inbox, () => {
    if (!owned()) close();
  });
  function admit(
    proof: Parameters<typeof createPublicHeadCandidate>[0],
    exactOnly: boolean
  ) {
    if (!current()) return;
    const candidate = createPublicHeadCandidate(proof);
    if (!candidate) return;
    const data = publicHeadSnapshot(candidate);
    if (
      data.kind !== 30402 ||
      data.pubkey !== reference.pubkey ||
      data.identifier !== reference.identifier
    )
      return;
    if (exactOnly) {
      if (data.id === exactId) exact = data.id;
      return;
    }
    head = selectPublicHead(head, candidate).head;
    try {
      resolveHeads(resolver, [head]);
    } catch {
      failed = true;
    }
  }
  try {
    const request = subscribePublicView(
      view,
      run,
      'head',
      [
        {
          kinds: [30402],
          authors: [reference.pubkey],
          '#d': [reference.identifier],
          limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay
        }
      ],
      (proof) => admit(proof, false)
    );
    requests.set(request, true);
    if (exactId && current())
      requests.set(
        subscribePublicView(
          view,
          run,
          'head',
          [
            {
              kinds: [30402],
              authors: [reference.pubkey],
              ids: [exactId],
              limit: PUBLIC_QUERY_BUDGETS.requestedPerRelay
            }
          ],
          (proof) => admit(proof, true)
        ),
        true
      );
  } catch {
    failed = true;
  }
  return token;
}
export function citedProductSnapshot(
  token: CitedProduct
): Snapshot | undefined {
  return owners.get(token)?.snapshot();
}
export function closeCitedProduct(token: CitedProduct): void {
  owners.get(token)?.close();
}
