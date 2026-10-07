import {
  readRelayPolicy,
  publicRelayTargets,
  type RelayPolicy
} from '../config/relays.ts';
import { publicRecordSnapshot } from '../persistence/records.ts';
import {
  loadPreferenceOperation,
  commitPublicOperationTransition,
  observePublicOperationTransition,
  publicQuotaOwner,
  type PublicQuotaRepository
} from '../persistence/quota.ts';
import { preparePublicReceiptTransition } from '../persistence/artifact-records.ts';
import { preferencePolicyFingerprint } from '../nostr/inbox-preference-publication.ts';
import {
  inboxReadbackSnapshot,
  inboxReadbackEvidence,
  useInboxReadback,
  type InboxReadback
} from '../nostr/inbox-readback.ts';
import {
  identityMessagingOwnership,
  recheckIdentityOwner,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  privateSessionOwnership,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  inboxResolverCurrentAfterSample,
  type InboxResolver
} from './resolve-inbox.ts';
import {
  messagingReadinessSnapshot,
  type ObserveInboxAccess,
  type InboxAccessObservation
} from './readiness.ts';
type Peer = Readonly<{ author: unknown; resolver: InboxResolver }>;
type Status =
  | 'invalid'
  | 'unknown'
  | 'stopped'
  | 'busy'
  | 'storage_failed'
  | 'awaiting_acceptance'
  | 'awaiting_readback'
  | 'conflict'
  | 'access_unavailable'
  | 'verified'
  | 'compatible_existing';
export type InboxSetupVerificationResult = Readonly<{
  status: Status;
  setupComplete: boolean;
  newListingReady: boolean;
  sendReady: boolean;
  knownHead?: Readonly<{ id: string; createdAt: number }>;
}>;
function blocked(
  status: Status,
  knownHead?: InboxSetupVerificationResult['knownHead']
): InboxSetupVerificationResult {
  return {
    status,
    setupComplete: false,
    newListingReady: false,
    sendReady: false,
    ...(knownHead ? { knownHead } : {})
  };
}
// Existing compatible configuration does not create a local command or perform
// a redundant write. This is detached information, never effect permission.
export function existingInboxSetupSnapshot(
  identity: IdentitySession,
  session: PrivateSession | undefined,
  policy: RelayPolicy,
  own: InboxResolver,
  observeAccess?: ObserveInboxAccess,
  peer?: Peer
): InboxSetupVerificationResult {
  const ready = messagingReadinessSnapshot(
    identity,
    session,
    policy,
    own,
    peer,
    observeAccess
  );
  if (ready.ownAccess !== 'qualified_exercised')
    return blocked('access_unavailable');
  return {
    status: 'compatible_existing',
    setupComplete: true,
    newListingReady:
      readRelayPolicy(policy).postingEnabled && ready.newListingReady,
    sendReady: ready.sendReady
  };
}
// Explicit verification of a chosen durable operation. ACK and exact readback
// remain separate facts. No EVENT, extension signing, AUTH or auto replacement.
export async function verifyInboxSetupOperation(
  repository: PublicQuotaRepository,
  identity: IdentitySession,
  session: PrivateSession | undefined,
  policy: RelayPolicy,
  reader: InboxReadback,
  observeAccess?: ObserveInboxAccess,
  peer?: Peer
): Promise<InboxSetupVerificationResult> {
  const initial = inboxReadbackSnapshot(reader);
  if (!initial) return blocked('invalid');
  if (!useInboxReadback(reader)) return blocked('stopped', initial.knownHead);
  try {
    if (typeof window === 'undefined' || !navigator.locks?.request)
      return blocked('unknown');
    await recheckIdentityOwner(identity);
    const owner = identityMessagingOwnership(identity);
    if (
      !owner ||
      owner.owner !== initial.owner ||
      publicQuotaOwner(repository) !== owner.owner
    )
      return blocked('stopped');
    return await navigator.locks.request(
      'harvestcircle:owner:' + owner.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock) => {
        if (!lock) return blocked('busy');
        const evidence = inboxReadbackEvidence(reader);
        if (!evidence || !owner.current())
          return blocked('unknown', initial.knownHead);
        const original = publicRecordSnapshot(
          evidence.record,
          owner.owner,
          evidence.id
        );
        const read = await loadPreferenceOperation(repository, evidence.id);
        if (!read.ok) return blocked('storage_failed');
        const row = publicRecordSnapshot(read.value, owner.owner, evidence.id);
        if (
          row?.family !== 'preference_operation' ||
          original?.family !== 'preference_operation' ||
          !row.artifact ||
          row.artifact.wire !== original.artifact?.wire ||
          JSON.stringify(row.capture) !== JSON.stringify(original.capture) ||
          JSON.stringify(row.source) !== JSON.stringify(original.source) ||
          row.consent !== original.consent ||
          evidence.policy !== policy ||
          row.capture.policyFingerprint !==
            (await preferencePolicyFingerprint(policy)) ||
          JSON.stringify(row.capture.targets) !==
            JSON.stringify(publicRelayTargets(policy, 'write')) ||
          !owner.current() ||
          !evidence.current()
        )
          return blocked('stopped');
        const accepted = row.receipts.filter(
          (fact) =>
            fact.status === 'accepted' && fact.eventId === row.artifact!.eventId
        );
        if (accepted.length === 0)
          return blocked('awaiting_acceptance', evidence.knownHead);
        // Named ACK and discovery readback may come from different fixed relays.
        // Each readback retains its own actual source, never an ACK at that source.
        const observed = evidence.wires;
        if (observed.length === 0)
          return blocked(
            evidence.status === 'conflict' ? 'conflict' : 'awaiting_readback',
            evidence.knownHead
          );
        // A fresh readback action retains prior ACK facts and records the actual
        // read observation; it never claims a new EVENT or new relay ACK.
        const actionId = crypto.randomUUID();
        for (const proof of observed) {
          const prior =
            accepted.find((fact) => fact.origin === proof.origin) ??
            accepted[0];
          if (!owner.current() || !evidence.current())
            return blocked('stopped');
          const current = await loadPreferenceOperation(
            repository,
            evidence.id
          );
          if (!current.ok) return blocked('storage_failed');
          const stored = publicRecordSnapshot(
            current.value,
            owner.owner,
            evidence.id
          );
          if (stored?.family !== 'preference_operation')
            return blocked('storage_failed');
          if (
            stored.receipts.some(
              (fact) =>
                fact.origin === prior.origin &&
                fact.readbackOrigin === proof.origin &&
                fact.status === 'accepted' &&
                fact.readbackWire === proof.wire
            )
          )
            continue;
          const transition = preparePublicReceiptTransition(
            current.value,
            owner.owner,
            evidence.id,
            JSON.stringify({
              actionId,
              origin: prior.origin,
              readbackOrigin: proof.origin,
              role: 'preference',
              attempt: prior.attempt,
              eventId: row.artifact.eventId,
              status: 'accepted',
              observedAtMilliseconds: Date.now(),
              readbackWire: proof.wire
            })
          );
          if (!owner.current() || !evidence.current())
            return blocked('stopped');
          if (
            !transition.ok ||
            !(
              await commitPublicOperationTransition(
                repository,
                transition.value
              )
            ).ok ||
            (
              await observePublicOperationTransition(
                repository,
                transition.value
              )
            ).state !== 'committed'
          )
            return blocked('storage_failed');
        }
        const privateOwner = session && privateSessionOwnership(session);
        let observedAccess: InboxAccessObservation | undefined;
        const ready = messagingReadinessSnapshot(
          identity,
          session,
          policy,
          evidence.resolver,
          peer,
          observeAccess
            ? (context) => {
                const observation = observeAccess(context);
                observedAccess = observation;
                return observation;
              }
            : undefined
        );
        // The last head sample can invalidate exercised access. Recheck the
        // retained access proof before the final clock-free ownership fences.
        const final = inboxReadbackSnapshot(reader);
        const accessCurrent = observedAccess?.current() ?? false;
        if (
          !final ||
          !owner.current() ||
          !privateOwner?.current() ||
          !evidence.current() ||
          (peer && !inboxResolverCurrentAfterSample(peer.resolver))
        )
          return blocked('stopped', final?.knownHead);
        if (final.status === 'conflict')
          return blocked('conflict', final.knownHead);
        if (final.status !== 'readback' || !final.complete)
          return blocked('unknown', final.knownHead);
        if (ready.ownAccess !== 'qualified_exercised' || !accessCurrent)
          return blocked('access_unavailable', final.knownHead);
        return {
          status: 'verified',
          setupComplete: true,
          newListingReady:
            readRelayPolicy(policy).postingEnabled && ready.newListingReady,
          sendReady: ready.sendReady,
          knownHead: final.knownHead
        };
      }
    );
  } catch {
    return blocked('unknown');
  }
}
