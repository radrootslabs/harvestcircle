import { buildFoodTemplate } from '../contracts/food-availability-v1/write.ts';
import type {
  PublicDraftRecord,
  PublicDraftForm
} from '../contracts/local-records.ts';
import { canonicalLocalId, newLocalId } from '../private-handles.ts';
import { readRelayPolicy, publicRelayTargets } from '../config/relays.ts';
import { inspectLocalTemplate } from '../nostr/local-template.ts';
import { operationEnabled } from '../config/operation-flags.ts';
import {
  verifyEnvelope,
  verifiedEnvelopeSnapshot
} from '../nostr/verified-envelope.ts';
import {
  decodePublicRecord,
  type PublicRecordHandle
} from '../persistence/records.ts';
import {
  publicPublicationOwner,
  publicPublicationDraftRepository,
  savePublicListingIntent,
  type PublicPublicationRepository,
  type PublicListingSave
} from '../persistence/public-publications.ts';
import {
  publicDraftControllerOwnership,
  publicDraftControllerSnapshot,
  flushPublicDraft,
  type PublicDraftController
} from './draft-controller.ts';
import {
  newListingReadinessOwnership,
  captureOwnedListingReadiness,
  closeNewListingReadiness,
  type NewListingReadiness
} from './new-listing-readiness.ts';
import {
  authorizeOwnedEditSource,
  type OwnedEditSource
} from './edit-source.ts';
import {
  assessFutureTimestamp,
  wallClockAvailable
} from '../catalog/clock-policy.ts';

declare const reviewBrand: unique symbol;
export type ListingReview = Readonly<{ [reviewBrand]: true }>;
export type ListingPreview = Readonly<{
  owner: string;
  command: string;
  draftId: string;
  draftRevision: number;
  identifier: string;
  createdAtSeconds: number;
  form: PublicDraftForm;
  summary: string;
  templateWire: string;
  expectedHash: string;
  targets: readonly string[];
  policyFingerprint: string;
  withdrawal?: Readonly<{ coordinate: string; eventId: string }>;
  revision?: Readonly<{
    coordinate: string;
    publishedAtSeconds: number;
    status: 'active' | 'sold';
  }>;
}>;
type Review = Readonly<{
  repository: PublicPublicationRepository;
  controller: PublicDraftController;
  expected: PublicDraftRecord;
  editVersion: number;
  command: string;
  readiness: NewListingReadiness;
  source?: OwnedEditSource;
  status?: 'active' | 'sold' | 'withdrawal';
  record: PublicRecordHandle;
  revisionBase?: Readonly<{ command: string; record: PublicRecordHandle }>;
  previewWire: string;
  current(this: void): boolean;
}>;
const reviews = new WeakMap<ListingReview, Review>();
const commands = new WeakMap<
  PublicPublicationRepository,
  Map<string, ListingReview>
>();
const activeReservations = new WeakMap<
  ListingReview,
  Promise<ListingReservation>
>();
export type ListingReviewResult =
  | Readonly<{ status: 'review'; review: ListingReview }>
  | Readonly<{
      status: 'blocked';
      reason:
        | 'invalid_scope'
        | 'invalid_command'
        | 'unavailable'
        | 'invalid_form'
        | 'conflict';
    }>;
export type ListingReservation =
  PublicListingSave | Readonly<{ status: 'invalid' }>;
// Review flushes local draft state and prepares an exact immutable template;
// it neither reserves a publication nor asks the extension to sign.
export async function reviewNewListing(
  repository: PublicPublicationRepository,
  controller: PublicDraftController,
  readiness: NewListingReadiness,
  command: unknown,
  consent: unknown
): Promise<ListingReviewResult> {
  return reviewListing(repository, controller, readiness, command, consent);
}
// Pure declared clock policy; production supplies actual Date.now below.
export function ownedRevisionTimestamp(
  head: number,
  now: number
): number | undefined {
  if (
    !Number.isSafeInteger(head) ||
    head < 0 ||
    head >= Number.MAX_SAFE_INTEGER ||
    !wallClockAvailable(now)
  )
    return;
  const next = Math.max(head + 1, Math.floor(now));
  return assessFutureTimestamp(next, now) === 'within_policy'
    ? next
    : undefined;
}
export async function reviewOwnedListing(
  source: OwnedEditSource,
  controller: PublicDraftController,
  command: unknown,
  status: 'active' | 'sold' | 'withdrawal',
  consent: unknown
): Promise<ListingReviewResult> {
  const original = authorizeOwnedEditSource(source);
  if (!original) return { status: 'blocked', reason: 'invalid_scope' };
  return reviewListing(
    original.repository,
    controller,
    undefined,
    command,
    consent,
    { source, status }
  );
}
async function reviewListing(
  repository: PublicPublicationRepository,
  controller: PublicDraftController,
  readiness: NewListingReadiness | undefined,
  command: unknown,
  consent: unknown,
  revision?: Readonly<{
    source: OwnedEditSource;
    status: 'active' | 'sold' | 'withdrawal';
  }>
): Promise<ListingReviewResult> {
  try {
    const custody = publicDraftControllerOwnership(controller),
      owner = publicPublicationOwner(repository);
    if (
      !custody ||
      !owner ||
      custody.repository !== publicPublicationDraftRepository(repository)
    )
      return { status: 'blocked', reason: 'invalid_scope' };
    const id = canonicalLocalId(command);
    if (!id) return { status: 'blocked', reason: 'invalid_command' };
    const retained = revision && authorizeOwnedEditSource(revision.source);
    const ready = revision ? retained : newListingReadinessOwnership(readiness);
    if (
      !ready ||
      ready.owner !== owner ||
      !ready.current() ||
      consent !==
        (revision
          ? revision.status === 'withdrawal'
            ? 'reviewed_owned_withdrawal'
            : revision.status === 'sold'
              ? 'reviewed_sold_availability'
              : 'reviewed_owned_revision'
          : 'reviewed_new_listing')
    )
      return { status: 'blocked', reason: 'unavailable' };
    if (!(await flushPublicDraft(controller)))
      return { status: 'blocked', reason: 'unavailable' };
    const snapshot = publicDraftControllerSnapshot(controller),
      expected = snapshot?.draft;
    if (
      !snapshot ||
      !expected ||
      expected.owner !== owner ||
      !custody.current(JSON.stringify(expected), snapshot.editVersion)
    )
      return { status: 'blocked', reason: 'conflict' };
    const cached = commands.get(repository)?.get(id);
    if (cached) {
      const original = reviews.get(cached);
      if (
        original &&
        original.controller === controller &&
        (revision
          ? original.source === revision.source &&
            original.status === revision.status
          : original.readiness === readiness) &&
        JSON.stringify(original.expected) === JSON.stringify(expected) &&
        original.current() &&
        custody.current(JSON.stringify(expected), snapshot.editVersion)
      ) {
        return { status: 'review', review: cached };
      }
      return { status: 'blocked', reason: 'conflict' };
    }
    const baseProof = retained && verifyEnvelope(retained.baseWire);
    const base = baseProof?.ok
      ? verifiedEnvelopeSnapshot(baseProof.value)
      : undefined;
    const identifier = revision
      ? base?.tags.find((tag) => tag[0] === 'd')?.[1]
      : newLocalId();
    const now = Math.floor(Date.now() / 1000);
    const createdAtSeconds = revision
      ? base && ownedRevisionTimestamp(base.created_at, now)
      : now;
    if (!identifier || createdAtSeconds === undefined)
      return {
        status: 'blocked',
        reason: revision ? 'conflict' : 'unavailable'
      };
    const form = expected.form;
    const withdrawal = revision?.status === 'withdrawal';
    if (!withdrawal && form.contactType === '' && form.contactValue !== '')
      return { status: 'blocked', reason: 'invalid_form' };
    const contact =
      form.contactType === ''
        ? undefined
        : {
            type: form.contactType,
            value: form.contactValue,
            public: true as const
          };
    const built = buildFoodTemplate({
      identifier,
      title: form.title,
      description: form.description,
      location: form.location,
      amount: form.amount,
      currency: form.currency,
      unit: form.unit,
      ...(form.quantity === '' ? {} : { quantity: form.quantity }),
      created_at: createdAtSeconds,
      published_at: retained ? retained.publishedAtSeconds : createdAtSeconds,
      status: revision?.status === 'sold' ? 'sold' : 'active',
      ...(contact ? { contact } : {})
    });
    if (!withdrawal && !built.ok)
      return { status: 'blocked', reason: 'invalid_form' };
    const manifest = readRelayPolicy(ready.policy),
      targets = publicRelayTargets(ready.policy, 'write');
    const templateWire = JSON.stringify({
      pubkey: owner,
      kind: withdrawal ? 5 : 30402,
      created_at: createdAtSeconds,
      tags:
        withdrawal && base
          ? [
              ['e', base.id],
              ['a', `30402:${owner}:${identifier}`],
              ['k', '30402']
            ]
          : built.ok
            ? built.wire_parts.tags
            : [],
      content: withdrawal ? '' : built.ok ? built.wire_parts.content : ''
    });
    const template = inspectLocalTemplate(
      templateWire,
      owner,
      withdrawal ? 5 : 30402
    );
    if (
      !template ||
      !operationEnabled(ready.policy, retained ? 'posting' : 'new_listing') ||
      targets.length === 0
    )
      return { status: 'blocked', reason: 'unavailable' };
    const digest = await crypto.subtle.digest(
      'SHA-256',
      new TextEncoder().encode(JSON.stringify(manifest))
    );
    const policyFingerprint = Array.from(new Uint8Array(digest), (value) =>
      value.toString(16).padStart(2, '0')
    ).join('');
    // Hashing is outside any IDB transaction; recheck original mutable owners.
    if (
      !ready.current() ||
      publicPublicationOwner(repository) !== owner ||
      !custody.current(JSON.stringify(expected), snapshot.editVersion)
    )
      return { status: 'blocked', reason: 'conflict' };
    // Another review may have completed while this one awaited hashing. The
    // first retained command owns its ID/time/template; never replace it.
    const racing = commands.get(repository)?.get(id);
    if (racing) {
      const original = reviews.get(racing);
      if (
        original &&
        original.controller === controller &&
        (revision
          ? original.source === revision.source &&
            original.status === revision.status
          : original.readiness === readiness) &&
        JSON.stringify(original.expected) === JSON.stringify(expected) &&
        original.current() &&
        custody.current(JSON.stringify(expected), snapshot.editVersion)
      )
        return { status: 'review', review: racing };
      return { status: 'blocked', reason: 'conflict' };
    }
    const admitted = decodePublicRecord(
      JSON.stringify({
        schema: 1,
        family: 'public_operation',
        owner,
        id,
        revision: 0,
        source:
          withdrawal && retained
            ? { type: 'public_head', wire: retained.baseWire }
            : { type: 'draft', id: expected.id, revision: expected.revision },
        capture: {
          kind: withdrawal ? 5 : 30402,
          wire: templateWire,
          hash: template.hash,
          targets,
          policyFingerprint
        },
        artifact: null,
        receipts: []
      }),
      owner,
      id
    );
    if (!admitted.ok) return { status: 'blocked', reason: 'invalid_form' };
    const proof = revision
      ? captureOwnedListingReadiness(
          revision.source,
          template.hash,
          withdrawal ? 'reviewed_owned_withdrawal' : 'reviewed_owned_listing'
        )
      : readiness;
    const finalReady = newListingReadinessOwnership(proof);
    if (!proof || !finalReady || !finalReady.current())
      return { status: 'blocked', reason: 'conflict' };
    const review = Object.freeze({}) as ListingReview;
    reviews.set(review, {
      repository,
      controller,
      expected,
      editVersion: snapshot.editVersion,
      command: id,
      readiness: proof,
      ...(revision ? { source: revision.source, status: revision.status } : {}),
      record: admitted.value,
      ...(retained
        ? {
            revisionBase: {
              command: retained.sourceCommand,
              record: retained.sourceRecord
            }
          }
        : {}),
      previewWire: JSON.stringify({
        owner,
        command: id,
        draftId: expected.id,
        draftRevision: expected.revision,
        identifier,
        createdAtSeconds,
        form,
        summary: withdrawal ? 'Removal request' : built.ok ? built.summary : '',
        templateWire,
        expectedHash: template.hash,
        targets,
        policyFingerprint,
        ...(withdrawal && retained && base
          ? {
              withdrawal: { coordinate: retained.coordinate, eventId: base.id }
            }
          : {}),
        ...(retained && revision && !withdrawal
          ? {
              revision: {
                coordinate: retained.coordinate,
                publishedAtSeconds: retained.publishedAtSeconds,
                status: revision.status
              }
            }
          : {})
      }),
      current: () =>
        finalReady.current() &&
        publicPublicationOwner(repository) === owner &&
        (!revision ||
          assessFutureTimestamp(
            createdAtSeconds,
            Math.floor(Date.now() / 1000)
          ) === 'within_policy')
    });
    const cache = commands.get(repository) ?? new Map<string, ListingReview>();
    cache.set(id, review);
    commands.set(repository, cache);
    return { status: 'review', review };
  } catch {
    return { status: 'blocked', reason: 'unavailable' };
  }
}
export function listingReviewSnapshot(
  review: ListingReview
): ListingPreview | undefined {
  const value = reviews.get(review);
  return value ? (JSON.parse(value.previewWire) as ListingPreview) : undefined;
}
export function closeListingReview(review: ListingReview): void {
  const original = reviews.get(review);
  if (original) closeNewListingReadiness(original.readiness);
}
// Internal original custody, not a signing or transport permission. The later
// controller must reread the persisted intent and obtain its own effect lease.
export function listingReviewOwnership(review: ListingReview) {
  const original = reviews.get(review);
  const ready = original && newListingReadinessOwnership(original.readiness);
  return original && ready && original.current()
    ? {
        repository: original.repository,
        record: original.record,
        command: original.command,
        readiness: ready,
        current: original.current
      }
    : undefined;
}
// One original review deduplicates simultaneous clicks. Failed/uncertain work
// remains the same capture for later explicit retry; no fresh ID/time/template.
export function reserveNewListing(
  review: ListingReview,
  consent: unknown
): Promise<ListingReservation> {
  const value = reviews.get(review);
  if (!value || consent !== 'publish_reviewed_listing')
    return Promise.resolve({ status: 'invalid' });
  const active = activeReservations.get(review);
  if (active) return active;
  const work = savePublicListingIntent(
    value.repository,
    value.controller,
    value.record,
    value.expected,
    value.editVersion,
    value.command,
    value.current,
    value.revisionBase
  ).finally(() => {
    if (activeReservations.get(review) === work)
      activeReservations.delete(review);
  });
  activeReservations.set(review, work);
  return work;
}
