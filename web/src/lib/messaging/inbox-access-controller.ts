import {
  inboxRelayTargets,
  publicRelayTargets,
  readRelayPolicy,
  type RelayPolicy
} from '../config/relays.ts';
import { inboxPreferenceSnapshot } from '../nostr/inbox-preferences.ts';
import {
  beginInboxAuthentication,
  respondInboxAuthentication,
  closeInboxAuthentication,
  inboxAuthSnapshot,
  type InboxAuthAction
} from '../nostr/inbox-auth.ts';
import {
  getPrivatePool,
  closePrivatePool,
  privatePoolCleanupRequired,
  type PrivatePool
} from '../nostr/private-pool.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  inboxResolutionSnapshot,
  inboxResolverCurrentAfterSample,
  type InboxResolver
} from './resolve-inbox.ts';

declare const accessBrand: unique symbol;
export type InboxAccessController = Readonly<{ [accessBrand]: true }>;
type Binding = Readonly<{
  owner: string;
  preferenceId: string;
  readTargets: string[];
  archiveTargets: string[];
}>;
export type InboxAccessSnapshot = Readonly<
  Binding & {
    state: string;
    busy: boolean;
    cleanupRequired: boolean;
    readiness: 'unavailable';
    auth: { origin: string; state: string; responses: number }[];
  }
>;
type Control = {
  policy: RelayPolicy;
  binding: string;
  current(): boolean;
  closed(): boolean;
  cleanupRequired(): boolean;
  snapshot(): InboxAccessSnapshot;
  authenticate(origin: unknown, review: unknown): Promise<string>;
  respond(origin: unknown, review: unknown): Promise<string>;
  close(): boolean;
};
const controllers = new WeakMap<InboxAccessController, Control>();
const currentControllers = new WeakMap<PrivateSession, InboxAccessController>();
function binding(
  session: PrivateSession,
  policy: RelayPolicy,
  own: InboxResolver
): Binding | undefined {
  try {
    const capture = privateSessionOwnership(session),
      head = inboxResolutionSnapshot(own);
    const sources = publicRelayTargets(policy, 'read');
    const preference = head.head && inboxPreferenceSnapshot(head.head);
    if (
      !capture?.current() ||
      !readRelayPolicy(policy).messagingEnabled ||
      head.author !== capture.owner ||
      head.status !== 'ready' ||
      head.coverage !== 'bounded-eose' ||
      !preference ||
      preference.status !== 'supported' ||
      preference.author !== capture.owner ||
      preference.id !== head.knownBase?.id ||
      preference.createdAt !== head.knownBase.createdAt ||
      !sources.length ||
      head.sources.length !== sources.length ||
      !sources.every((origin) =>
        head.sources.some(
          (source) => source.source === origin && source.state === 'eose'
        )
      )
    )
      return undefined;
    const readTargets = inboxRelayTargets(
      policy,
      preference.relays,
      'read'
    ).slice();
    const archiveTargets = inboxRelayTargets(
      policy,
      preference.relays,
      'write'
    ).slice();
    if (
      !readTargets.length ||
      !archiveTargets.length ||
      !capture.current() ||
      !inboxResolverCurrentAfterSample(own)
    )
      return undefined;
    return {
      owner: capture.owner,
      preferenceId: preference.id,
      readTargets,
      archiveTargets
    };
  } catch {
    return undefined;
  }
}
// Capture only genuine current discovery/session metadata. No socket, provider,
// AUTH, probe or publication occurs here. AUTH alone never grants readiness;
// the separate qualified access probe must supply that later capability.
export function createInboxAccessController(
  session: PrivateSession,
  policy: RelayPolicy,
  own: InboxResolver
): InboxAccessController | undefined {
  if (typeof window === 'undefined') return undefined;
  const captured = binding(session, policy, own);
  if (!captured) return undefined;
  const initial = captured;
  const wire = JSON.stringify(initial),
    previous = currentControllers.get(session);
  const prior = previous && controllers.get(previous);
  if (
    prior &&
    !prior.closed() &&
    prior.current() &&
    prior.policy === policy &&
    prior.binding === wire
  )
    return previous;
  // Stale or failed disposal cannot be silently replaced by another capture.
  if (prior && (!prior.closed() || prior.cleanupRequired())) return undefined;
  const token = Object.freeze({}) as InboxAccessController;
  const auth = new Map<string, InboxAuthAction>();
  let pool: PrivatePool | undefined,
    closed = false,
    cleaned = false,
    busy = false,
    epoch = 0,
    phase = 'not_checked';
  let off = () => {};
  const current = () =>
    !closed && JSON.stringify(binding(session, policy, own)) === wire;
  function close(): boolean {
    closed = true;
    epoch++;
    phase = 'stopped';
    let failed = false;
    for (const [origin, action] of auth) {
      try {
        closeInboxAuthentication(action);
        auth.delete(origin);
      } catch {
        failed = true;
      }
    }
    if (pool) {
      try {
        closePrivatePool(pool);
      } catch {
        failed = true;
      }
      if (privatePoolCleanupRequired(pool)) failed = true;
    }
    cleaned = !failed;
    if (cleaned) off();
    return cleaned;
  }
  async function authenticate(
    origin: unknown,
    review: unknown
  ): Promise<string> {
    if (
      review !== 'reviewed_inbox_connection_auth' ||
      typeof origin !== 'string' ||
      !current() ||
      (!initial.readTargets.includes(origin) &&
        !initial.archiveTargets.includes(origin))
    )
      return 'unavailable';
    if (busy) return 'busy';
    const existing = auth.get(origin);
    if (existing) return inboxAuthSnapshot(existing)?.state ?? 'stopped';
    busy = true;
    const generation = epoch;
    try {
      pool ??= getPrivatePool(
        session,
        policy,
        Array.from(
          new Set([...initial.readTargets, ...initial.archiveTargets])
        ),
        token,
        'access',
        current
      );
      if (!pool || !current()) return 'unavailable';
      const action = await beginInboxAuthentication(
        pool,
        origin,
        'reviewed_connection_auth'
      );
      if (!action) return (phase = 'unavailable');
      if (!current() || generation !== epoch) {
        closeInboxAuthentication(action);
        return 'stopped';
      }
      auth.set(origin, action);
      phase = 'authenticating';
      return inboxAuthSnapshot(action)?.state ?? 'unavailable';
    } catch {
      return (phase = closed ? 'stopped' : 'unavailable');
    } finally {
      busy = false;
    }
  }
  async function respond(origin: unknown, review: unknown): Promise<string> {
    const action = typeof origin === 'string' && auth.get(origin);
    if (!action || review !== 'reviewed_inbox_auth_response' || !current())
      return 'unavailable';
    if (busy) return 'busy';
    busy = true;
    const generation = epoch;
    try {
      const result = await respondInboxAuthentication(action);
      if (!current() || generation !== epoch) {
        closeInboxAuthentication(action);
        return 'stopped';
      }
      phase = result.status;
      return result.status;
    } catch {
      return (phase = closed ? 'stopped' : 'unavailable');
    } finally {
      busy = false;
    }
  }
  controllers.set(token, {
    policy,
    binding: wire,
    current,
    closed: () => closed,
    cleanupRequired: () =>
      closed && (!cleaned || (!!pool && privatePoolCleanupRequired(pool))),
    authenticate,
    respond,
    close,
    snapshot: () => ({
      ...initial,
      readTargets: [...initial.readTargets],
      archiveTargets: [...initial.archiveTargets],
      state: closed ? 'stopped' : current() ? phase : 'stale',
      busy,
      cleanupRequired:
        closed && (!cleaned || (!!pool && privatePoolCleanupRequired(pool))),
      readiness: 'unavailable',
      auth: [...auth].map(([origin, action]) => ({
        origin,
        state: inboxAuthSnapshot(action)?.state ?? 'stopped',
        responses: inboxAuthSnapshot(action)?.responses ?? 0
      }))
    })
  });
  off = subscribePrivateSessionClose(session, () => {
    if (!close()) throw Error('inbox_access_cleanup_required');
  });
  if (!current()) {
    close();
    return undefined;
  }
  currentControllers.set(session, token);
  return token;
}
export function inboxAccessControllerSnapshot(
  token: InboxAccessController
): InboxAccessSnapshot | undefined {
  return controllers.get(token)?.snapshot();
}
export function authenticateInboxAccess(
  token: InboxAccessController,
  origin: unknown,
  review: unknown
): Promise<string> {
  return (
    controllers.get(token)?.authenticate(origin, review) ??
    Promise.resolve('unavailable')
  );
}
export function respondInboxAccessAuthentication(
  token: InboxAccessController,
  origin: unknown,
  review: unknown
): Promise<string> {
  return (
    controllers.get(token)?.respond(origin, review) ??
    Promise.resolve('unavailable')
  );
}
export function closeInboxAccessController(
  token: InboxAccessController
): boolean {
  return controllers.get(token)?.close() ?? false;
}
