import {
  identitySessionSnapshot,
  identityMessagingOwnership,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  privateSessionOwnership,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  readRelayPolicy,
  publicRelayTargets,
  inboxRelayTargets,
  type RelayPolicy
} from '../config/relays.ts';
import { inboxPreferenceSnapshot } from '../nostr/inbox-preferences.ts';
import {
  inboxResolutionSnapshot,
  inboxResolverCurrentAfterSample,
  type InboxResolver
} from './resolve-inbox.ts';
import { createInboxRoutePlan } from './inbox-routing.ts';

export type OwnInboxAccessContext = Readonly<{
  owner: string;
  session: symbol;
  preferenceId: string;
  readTargets: readonly string[];
  archiveTargets: readonly string[];
}>;
export type InboxAccessObservation = Readonly<{
  owner: string;
  session: symbol;
  preferenceId: string;
  readTargets: readonly string[];
  archiveTargets: readonly string[];
  receive: 'qualified_exercised';
  archive: 'qualified_exercised';
  current(): boolean;
}>;
// Internal trusted observation port, like the discovery request/generation ports.
// It must report real qualified, exercised access for these exact bindings, not
// infer it from manifest flags, an EOSE or a caller boolean. No production port
// is configured here. Controlled test observations qualify only the projection.
export type ObserveInboxAccess = (
  context: OwnInboxAccessContext
) => InboxAccessObservation | undefined;
export type MessagingReadiness = Readonly<{
  browse: true;
  draft: true;
  preferenceWrite: 'not_requested';
  ownConfiguration: 'compatible' | 'missing' | 'unsupported' | 'unknown';
  ownAccess: 'qualified_exercised' | 'unavailable';
  newListingReady: boolean;
  sendReady: boolean;
}>;
function sameTargets(a: readonly string[], b: readonly string[]): boolean {
  return (
    a.length === b.length && a.every((origin, index) => origin === b[index])
  );
}
// Detached informational projection only. No prompt, transport, publication or
// private effect permission. Effect owners still require fresh qualified proof,
// account recheck and explicit review; a cached boolean never authorizes them.
export function messagingReadinessSnapshot(
  identity: IdentitySession,
  session: PrivateSession | undefined,
  policy: RelayPolicy,
  own: InboxResolver | undefined,
  peer?: Readonly<{ author: unknown; resolver: InboxResolver }>,
  observeAccess?: ObserveInboxAccess
): MessagingReadiness {
  const blocked: MessagingReadiness = {
    browse: true,
    draft: true,
    preferenceWrite: 'not_requested',
    ownConfiguration: 'unknown',
    ownAccess: 'unavailable',
    newListingReady: false,
    sendReady: false
  };
  try {
    const identityState = identitySessionSnapshot(identity);
    const capture = identityMessagingOwnership(identity);
    const privateOwner = session && privateSessionOwnership(session);
    if (
      identityState.state !== 'messaging_capable' ||
      !capture ||
      !privateOwner ||
      privateOwner.owner !== capture.owner ||
      privateOwner.session !== capture.session ||
      !own
    )
      return blocked;
    const manifest = readRelayPolicy(policy);
    const resolution = inboxResolutionSnapshot(own);
    if (resolution.author !== capture.owner) return blocked;
    if (
      resolution.status !== 'ready' ||
      !resolution.head ||
      !resolution.knownBase
    )
      return {
        ...blocked,
        ownConfiguration:
          resolution.status === 'missing'
            ? 'missing'
            : resolution.status === 'unsupported'
              ? 'unsupported'
              : 'unknown'
      };
    const preference = inboxPreferenceSnapshot(resolution.head);
    const discovery = publicRelayTargets(policy, 'read');
    if (
      !preference ||
      preference.status !== 'supported' ||
      preference.author !== capture.owner ||
      preference.id !== resolution.knownBase.id ||
      resolution.coverage !== 'bounded-eose' ||
      discovery.length === 0 ||
      discovery.length !== resolution.sources.length ||
      !discovery.every((origin) =>
        resolution.sources.some(
          (row) => row.source === origin && row.state === 'eose'
        )
      )
    )
      return blocked;
    const readTargets = inboxRelayTargets(policy, preference.relays, 'read');
    const archiveTargets = inboxRelayTargets(
      policy,
      preference.relays,
      'write'
    );
    if (readTargets.length === 0 || archiveTargets.length === 0)
      return { ...blocked, ownConfiguration: 'unsupported' };
    const configured = { ...blocked, ownConfiguration: 'compatible' as const };
    if (!manifest.messagingEnabled || !observeAccess) return configured;
    // Detached input prevents a port from changing our expected routes.
    const context: OwnInboxAccessContext = {
      owner: capture.owner,
      session: capture.session,
      preferenceId: preference.id,
      readTargets: readTargets.slice(),
      archiveTargets: archiveTargets.slice()
    };
    const access = observeAccess(context);
    const matches =
      !!access &&
      access.owner === capture.owner &&
      access.session === capture.session &&
      access.preferenceId === preference.id &&
      access.receive === 'qualified_exercised' &&
      access.archive === 'qualified_exercised' &&
      sameTargets(access.readTargets, readTargets) &&
      sameTargets(access.archiveTargets, archiveTargets);
    const plan =
      peer &&
      createInboxRoutePlan(
        policy,
        capture.owner,
        peer.author,
        own,
        peer.resolver
      );
    // Sample-free ownership fences after all clocks and trusted port reads.
    const available =
      matches &&
      access.current() &&
      capture.current() &&
      privateOwner.current() &&
      inboxResolverCurrentAfterSample(own);
    if (!available) return configured;
    return {
      ...configured,
      ownAccess: 'qualified_exercised',
      newListingReady: true,
      sendReady:
        !!plan && !!peer && inboxResolverCurrentAfterSample(peer.resolver)
    };
  } catch {
    return blocked;
  }
}
