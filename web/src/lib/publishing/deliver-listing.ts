import { PUBLIC_PUBLICATION_BUDGETS } from '../config/budgets.ts';
import type { RelayPolicy } from '../config/relays.ts';
import type { ListingPublication } from './sign-listing.ts';
import {
  listingRetryOwnership,
  type ListingRetry,
  type ListingRetrySource
} from './retry-listing.ts';
import {
  authorizeListingAttempt,
  listingDeliveryOwnership,
  readOwnedListingDelivery,
  readRetainedListingDelivery,
  withListingDeliveryOwnership,
  type ListingDeliveryLease
} from '../nostr/listing-publisher.ts';
import {
  publishPublicListingAttempt,
  readPublicListingExactAttempt,
  type PublicPool
} from '../nostr/public-pool.ts';
import { publicRecordSnapshot } from '../persistence/records.ts';
import { commitPublicListingReceipt } from '../persistence/public-receipts.ts';
export type ListingDeliveryResult = Readonly<{
  status:
    | 'completed'
    | 'needs_action'
    | 'storage_failed'
    | 'invalid'
    | 'unavailable'
    | 'busy'
    | 'stopped';
}>;
const pending = new WeakMap<
  ListingRetrySource,
  Promise<ListingDeliveryResult>
>();
async function deliver(
  lease: ListingDeliveryLease,
  pool: PublicPool,
  policy: RelayPolicy,
  signal?: AbortSignal
): Promise<ListingDeliveryResult> {
  const custody = listingDeliveryOwnership(lease),
    initial = await readOwnedListingDelivery(lease);
  if (!custody || !initial)
    return {
      status:
        signal?.aborted || (custody && !custody.current())
          ? 'stopped'
          : 'storage_failed'
    };
  const row = publicRecordSnapshot(initial, custody.owner, custody.command);
  if (row?.family !== 'public_operation' || !row.artifact)
    return { status: 'invalid' };
  const eventId = row.artifact.eventId;
  const actionId = crypto.randomUUID(),
    controller = new AbortController(),
    started = performance.now();
  const stop = () => controller.abort();
  signal?.addEventListener('abort', stop, { once: true });
  if (signal?.aborted) stop();
  const remaining = () =>
    PUBLIC_PUBLICATION_BUDGETS.networkActionMilliseconds -
    (performance.now() - started);
  async function retain(
    origin: string,
    attempt: number,
    status: string,
    readbackWire: string | null
  ) {
    const current = await readRetainedListingDelivery(lease);
    if (!current || !custody) return false;
    const result = await commitPublicListingReceipt(
      custody.repository,
      current,
      custody.command,
      JSON.stringify({
        actionId,
        origin,
        role: 'publication',
        attempt,
        eventId,
        status,
        observedAtMilliseconds: Date.now(),
        readbackWire
      })
    );
    return result.status === 'saved';
  }
  try {
    for (const origin of row.capture.targets) {
      if (
        row.receipts.some(
          (fact) =>
            fact.origin === origin &&
            fact.eventId === eventId &&
            fact.readbackWire !== null
        )
      )
        continue;
      const accepted = row.receipts.some(
        (fact) =>
          fact.origin === origin &&
          fact.eventId === row.artifact!.eventId &&
          fact.status === 'accepted'
      );
      let done = accepted;
      for (
        let attempt = 1;
        attempt <= PUBLIC_PUBLICATION_BUDGETS.attemptsPerTargetAction;
        attempt++
      ) {
        if (!custody.current() || controller.signal.aborted)
          return { status: 'stopped' };
        if (remaining() <= 0) return { status: 'needs_action' };
        let status: 'accepted' | 'timed_out' = 'accepted';
        if (!accepted) {
          const permission = await authorizeListingAttempt(
            lease,
            origin,
            'publish'
          );
          if (!permission) return { status: 'stopped' };
          if (remaining() <= 0) return { status: 'needs_action' };
          const result = await publishPublicListingAttempt(
            pool,
            permission,
            policy,
            origin,
            controller.signal,
            remaining()
          );
          if (!(await retain(origin, attempt, result.status, null)))
            return { status: 'storage_failed' };
          if (result.status !== 'accepted' && result.status !== 'timed_out')
            return {
              status: result.status === 'stopped' ? 'stopped' : 'needs_action'
            };
          status = result.status;
          done = result.status === 'accepted';
        }
        const permission = await authorizeListingAttempt(
          lease,
          origin,
          'readback'
        );
        if (!permission) return { status: 'stopped' };
        if (remaining() <= 0) return { status: 'needs_action' };
        const readback = await readPublicListingExactAttempt(
          pool,
          permission,
          policy,
          origin,
          controller.signal,
          remaining()
        );
        if (readback.status === 'found') {
          if (!(await retain(origin, attempt, status, readback.wire)))
            return { status: 'storage_failed' };
          done = true;
        }
        if (readback.status === 'stopped') return { status: 'stopped' };
        if (done) break;
      }
      if (!done) return { status: 'needs_action' };
    }
    return { status: 'completed' };
  } finally {
    controller.abort();
    signal?.removeEventListener('abort', stop);
  }
}
// The public control is opaque: no caller-supplied AbortSignal, raw event,
// repository or destination can substitute for the original retry custody.
export function deliverControlledListing(
  control: ListingRetry
): Promise<ListingDeliveryResult> {
  const original = listingRetryOwnership(control);
  if (!original) return Promise.resolve({ status: 'invalid' });
  if (!original.current()) return Promise.resolve({ status: 'stopped' });
  if (pending.has(original.source)) return Promise.resolve({ status: 'busy' });
  const job = withListingDeliveryOwnership(
    original.source,
    original.policy,
    original.consent,
    (lease) => deliver(lease, original.pool, original.policy, original.signal),
    control
  )
    .then((result) =>
      result.status === 'completed' ? result.value : { status: result.status }
    )
    .finally(() => {
      if (pending.get(original.source) === job) pending.delete(original.source);
    });
  pending.set(original.source, job);
  return job;
}
export function deliverListing(
  publication: ListingPublication,
  pool: PublicPool,
  policy: RelayPolicy,
  consent: unknown
): Promise<ListingDeliveryResult> {
  if (consent !== 'publish_stored_listing')
    return Promise.resolve({ status: 'invalid' });
  const active = pending.get(publication);
  if (active) return active;
  const job = withListingDeliveryOwnership(
    publication,
    policy,
    consent,
    (lease) => deliver(lease, pool, policy)
  )
    .then((result) =>
      result.status === 'completed' ? result.value : { status: result.status }
    )
    .finally(() => {
      if (pending.get(publication) === job) pending.delete(publication);
    });
  pending.set(publication, job);
  return job;
}
