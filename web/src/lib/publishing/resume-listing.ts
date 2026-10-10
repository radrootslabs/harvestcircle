import {
  publicPublicationOwner,
  readPublicListingIntent,
  type PublicPublicationRepository
} from '../persistence/public-publications.ts';
import {
  decodePublicRecord,
  publicRecordSnapshot
} from '../persistence/records.ts';
import { canonicalLocalId } from '../private-handles.ts';
import {
  identityMessagingOwnership,
  recheckIdentityOwner,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  publicRelayTargets,
  readRelayPolicy,
  type RelayPolicy
} from '../config/relays.ts';
import { preferencePolicyFingerprint } from '../nostr/inbox-preference-publication.ts';
import type { PublicPool } from '../nostr/public-pool.ts';
import { beginListingRetry } from './retry-listing.ts';

declare const resumeBrand: unique symbol;
export type ListingResumption = Readonly<{ [resumeBrand]: true }>;
type Ownership = Readonly<{
  repository: PublicPublicationRepository;
  identity: IdentitySession;
  policy: RelayPolicy;
  owner: string;
  command: string;
  recordWire: string;
  current(): boolean;
}>;
const resumptions = new WeakMap<ListingResumption, Ownership>();
export function listingResumeOwnership(token: ListingResumption) {
  const original = resumptions.get(token);
  return original?.current() ? original : undefined;
}
export function listingResumeSnapshot(token: ListingResumption) {
  const original = listingResumeOwnership(token);
  if (!original) return;
  const decoded = decodePublicRecord(
    original.recordWire,
    original.owner,
    original.command
  );
  const row =
    decoded.ok &&
    publicRecordSnapshot(decoded.value, original.owner, original.command);
  if (!row || row.family !== 'public_operation' || !row.artifact) return;
  return {
    owner: row.owner,
    command: row.id,
    hash: row.artifact.eventId,
    wire: row.artifact.wire,
    targets: row.capture.targets.slice(),
    policyFingerprint: row.capture.policyFingerprint
  };
}
export async function prepareListingResume(
  repository: PublicPublicationRepository,
  identity: IdentitySession,
  policy: RelayPolicy,
  command: unknown,
  consent: unknown
): Promise<
  | Readonly<{ status: 'ready'; resume: ListingResumption }>
  | Readonly<{ status: 'invalid' | 'unavailable' | 'needs_action' }>
> {
  const owner = publicPublicationOwner(repository),
    id = canonicalLocalId(command);
  if (
    !owner ||
    !id ||
    !identityMessagingOwnership(identity) ||
    consent !== 'reviewed_stored_listing_resume'
  )
    return { status: 'invalid' };
  if (typeof window === 'undefined' || !navigator.locks?.request)
    return { status: 'unavailable' };
  try {
    await recheckIdentityOwner(identity);
    const capture = identityMessagingOwnership(identity);
    if (!capture || capture.owner !== owner) return { status: 'needs_action' };
    const current = () =>
      capture.current() && publicPublicationOwner(repository) === owner;
    const read = await readPublicListingIntent(repository, id);
    const row =
      read.status === 'found' && publicRecordSnapshot(read.record, owner, id);
    const manifest = readRelayPolicy(policy),
      targets = publicRelayTargets(policy, 'write'),
      fingerprint = await preferencePolicyFingerprint(policy);
    if (
      !current() ||
      !row ||
      row.family !== 'public_operation' ||
      row.capture.kind !== 30402 ||
      !row.artifact ||
      row.revision < 2 ||
      !manifest.postingEnabled ||
      !manifest.messagingEnabled ||
      fingerprint !== row.capture.policyFingerprint ||
      JSON.stringify(targets) !== JSON.stringify(row.capture.targets)
    )
      return { status: 'needs_action' };
    // Normalize receipt bookkeeping only. Every original capture field and the
    // independently verified stored signature remain exact; no signer is minted.
    const recordWire = JSON.stringify({ ...row, revision: 2, receipts: [] });
    if (!decodePublicRecord(recordWire, owner, id).ok || !current())
      return { status: 'needs_action' };
    const resume = Object.freeze({}) as ListingResumption;
    resumptions.set(resume, {
      repository,
      identity,
      policy,
      owner,
      command: id,
      recordWire,
      current
    });
    return { status: 'ready', resume };
  } catch {
    return { status: 'needs_action' };
  }
}
export function resumeListing(
  token: ListingResumption,
  pool: PublicPool,
  consent: unknown
) {
  const original = listingResumeOwnership(token);
  if (!original || consent !== 'resume_stored_listing') return;
  return beginListingRetry(token, pool, original.policy, consent);
}
