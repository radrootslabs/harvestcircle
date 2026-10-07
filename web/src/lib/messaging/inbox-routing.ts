import { canonicalPublicKey } from '../contracts/public-key.ts';
import {
  inboxRelayTargets,
  publicRelayTargets,
  type RelayPolicy
} from '../config/relays.ts';
import { inboxPreferenceSnapshot } from '../nostr/inbox-preferences.ts';
import {
  inboxResolutionSnapshot,
  inboxResolverCurrentAfterSample,
  type InboxResolver
} from './resolve-inbox.ts';

declare const routeBrand: unique symbol;
export type InboxRoutePlan = Readonly<{ readonly [routeBrand]: true }>;
type Role = 'peer' | 'self_archive';
type Route = Readonly<{
  role: Role;
  author: string;
  targets: string[];
  knownBase: Readonly<{ author: string; id: string; createdAt: number }>;
  sources: {
    source: string;
    state: string;
    head?: Readonly<{ id: string; createdAt: number }>;
  }[];
}>;
export type InboxRouteSnapshot = Readonly<{ peer: Route; archive: Route }>;
// Only admitted snapshots of genuine discovery and policy owners enter this map.
// The plan is preparation evidence, never private-effect or access permission.
const plans = new WeakMap<InboxRoutePlan, string>();
function route(
  policy: RelayPolicy,
  author: string,
  resolver: InboxResolver,
  role: Role
): Route | undefined {
  const resolution = inboxResolutionSnapshot(resolver);
  const origins = publicRelayTargets(policy, 'read');
  if (
    resolution.author !== author ||
    resolution.status !== 'ready' ||
    resolution.coverage !== 'bounded-eose' ||
    !resolution.head ||
    !resolution.knownBase ||
    origins.length === 0 ||
    resolution.sources.length !== origins.length ||
    !origins.every((origin) =>
      resolution.sources.some(
        (row) => row.source === origin && row.state === 'eose'
      )
    )
  )
    return undefined;
  const preference = inboxPreferenceSnapshot(resolution.head);
  if (
    !preference ||
    preference.status !== 'supported' ||
    preference.author !== author ||
    preference.id !== resolution.knownBase.id ||
    preference.createdAt !== resolution.knownBase.createdAt
  )
    return undefined;
  const targets = inboxRelayTargets(policy, preference.relays, 'write').slice();
  if (targets.length === 0) return undefined;
  return {
    role,
    author,
    targets,
    knownBase: { ...resolution.knownBase },
    sources: resolution.sources.map((row) => ({
      source: row.source,
      state: row.state,
      ...(row.head ? { head: { ...row.head } } : {})
    }))
  };
}
function capture(
  policy: RelayPolicy,
  sender: unknown,
  peer: unknown,
  own: InboxResolver,
  other: InboxResolver
): InboxRouteSnapshot | undefined {
  const senderKey = canonicalPublicKey(sender),
    peerKey = canonicalPublicKey(peer);
  if (!senderKey || !peerKey || senderKey === peerKey) return undefined;
  try {
    const archive = route(policy, senderKey, own, 'self_archive'),
      recipient = route(policy, peerKey, other, 'peer');
    if (
      !archive ||
      !recipient ||
      !inboxResolverCurrentAfterSample(own) ||
      !inboxResolverCurrentAfterSample(other)
    )
      return undefined;
    return { peer: recipient, archive };
  } catch {
    return undefined;
  }
}
export function createInboxRoutePlan(
  policy: RelayPolicy,
  sender: unknown,
  peer: unknown,
  own: InboxResolver,
  other: InboxResolver
): InboxRoutePlan | undefined {
  const snapshot = capture(policy, sender, peer, own, other);
  if (!snapshot) return undefined;
  const token = Object.freeze({}) as InboxRoutePlan;
  plans.set(token, JSON.stringify(snapshot));
  return token;
}
export function inboxRoutePlanSnapshot(
  token: InboxRoutePlan
): InboxRouteSnapshot | undefined {
  const saved = plans.get(token);
  return saved ? (JSON.parse(saved) as InboxRouteSnapshot) : undefined;
}
// Any changed known head, named source version or destination requires explicit
// later review. The original plan remains immutable; no replay or rerouting.
export function recheckInboxRoutePlan(
  token: InboxRoutePlan,
  policy: RelayPolicy,
  own: InboxResolver,
  other: InboxResolver
): 'unchanged' | 'review_required' | 'unavailable' {
  const previous = inboxRoutePlanSnapshot(token);
  if (!previous) return 'unavailable';
  const current = capture(
    policy,
    previous.archive.author,
    previous.peer.author,
    own,
    other
  );
  if (!current) return 'unavailable';
  return JSON.stringify(current) === plans.get(token)
    ? 'unchanged'
    : 'review_required';
}
