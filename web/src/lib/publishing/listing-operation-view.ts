import { canonicalLocalId } from '../private-handles.ts';
import {
  publicPublicationOwner,
  readPublicListingIntent,
  type PublicPublicationRepository
} from '../persistence/public-publications.ts';
import { publicRecordSnapshot } from '../persistence/records.ts';
import {
  identitySessionSnapshot,
  subscribeIdentityInvalidation,
  type IdentitySession
} from '../runtime/identity-session.ts';
import {
  listingSigningSnapshot,
  subscribeListingSigning,
  type ListingSigning
} from './sign-listing.ts';
import { inspectLocalTemplate } from '../nostr/local-template.ts';
import { projectFoodParts } from '../contracts/food-availability-v1/read.ts';
import type { PublicTargetReceipt } from '../contracts/local-records.ts';
import type { StatusCopy } from '../presentation/status-copy.ts';

declare const viewBrand: unique symbol;
export type ListingOperationView = Readonly<{ [viewBrand]: true }>;
export type ListingOperationSnapshot = Readonly<{
  owner: string;
  command: string;
  revision: number;
  eventId: string;
  purpose: 'listing' | 'withdrawal';
  artifact: 'unsigned' | 'signed';
  state:
    | 'prepared'
    | 'signing'
    | 'partial'
    | 'accepted'
    | 'readback'
    | 'unknown'
    | 'needs-action';
  acceptanceCount: number;
  readbackCount: number;
  needsAction: boolean;
  localSource: 'retained-prototype' | 'read-only';
  summary: StatusCopy;
  targets: readonly Readonly<{
    origin: string;
    eventId: string;
    accepted: boolean;
    readback: 'confirmed' | 'not-observed';
    latestOutcome: PublicTargetReceipt['status'] | 'unattempted';
    receipts: readonly PublicTargetReceipt[];
  }>[];
}>;
type View = Readonly<{
  wire: string;
  current(): boolean;
  retire(): void;
  subscribe(listener: () => void): () => void;
}>;
const views = new WeakMap<ListingOperationView, View>();
const active = new WeakMap<
  PublicPublicationRepository,
  Map<string, Readonly<{ stamp: symbol; retire(): void }>>
>();

// This is account-local presentation only. Reading a retained operation grants
// no sign, send, edit, current-head or relay qualification capability.
export async function readListingOperationView(
  repository: PublicPublicationRepository,
  identity: IdentitySession,
  command: unknown,
  signing?: ListingSigning
): Promise<
  | Readonly<{ status: 'found'; view: ListingOperationView }>
  | Readonly<{ status: 'unavailable' }>
> {
  const owner = publicPublicationOwner(repository),
    id = canonicalLocalId(command);
  const identityOwner = () => {
    const snapshot = identitySessionSnapshot(identity);
    return 'publicKey' in snapshot ? snapshot.publicKey : undefined;
  };
  if (!owner || !id || identityOwner() !== owner)
    return { status: 'unavailable' };
  const cache =
    active.get(repository) ??
    new Map<string, Readonly<{ stamp: symbol; retire(): void }>>();
  active.set(repository, cache);
  cache.get(id)?.retire();
  const stamp = Symbol(),
    listeners = new Map<symbol, () => void>();
  let alive = true,
    unsubscribe = () => {},
    unsubscribeSigning = () => {},
    observesSigning = false;
  function retire() {
    if (!alive) return;
    alive = false;
    unsubscribe();
    unsubscribeSigning();
    if (cache.get(id!)?.stamp === stamp) cache.delete(id!);
    for (const listener of listeners.values()) {
      try {
        listener();
      } catch {
        /* Presentation callbacks cannot retain original custody. */
      }
    }
    listeners.clear();
  }
  function current() {
    const live =
      observesSigning && signing ? listingSigningSnapshot(signing) : undefined;
    const originalSigningCurrent =
      !observesSigning ||
      !!(
        live &&
        live.signing &&
        !live.stopped &&
        live.owner === owner &&
        live.command === id
      );
    return (
      originalSigningCurrent &&
      alive &&
      cache.get(id!)?.stamp === stamp &&
      publicPublicationOwner(repository) === owner &&
      identityOwner() === owner
    );
  }
  cache.set(id, { stamp, retire });
  unsubscribe = subscribeIdentityInvalidation(identity, retire);
  try {
    const read = await readPublicListingIntent(repository, id);
    const row =
      read.status === 'found' && publicRecordSnapshot(read.record, owner, id);
    if (
      !current() ||
      !row ||
      row.family !== 'public_operation' ||
      !(
        row.capture.kind === 30402 ||
        (row.capture.kind === 5 && row.source.type === 'public_head')
      ) ||
      row.capture.targets.length === 0 ||
      (!row.artifact && (row.revision > 1 || row.receipts.length !== 0)) ||
      (row.artifact && row.revision < 2)
    ) {
      retire();
      return { status: 'unavailable' };
    }
    const template = inspectLocalTemplate(
      row.capture.wire,
      owner,
      row.capture.kind
    );
    if (
      !template ||
      (template.kind === 30402 &&
        projectFoodParts(template).outcome !== 'focused')
    ) {
      retire();
      return { status: 'unavailable' };
    }
    const targets = row.capture.targets.map((origin) => {
      const receipts = row.receipts.filter(
        (receipt) =>
          receipt.origin === origin && receipt.eventId === row.capture.hash
      );
      return {
        origin,
        eventId: row.capture.hash,
        accepted: receipts.some((receipt) => receipt.status === 'accepted'),
        readback: receipts.some((receipt) => receipt.readbackWire !== null)
          ? ('confirmed' as const)
          : ('not-observed' as const),
        latestOutcome: receipts.at(-1)?.status ?? ('unattempted' as const),
        receipts
      };
    });
    const acceptanceCount = targets.filter((target) => target.accepted).length,
      readbackCount = targets.filter(
        (target) => target.readback === 'confirmed'
      ).length;
    const needsAction = targets.some((target) =>
      ['refused', 'timed_out', 'unknown', 'stopped'].includes(
        target.latestOutcome
      )
    );
    const live = signing && listingSigningSnapshot(signing);
    const genuinelySigning =
      live &&
      live.owner === owner &&
      live.command === id &&
      live.hash === row.capture.hash &&
      live.signing &&
      !live.stopped;
    const state: ListingOperationSnapshot['state'] = !row.artifact
      ? row.revision === 0
        ? 'prepared'
        : genuinelySigning
          ? 'signing'
          : 'unknown'
      : readbackCount === targets.length
        ? 'readback'
        : acceptanceCount === targets.length
          ? 'accepted'
          : acceptanceCount > 0 || readbackCount > 0
            ? 'partial'
            : targets.some(
                  (target) =>
                    target.latestOutcome === 'refused' ||
                    target.latestOutcome === 'stopped'
                )
              ? 'needs-action'
              : row.receipts.length > 0
                ? 'unknown'
                : 'prepared';
    observesSigning = state === 'signing';
    if (observesSigning && signing)
      unsubscribeSigning = subscribeListingSigning(signing, retire);
    const listingText =
      state === 'signing'
        ? 'Waiting for your extension to sign the retained listing.'
        : state === 'unknown'
          ? 'The outcome is unknown. Keep the original listing operation for reconciliation.'
          : state === 'needs-action'
            ? 'Needs action. Keep the original listing operation and its recorded target outcomes.'
            : state === 'partial'
              ? 'Publication evidence is partial. Check the individual target outcomes.'
              : state === 'readback'
                ? 'Exact listing read-back is recorded for each frozen target.'
                : state === 'accepted'
                  ? 'Each frozen target accepted this listing event. Exact read-back is recorded separately.'
                  : row.artifact
                    ? 'Signed listing saved in this browser. No relay acceptance is recorded.'
                    : 'Saved in this browser. The retained listing has not been signed.';
    const text =
      row.capture.kind === 5
        ? listingText
            .replaceAll('listing event', 'removal request')
            .replaceAll('listing', 'removal request') +
          ' This request cannot erase all other copies.'
        : listingText;
    const snapshot: ListingOperationSnapshot = {
      owner,
      command: id,
      revision: row.revision,
      eventId: row.capture.hash,
      purpose: row.capture.kind === 5 ? 'withdrawal' : 'listing',
      artifact: row.artifact ? 'signed' : 'unsigned',
      state,
      acceptanceCount,
      readbackCount,
      needsAction,
      localSource:
        row.artifact && row.source.type === 'draft'
          ? 'retained-prototype'
          : 'read-only',
      summary: {
        tone:
          needsAction || state === 'unknown' || state === 'partial'
            ? 'warning'
            : 'info',
        text
      },
      targets
    };
    if (!current()) {
      retire();
      return { status: 'unavailable' };
    }
    const view = Object.freeze({}) as ListingOperationView;
    views.set(view, {
      wire: JSON.stringify(snapshot),
      current,
      retire,
      subscribe(listener) {
        if (!current()) return () => {};
        const key = Symbol();
        listeners.set(key, listener);
        return () => {
          listeners.delete(key);
        };
      }
    });
    return { status: 'found', view };
  } catch {
    retire();
    return { status: 'unavailable' };
  }
}
export function listingOperationSnapshot(
  token: ListingOperationView
): ListingOperationSnapshot | undefined {
  const value = views.get(token);
  if (!value) return;
  if (!value.current()) {
    value.retire();
    return;
  }
  return JSON.parse(value.wire) as ListingOperationSnapshot;
}
export function closeListingOperationView(token: ListingOperationView): void {
  views.get(token)?.retire();
}
export function subscribeListingOperationView(
  token: ListingOperationView,
  listener: () => void
): () => void {
  const value = views.get(token);
  if (!value?.current()) return () => {};
  return value.subscribe(listener);
}
