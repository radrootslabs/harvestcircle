import type { NostrEvent } from 'applesauce-core/helpers';
import {
  publicRelayTargets,
  readRelayPolicy,
  type RelayPolicy
} from '../config/relays.ts';
import {
  identityMessagingOwnership,
  identityPublicOperationOwnership,
  recheckIdentityOwner,
  type IdentitySession,
  type IdentityPublicOperation
} from '../runtime/identity-session.ts';
import {
  publicEffectLeaseCurrent,
  type PublicEffectLease
} from '../runtime/effect-ownership.ts';
import {
  loadPreferenceOperation,
  type PublicQuotaRepository
} from '../persistence/quota.ts';
import {
  publicRecordSnapshot,
  publicRecordWire,
  type PublicRecordHandle
} from '../persistence/records.ts';
declare const publicationBrand: unique symbol;
export type PreferencePublication = Readonly<{ [publicationBrand]: true }>;
type Saved = Readonly<{
  policy: RelayPolicy;
  wire: string;
  eventId: string;
  origins: readonly string[];
  current(): boolean;
}>;
const publications = new WeakMap<PreferencePublication, Saved>();
// Internal exact policy observation, not a Nostr cryptographic replacement.
export async function preferencePolicyFingerprint(
  policy: RelayPolicy
): Promise<string> {
  const hash = await crypto.subtle.digest(
    'SHA-256',
    new TextEncoder().encode(JSON.stringify(readRelayPolicy(policy)))
  );
  return Array.from(new Uint8Array(hash), (byte) =>
    byte.toString(16).padStart(2, '0')
  ).join('');
}
// Only a genuine current identity operation and original lease can acquire this
// one-attempt capability from actual acknowledged storage. No caller event wire.
// beforeEffect is an internal bounded lookup port; its final fence is retained.
export async function authorizeStoredInboxPreference(
  repository: PublicQuotaRepository,
  identity: IdentitySession,
  operation: IdentityPublicOperation,
  lease: PublicEffectLease,
  original: PublicRecordHandle,
  id: string,
  policy: RelayPolicy,
  beforeEffect: () => Promise<(() => boolean) | undefined>
): Promise<PreferencePublication | undefined> {
  try {
    if (typeof window === 'undefined') return undefined;
    const capture = identityPublicOperationOwnership(identity, operation);
    const messageOwner = identityMessagingOwnership(identity);
    if (
      !capture ||
      !messageOwner ||
      capture.owner !== messageOwner.owner ||
      capture.session !== messageOwner.session
    )
      return undefined;
    const originalWire = publicRecordWire(original, capture.owner, id),
      row = publicRecordSnapshot(original, capture.owner, id);
    if (
      !originalWire ||
      row?.family !== 'preference_operation' ||
      row.revision !== 0 ||
      row.capture.kind !== 10050
    )
      return undefined;
    const authority = {
      owner: capture.owner,
      id,
      recordWire: originalWire,
      session: capture.session
    };
    const fingerprint = await preferencePolicyFingerprint(policy),
      manifest = readRelayPolicy(policy),
      targets = publicRelayTargets(policy, 'write');
    if (
      !manifest.messagingEnabled ||
      !manifest.postingEnabled ||
      fingerprint !== row.capture.policyFingerprint ||
      JSON.stringify(targets) !== JSON.stringify(row.capture.targets)
    )
      return undefined;
    const read = await loadPreferenceOperation(repository, id);
    if (!read.ok) return undefined;
    const stored = publicRecordSnapshot(read.value, capture.owner, id);
    if (
      stored?.family !== 'preference_operation' ||
      stored.revision < 2 ||
      !stored.artifact ||
      JSON.stringify(stored.capture) !== JSON.stringify(row.capture) ||
      JSON.stringify(stored.source) !== JSON.stringify(row.source) ||
      stored.consent !== row.consent
    )
      return undefined;
    const headCurrent = await beforeEffect();
    if (!headCurrent) return undefined;
    await recheckIdentityOwner(identity);
    const current = () =>
      capture.current() &&
      messageOwner.current() &&
      headCurrent() &&
      publicEffectLeaseCurrent(lease, authority);
    if (!current()) return undefined;
    const token = Object.freeze({}) as PreferencePublication;
    publications.set(token, {
      policy,
      wire: stored.artifact.wire,
      eventId: stored.artifact.eventId,
      origins: [...stored.capture.targets],
      current
    });
    return token;
  } catch {
    return undefined;
  }
}
// Detached metadata never grants permission. Pool consumes the original token.
export function preferencePublicationSnapshot(token: PreferencePublication) {
  const saved = publications.get(token);
  return saved
    ? { eventId: saved.eventId, origins: [...saved.origins] }
    : undefined;
}
export function takePreferencePublication(
  token: PreferencePublication,
  policy: RelayPolicy,
  origin: string
): Readonly<{ event: NostrEvent; current(): boolean }> | undefined {
  const saved = publications.get(token);
  if (!saved) return undefined;
  publications.delete(token);
  if (
    saved.policy !== policy ||
    !saved.origins.includes(origin) ||
    !saved.current()
  )
    return undefined;
  return {
    event: JSON.parse(saved.wire) as NostrEvent,
    current: saved.current
  };
}
