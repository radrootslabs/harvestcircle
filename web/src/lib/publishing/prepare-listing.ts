import { buildFoodTemplate } from '../contracts/food-availability-v1/write.ts';
import type {
  PublicDraftRecord,
  PublicDraftForm
} from '../contracts/local-records.ts';
import { canonicalLocalId, newLocalId } from '../private-handles.ts';
import { readRelayPolicy, publicRelayTargets } from '../config/relays.ts';
import { inspectLocalTemplate } from '../nostr/local-template.ts';
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
  type NewListingReadiness
} from './new-listing-readiness.ts';

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
}>;
type Review = Readonly<{
  repository: PublicPublicationRepository;
  controller: PublicDraftController;
  expected: PublicDraftRecord;
  editVersion: number;
  command: string;
  readiness: NewListingReadiness;
  record: PublicRecordHandle;
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
    const ready = newListingReadinessOwnership(readiness);
    if (
      !ready ||
      ready.owner !== owner ||
      !ready.current() ||
      consent !== 'reviewed_new_listing'
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
        original.readiness === readiness &&
        JSON.stringify(original.expected) === JSON.stringify(expected) &&
        original.current() &&
        custody.current(JSON.stringify(expected), snapshot.editVersion)
      ) {
        return { status: 'review', review: cached };
      }
      return { status: 'blocked', reason: 'conflict' };
    }
    const identifier = newLocalId(),
      createdAtSeconds = Math.floor(Date.now() / 1000);
    if (!identifier) return { status: 'blocked', reason: 'unavailable' };
    const form = expected.form;
    if (form.contactType === '' && form.contactValue !== '')
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
      published_at: createdAtSeconds,
      status: 'active',
      ...(contact ? { contact } : {})
    });
    if (!built.ok) return { status: 'blocked', reason: 'invalid_form' };
    const manifest = readRelayPolicy(ready.policy),
      targets = publicRelayTargets(ready.policy, 'write');
    const templateWire = JSON.stringify({
      pubkey: owner,
      kind: 30402,
      created_at: createdAtSeconds,
      tags: built.wire_parts.tags,
      content: built.wire_parts.content
    });
    const template = inspectLocalTemplate(templateWire, owner, 30402);
    if (!template || !manifest.postingEnabled || targets.length === 0)
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
        original.readiness === readiness &&
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
        source: { type: 'draft', id: expected.id, revision: expected.revision },
        capture: {
          kind: 30402,
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
    const review = Object.freeze({}) as ListingReview;
    reviews.set(review, {
      repository,
      controller,
      expected,
      editVersion: snapshot.editVersion,
      command: id,
      readiness,
      record: admitted.value,
      previewWire: JSON.stringify({
        owner,
        command: id,
        draftId: expected.id,
        draftRevision: expected.revision,
        identifier,
        createdAtSeconds,
        form,
        summary: built.summary,
        templateWire,
        expectedHash: template.hash,
        targets,
        policyFingerprint
      }),
      current: () =>
        ready.current() && publicPublicationOwner(repository) === owner
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
    value.current
  ).finally(() => {
    if (activeReservations.get(review) === work)
      activeReservations.delete(review);
  });
  activeReservations.set(review, work);
  return work;
}
