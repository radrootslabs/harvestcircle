import {
  captureInboxSetup,
  inboxSetupRecordCurrent,
  type InboxSetupReview
} from './inbox-setup.ts';
import type { InboxResolver } from './resolve-inbox.ts';
import {
  publicRelayTargets,
  readRelayPolicy,
  type RelayPolicy
} from '../config/relays.ts';
import { canonicalLocalId } from '../private-handles.ts';
import {
  identityMessagingOwnership,
  captureIdentityPublicOperation,
  identityPublicOperationOwnership,
  signIdentityPublicOperation,
  stopIdentityPublicOperation,
  recheckIdentityOwner,
  subscribeIdentityInvalidation,
  type IdentitySession,
  type IdentityPublicOperation
} from '../runtime/identity-session.ts';
import {
  runCapturedPublicEffect,
  type PublicEffectLease
} from '../runtime/effect-ownership.ts';
import {
  publicRecordSnapshot,
  decodePublicRecord,
  type PublicRecordHandle
} from '../persistence/records.ts';
import {
  loadPreferenceOperation,
  commitPublicOperationTransition,
  observePublicOperationTransition,
  publicQuotaOwner,
  type PublicQuotaRepository
} from '../persistence/quota.ts';
import {
  preparePreferenceSigningTransition,
  preparePublicArtifactTransition,
  type PublicOperationTransition
} from '../persistence/artifact-records.ts';
import { getPublicPool } from '../nostr/public-pool.ts';
import {
  authorizeStoredInboxPreference,
  preferencePolicyFingerprint
} from '../nostr/inbox-preference-publication.ts';
import {
  publishInboxPreference,
  type PreferencePublicationResult
} from '../nostr/inbox-preference-publisher.ts';
declare const actionBrand: unique symbol;
export type InboxPreferenceAction = Readonly<{ [actionBrand]: true }>;
type Lookup = () => Promise<InboxResolver>;
type Saved = Readonly<{
  repository: PublicQuotaRepository;
  identity: IdentitySession;
  policy: RelayPolicy;
  original: PublicRecordHandle;
  operation: IdentityPublicOperation;
  id: string;
  actionId: string;
  lookup: Lookup;
  retry: boolean;
  control: AbortController;
  running(): boolean;
  markRunning(): void;
}>;
const actions = new WeakMap<InboxPreferenceAction, Saved>();
type Prepared =
  | Readonly<{ status: 'prepared'; action: InboxPreferenceAction }>
  | Readonly<{ status: 'invalid' | 'unavailable' | 'conflict' | 'unknown' }>;
function retain(
  repository: PublicQuotaRepository,
  identity: IdentitySession,
  policy: RelayPolicy,
  original: PublicRecordHandle,
  id: string,
  lookup: Lookup,
  retry: boolean
): Prepared {
  const operation = captureIdentityPublicOperation(
    identity,
    original,
    id,
    'reviewed_captured_operation'
  );
  if (!operation || typeof lookup !== 'function')
    return { status: 'unavailable' };
  let used = false;
  const action = Object.freeze({}) as InboxPreferenceAction;
  actions.set(action, {
    repository,
    identity,
    policy,
    original,
    operation,
    id,
    actionId: crypto.randomUUID(),
    lookup,
    retry,
    control: new AbortController(),
    running: () => used,
    markRunning: () => {
      used = true;
    }
  });
  return { status: 'prepared', action };
}
// Original opaque065 review only. This does not auto-run after connect/setup.
export async function beginInboxPreferenceOperation(
  repository: PublicQuotaRepository,
  identity: IdentitySession,
  policy: RelayPolicy,
  review: InboxSetupReview,
  command: unknown,
  lookup: Lookup,
  consent: unknown
): Promise<Prepared> {
  try {
    const id = canonicalLocalId(command),
      owner = identityMessagingOwnership(identity);
    if (!id || !owner || publicQuotaOwner(repository) !== owner.owner)
      return { status: 'unavailable' };
    const resolver = await lookup(),
      captured = captureInboxSetup(review, resolver, id, consent);
    if (captured.status !== 'captured') return captured;
    if (captured.owner !== owner.owner || !owner.current())
      return { status: 'unavailable' };
    return retain(
      repository,
      identity,
      policy,
      captured.record,
      id,
      lookup,
      false
    );
  } catch {
    return { status: 'unavailable' };
  }
}
// Explicit reload review of the actual stored signed operation, with full
// original capture retained. Unsigned uncertainty cannot become a fresh plan.
export async function resumeInboxPreferenceOperation(
  repository: PublicQuotaRepository,
  identity: IdentitySession,
  policy: RelayPolicy,
  command: unknown,
  lookup: Lookup,
  consent: unknown
): Promise<Prepared> {
  try {
    if (consent !== 'reviewed_stored_inbox_preference')
      return { status: 'invalid' };
    const id = canonicalLocalId(command);
    await recheckIdentityOwner(identity);
    const owner = identityMessagingOwnership(identity);
    if (!id || !owner || publicQuotaOwner(repository) !== owner.owner)
      return { status: 'unavailable' };
    const read = await loadPreferenceOperation(repository, id);
    if (!read.ok) return { status: 'unavailable' };
    const row = publicRecordSnapshot(read.value, owner.owner, id);
    if (row?.family !== 'preference_operation') return { status: 'invalid' };
    if (row.revision < 2 || !row.artifact) return { status: 'unknown' };
    const manifest = readRelayPolicy(policy),
      fingerprint = await preferencePolicyFingerprint(policy),
      resolver = await lookup();
    if (
      !manifest.messagingEnabled ||
      !manifest.postingEnabled ||
      row.capture.policyFingerprint !== fingerprint ||
      JSON.stringify(row.capture.targets) !==
        JSON.stringify(publicRelayTargets(policy, 'write')) ||
      !inboxSetupRecordCurrent(read.value, owner.owner, id, resolver, policy) ||
      !owner.current()
    )
      return { status: 'conflict' };
    // Historical original signing wire for lease matching only; never written
    // back, never a reset. The effect runner uses the actual stored artifact.
    const original = decodePublicRecord(
      JSON.stringify({ ...row, revision: 0, artifact: null, receipts: [] }),
      owner.owner,
      id
    );
    return original.ok
      ? retain(repository, identity, policy, original.value, id, lookup, true)
      : { status: 'invalid' };
  } catch {
    return { status: 'unavailable' };
  }
}
async function acknowledged(
  repository: PublicQuotaRepository,
  transition: PublicOperationTransition
): Promise<boolean> {
  const result = await commitPublicOperationTransition(repository, transition);
  return (
    result.ok &&
    (await observePublicOperationTransition(repository, transition)).state ===
      'committed'
  );
}
async function knownBase(saved: Saved): Promise<(() => boolean) | undefined> {
  const owner = identityMessagingOwnership(saved.identity);
  const operation = identityPublicOperationOwnership(
    saved.identity,
    saved.operation
  );
  if (!owner || !operation || saved.control.signal.aborted) return undefined;
  const stored = await loadPreferenceOperation(saved.repository, saved.id);
  if (!stored.ok) return undefined;
  const resolver = await saved.lookup();
  const current = () =>
    !saved.control.signal.aborted &&
    operation.current() &&
    owner.current() &&
    inboxSetupRecordCurrent(
      stored.value,
      owner.owner,
      saved.id,
      resolver,
      saved.policy
    );
  return current() ? current : undefined;
}
export async function runInboxPreferenceOperation(
  action: InboxPreferenceAction
): Promise<PreferencePublicationResult> {
  const saved = actions.get(action);
  if (!saved) return { status: 'invalid' };
  if (saved.running() || saved.control.signal.aborted)
    return { status: 'stopped' };
  saved.markRunning();
  const capture = identityPublicOperationOwnership(
    saved.identity,
    saved.operation
  );
  if (!capture) return { status: 'stopped' };
  const messagingOwner = identityMessagingOwnership(saved.identity);
  if (
    !messagingOwner ||
    messagingOwner.owner !== capture.owner ||
    messagingOwner.session !== capture.session
  )
    return { status: 'stopped' };
  if (!capture.current() || saved.control.signal.aborted)
    return { status: 'stopped' };
  const invalidate = subscribeIdentityInvalidation(saved.identity, () => {
    // Notifications also describe pending/completed observations. Only actual
    // owner/capability/generation loss cancels this original explicit action.
    if (!capture.current() || !messagingOwner.current()) saved.control.abort();
  });
  try {
    const result = await runCapturedPublicEffect<PreferencePublicationResult>(
      saved.repository,
      saved.original,
      saved.id,
      {
        ...capture,
        current: () => capture.current() && !saved.control.signal.aborted
      },
      saved.retry
        ? 'reviewed_stored_preference_retry'
        : 'reviewed_prepared_preference_operation',
      async (lease: PublicEffectLease, claimed: PublicRecordHandle) => {
        if (!(await knownBase(saved))) return { status: 'stopped' };
        const row = publicRecordSnapshot(claimed, capture.owner, saved.id);
        if (row?.family !== 'preference_operation')
          return { status: 'invalid' };
        const manifest = readRelayPolicy(saved.policy);
        if (
          !manifest.messagingEnabled ||
          !manifest.postingEnabled ||
          row.capture.policyFingerprint !==
            (await preferencePolicyFingerprint(saved.policy)) ||
          JSON.stringify(row.capture.targets) !==
            JSON.stringify(publicRelayTargets(saved.policy, 'write'))
        )
          return { status: 'stopped' };
        if (!capture.current() || saved.control.signal.aborted)
          return { status: 'stopped' };
        if (!row.artifact) {
          const marker = preparePreferenceSigningTransition(
            claimed,
            capture.owner,
            saved.id
          );
          if (
            !marker.ok ||
            !(await acknowledged(saved.repository, marker.value))
          )
            return { status: 'storage_failed' };
          if (!(await knownBase(saved))) return { status: 'stopped' };
          const signed = await signIdentityPublicOperation(
            saved.identity,
            saved.operation,
            lease
          );
          if (signed.status !== 'signed') return { status: 'stopped' };
          const pending = await loadPreferenceOperation(
            saved.repository,
            saved.id
          );
          if (!pending.ok) return { status: 'storage_failed' };
          if (!capture.current() || saved.control.signal.aborted)
            return { status: 'stopped' };
          const transition = preparePublicArtifactTransition(
            pending.value,
            capture.owner,
            saved.id,
            signed.artifact
          );
          if (
            !transition.ok ||
            !(await acknowledged(saved.repository, transition.value))
          )
            return { status: 'storage_failed' };
        }
        if (saved.control.signal.aborted || !capture.current())
          return { status: 'stopped' };
        const pool = getPublicPool(saved.policy);
        if (!pool) return { status: 'stopped' };
        return await publishInboxPreference(
          saved.repository,
          capture.owner,
          saved.id,
          saved.actionId,
          pool,
          saved.policy,
          saved.control.signal,
          () =>
            authorizeStoredInboxPreference(
              saved.repository,
              saved.identity,
              saved.operation,
              lease,
              saved.original,
              saved.id,
              saved.policy,
              () => knownBase(saved)
            )
        );
      }
    );
    return result.status === 'completed' ? result.value : { status: 'stopped' };
  } catch {
    return { status: 'stopped' };
  } finally {
    invalidate();
  }
}
export function stopInboxPreferenceOperation(
  action: InboxPreferenceAction
): void {
  const saved = actions.get(action);
  if (!saved) return;
  saved.control.abort();
  stopIdentityPublicOperation(saved.identity, saved.operation);
}
