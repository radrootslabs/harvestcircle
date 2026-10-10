import type { NostrEvent } from 'applesauce-core/helpers';
import {
  publicRelayTargets,
  readRelayPolicy,
  type RelayPolicy
} from '../config/relays.ts';
import {
  listingPublicationOwnership,
  type ListingPublication
} from '../publishing/sign-listing.ts';
import {
  listingResumeOwnership,
  type ListingResumption
} from '../publishing/resume-listing.ts';
import {
  listingRetryOwnership,
  type ListingRetry
} from '../publishing/retry-listing.ts';
import {
  identityMessagingOwnership,
  identityPublicOperationOwnership,
  recheckIdentityOwner,
  type IdentitySession
} from '../runtime/identity-session.ts';
import { preferencePolicyFingerprint } from './inbox-preference-publication.ts';
import { readPublicListingIntent } from '../persistence/public-publications.ts';
import {
  decodePublicRecord,
  publicRecordSnapshot,
  type PublicRecordHandle
} from '../persistence/records.ts';
declare const leaseBrand: unique symbol;
declare const permissionBrand: unique symbol;
export type ListingDeliveryLease = Readonly<{ [leaseBrand]: true }>;
export type ListingAttemptPermission = Readonly<{ [permissionBrand]: true }>;
type Original = NonNullable<
  | ReturnType<typeof listingPublicationOwnership>
  | ReturnType<typeof listingResumeOwnership>
>;
type Owner = Readonly<{
  original: Original;
  policy: RelayPolicy;
  signal?: AbortSignal;
  current(): boolean;
}>;
type Permission = Readonly<{
  owner: Owner;
  origin: string;
  mode: 'publish' | 'readback';
  wire: string;
}>;
const leases = new WeakMap<ListingDeliveryLease, Owner>();
const permissions = new WeakMap<ListingAttemptPermission, Permission>();
// Stop releases this action's wait and author lock. The extension promise may
// settle later; it cannot resurrect the revoked network lease or a signer.
async function recheckDeliveryOwner(
  identity: IdentitySession,
  signal?: AbortSignal
): Promise<boolean> {
  if (signal?.aborted) return false;
  if (!signal) {
    await recheckIdentityOwner(identity);
    return true;
  }
  let stop = () => {};
  const stopped = new Promise<boolean>((resolve) => {
    stop = () => resolve(false);
    signal.addEventListener('abort', stop, { once: true });
    if (signal.aborted) stop();
  });
  try {
    return await Promise.race([
      recheckIdentityOwner(identity).then(() => true),
      stopped
    ]);
  } finally {
    signal.removeEventListener('abort', stop);
  }
}
// Receipt revisions may grow, but the complete original operation payload and
// verified artifact stay exact. No new command, author or destination is adopted.
export async function readRetainedListingDelivery(
  lease: ListingDeliveryLease
): Promise<PublicRecordHandle | undefined> {
  const owner = leases.get(lease);
  if (!owner) return;
  const original = owner.original;
  const expected = decodePublicRecord(
    original.recordWire,
    original.owner,
    original.command
  );
  if (!expected.ok) return;
  const baseline = publicRecordSnapshot(
    expected.value,
    original.owner,
    original.command
  );
  if (
    baseline?.family !== 'public_operation' ||
    baseline.capture.kind !== 30402 ||
    !baseline.artifact ||
    baseline.revision !== 2
  )
    return;
  const read = await readPublicListingIntent(
    original.repository,
    original.command
  );
  if (read.status !== 'found') return;
  const row = publicRecordSnapshot(
    read.record,
    original.owner,
    original.command
  );
  if (
    row?.family !== 'public_operation' ||
    row.revision < baseline.revision ||
    JSON.stringify({
      ...row,
      revision: baseline.revision,
      receipts: baseline.receipts
    }) !== JSON.stringify(baseline)
  )
    return;
  return read.record;
}
// Current authority is mandatory for every new network permission. Retained
// exact storage observation above remains usable for bookkeeping after Stop.
export async function readOwnedListingDelivery(
  lease: ListingDeliveryLease
): Promise<PublicRecordHandle | undefined> {
  const owner = leases.get(lease);
  if (!owner?.current()) return;
  const read = readRetainedListingDelivery(lease);
  let stop = () => {};
  const stopped =
    owner.signal &&
    new Promise<undefined>((resolve) => {
      stop = () => resolve(undefined);
      owner.signal!.addEventListener('abort', stop, { once: true });
      if (owner.signal!.aborted) stop();
    });
  let record: PublicRecordHandle | undefined;
  try {
    record = await (stopped ? Promise.race([read, stopped]) : read);
  } finally {
    owner.signal?.removeEventListener('abort', stop);
  }
  return owner.current() ? record : undefined;
}
export function listingDeliveryOwnership(lease: ListingDeliveryLease) {
  const owner = leases.get(lease);
  return owner?.current()
    ? {
        repository: owner.original.repository,
        owner: owner.original.owner,
        command: owner.original.command,
        current: owner.current
      }
    : undefined;
}
export type ListingOwnershipResult<T> =
  | Readonly<{ status: 'completed'; value: T }>
  | Readonly<{ status: 'invalid' | 'unavailable' | 'busy' | 'stopped' }>;
// One original author lock spans this finite explicitly requested network
// action. It never spans a native IDB transaction and cannot mint a signer.
export async function withListingDeliveryOwnership<T>(
  publication: ListingPublication | ListingResumption,
  policy: RelayPolicy,
  consent: unknown,
  work: (lease: ListingDeliveryLease) => Promise<T>,
  control?: ListingRetry
): Promise<ListingOwnershipResult<T>> {
  const signed = listingPublicationOwnership(publication as ListingPublication),
    resumed = listingResumeOwnership(publication as ListingResumption),
    original = signed ?? resumed,
    retry = control && listingRetryOwnership(control);
  if (
    !original ||
    (control
      ? !retry ||
        retry.source !== publication ||
        retry.policy !== policy ||
        retry.consent !== consent
      : !signed || consent !== 'publish_stored_listing') ||
    original.policy !== policy
  )
    return { status: 'invalid' };
  if (retry && !retry.current()) return { status: 'stopped' };
  if (typeof window === 'undefined' || !navigator.locks?.request)
    return { status: 'unavailable' };
  try {
    return await navigator.locks.request(
      'harvestcircle:owner:' + original.owner,
      { mode: 'exclusive', ifAvailable: true },
      async (lock): Promise<ListingOwnershipResult<T>> => {
        if (!lock) return { status: 'busy' };
        if (!(await recheckDeliveryOwner(original.identity, retry?.signal)))
          return { status: 'stopped' };
        const operation =
            signed &&
            identityPublicOperationOwnership(
              original.identity,
              signed.operation
            ),
          // A signed public update retains its original public operation.
          // New-listing inbox readiness stays in original.current(); edits
          // do not require a messaging probe. Resumptions keep their gate.
          identity = signed
            ? operation
            : identityMessagingOwnership(original.identity);
        if (
          !identity ||
          (signed && !operation) ||
          identity.owner !== original.owner ||
          (operation &&
            (operation.owner !== original.owner ||
              operation.session !== identity.session)) ||
          !original.current() ||
          (retry && !retry.current())
        )
          return { status: 'stopped' };
        let active = true;
        const owner: Owner = {
          original,
          policy,
          signal: retry?.signal,
          current: () =>
            active &&
            original.current() &&
            identity.current() &&
            (!operation || operation.current()) &&
            (!retry || retry.current())
        };
        const lease = Object.freeze({}) as ListingDeliveryLease;
        leases.set(lease, owner);
        try {
          if (!(await readOwnedListingDelivery(lease)))
            return {
              status: retry && !retry.current() ? 'stopped' : 'unavailable'
            };
          return { status: 'completed', value: await work(lease) };
        } finally {
          active = false;
        }
      }
    );
  } catch {
    return { status: 'unavailable' };
  }
}
// Each permission comes from actual stored signed bytes, fresh identity and
// unchanged original policy. Preference/private capabilities cannot enter it.
export async function authorizeListingAttempt(
  lease: ListingDeliveryLease,
  origin: string,
  mode: 'publish' | 'readback'
): Promise<ListingAttemptPermission | undefined> {
  const owner = leases.get(lease);
  if (!owner || !owner.current() || (mode !== 'publish' && mode !== 'readback'))
    return;
  try {
    const record = await readOwnedListingDelivery(lease),
      original = owner.original;
    const row =
      record && publicRecordSnapshot(record, original.owner, original.command);
    const manifest = readRelayPolicy(owner.policy),
      targets = publicRelayTargets(owner.policy, 'write'),
      fingerprint = await preferencePolicyFingerprint(owner.policy);
    if (
      row?.family !== 'public_operation' ||
      !row.artifact ||
      !manifest.postingEnabled ||
      !manifest.messagingEnabled ||
      fingerprint !== row.capture.policyFingerprint ||
      JSON.stringify(targets) !== JSON.stringify(row.capture.targets) ||
      !row.capture.targets.includes(origin)
    )
      return;
    if (!(await recheckDeliveryOwner(original.identity, owner.signal))) return;
    if (!owner.current()) return;
    const token = Object.freeze({}) as ListingAttemptPermission;
    permissions.set(token, { owner, origin, mode, wire: row.artifact.wire });
    return token;
  } catch {
    return;
  }
}
// Original once-only token: detached metadata, wrong role, policy or origin
// cannot supply a raw event. SDK effects remain owned by the public pool.
export function takeListingPublication(
  token: ListingAttemptPermission,
  policy: RelayPolicy,
  origin: string,
  mode: 'publish' | 'readback'
):
  | Readonly<{ event: NostrEvent; wire: string; current(): boolean }>
  | undefined {
  const saved = permissions.get(token);
  if (!saved) return;
  permissions.delete(token);
  if (
    saved.owner.policy !== policy ||
    saved.origin !== origin ||
    saved.mode !== mode ||
    !saved.owner.current()
  )
    return;
  return {
    event: JSON.parse(saved.wire) as NostrEvent,
    wire: saved.wire,
    current: saved.owner.current
  };
}
