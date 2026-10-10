import {
  exactLocalFields,
  type PublicDraftForm
} from '../contracts/local-records.ts';
import {
  buildFoodTemplate,
  type FoodDraftErrorCode
} from '../contracts/food-availability-v1/write.ts';
import type { InboxSetupView } from '../messaging/inbox-setup-view.ts';
import {
  reviewListingEditor,
  listingEditorReviewCurrent,
  editListingEditor,
  listingEditorSnapshot,
  type ListingEditor
} from './listing-editor.ts';
import {
  listingReviewSnapshot,
  listingReviewOwnership,
  reserveNewListing,
  type ListingReview,
  type ListingPreview
} from './prepare-listing.ts';
import {
  beginListingSigning,
  runListingSigning,
  stopListingSigning,
  type ListingSigning
} from './sign-listing.ts';
import {
  deliverListing,
  type ListingDeliveryResult
} from './deliver-listing.ts';
import { getPublicPool } from '../nostr/public-pool.ts';
import {
  readListingOperationView,
  closeListingOperationView,
  type ListingOperationView
} from './listing-operation-view.ts';

export type EditorFormError = Readonly<{
  field: keyof PublicDraftForm;
  message: string;
}>;
const fields = [
  'title',
  'description',
  'location',
  'amount',
  'currency',
  'unit',
  'quantity',
  'contactType',
  'contactValue'
] as const;
export function validateEditorForm(
  input: PublicDraftForm
): readonly EditorFormError[] {
  try {
    if (
      !exactLocalFields(input, fields) ||
      fields.some((field) => typeof input[field] !== 'string')
    )
      return [{ field: 'title', message: 'Keep all listing fields as text.' }];
    if (input.contactType === '' && input.contactValue !== '')
      return [
        {
          field: 'contactValue',
          message: 'Choose a public contact type or remove the contact details.'
        }
      ];
    const built = buildFoodTemplate({
      identifier: '00000000-0000-4000-8000-000000000001',
      created_at: 1,
      published_at: 1,
      status: 'active',
      title: input.title,
      description: input.description,
      location: input.location,
      amount: input.amount,
      currency: input.currency,
      unit: input.unit,
      ...(input.quantity === '' ? {} : { quantity: input.quantity }),
      ...(input.contactType === ''
        ? {}
        : {
            contact: {
              type: input.contactType,
              value: input.contactValue,
              public: true as const
            }
          })
    });
    if (built.ok) return [];
    const messages: Partial<Record<FoodDraftErrorCode, EditorFormError>> = {
      title_invalid: {
        field: 'title',
        message: 'Use a product name within the public text limit.'
      },
      location_invalid: {
        field: 'location',
        message: 'Use a public area within the text limit.'
      },
      description_invalid: {
        field: 'description',
        message: 'Add a complete description within the public text limit.'
      },
      summary_invalid: {
        field: 'description',
        message: 'Use supported public text in the description.'
      },
      price_invalid: {
        field: 'amount',
        message: 'Use a supported price, currency and price unit.'
      },
      quantity_invalid: {
        field: 'quantity',
        message: 'Use a supported advertised quantity in the selected unit.'
      },
      contact_invalid: {
        field: 'contactValue',
        message: 'Use supported public contact details for the selected type.'
      }
    };
    return [
      messages[built.error.code] ?? {
        field: 'description',
        message: 'The public listing exceeds its supported limits.'
      }
    ];
  } catch {
    return [
      {
        field: 'title',
        message:
          'The listing fields could not be read. Keep your current input.'
      }
    ];
  }
}
declare const reviewBrand: unique symbol;
export type EditorReview = Readonly<{ [reviewBrand]: true }>;
type Owned = Readonly<{
  editor: ListingEditor;
  original: ListingReview;
  active(): boolean;
  close(): void;
  signing(): ListingSigning | undefined;
  setSigning(value: ListingSigning): void;
  operation(): ListingOperationView | undefined;
  setOperation(value: ListingOperationView): void;
}>;
function createReviewOwner(
  editor: ListingEditor,
  original: ListingReview
): Owned {
  let active = true;
  let signing: ListingSigning | undefined;
  let operation: ListingOperationView | undefined;
  return {
    editor,
    original,
    active: () => active,
    signing: () => signing,
    setSigning(value) {
      signing = value;
    },
    operation: () => operation,
    setOperation(value) {
      if (operation) closeListingOperationView(operation);
      operation = value;
    },
    close() {
      active = false;
      if (signing) stopListingSigning(signing);
      if (operation) closeListingOperationView(operation);
    }
  };
}
const reviews = new WeakMap<EditorReview, Owned>();
const publishing = new WeakMap<EditorReview, Promise<ListingDeliveryResult>>();
export type EditorReviewSnapshot = ListingPreview &
  Readonly<{ content: string; tags: readonly (readonly string[])[] }>;
export async function openEditorReview(
  editor: ListingEditor,
  setup: InboxSetupView | undefined,
  form: PublicDraftForm,
  status: 'active' | 'sold' = 'active'
): Promise<
  | Readonly<{ status: 'review'; review: EditorReview }>
  | Readonly<{
      status: 'blocked';
      errors: readonly EditorFormError[];
      message: string;
    }>
> {
  const errors = validateEditorForm(form);
  if (errors.length)
    return {
      status: 'blocked',
      errors,
      message: 'Check the linked fields. Your input remains here.'
    };
  if (
    listingEditorSnapshot(editor)?.mode === 'edit' &&
    !(await editListingEditor(editor, form))
  )
    return {
      status: 'blocked',
      errors: [],
      message:
        'Keep your proposed changes. The current listing could not be safely prepared.'
    };
  const result = await reviewListingEditor(editor, setup, form, status);
  if (
    result.status !== 'review' ||
    !listingEditorReviewCurrent(editor, result.review)
  )
    return {
      status: 'blocked',
      errors: [],
      message:
        result.status === 'blocked' && result.reason === 'unavailable'
          ? 'Enable messages before reviewing this listing. Your draft remains in this browser.'
          : 'The saved listing or account changed. Keep your input and review the current values again.'
    };
  const review = Object.freeze({}) as EditorReview;
  reviews.set(review, createReviewOwner(editor, result.review));
  return { status: 'review', review };
}
export function editorReviewSnapshot(
  review: EditorReview
): EditorReviewSnapshot | undefined {
  const owned = reviews.get(review);
  if (!owned || !listingEditorReviewCurrent(owned.editor, owned.original))
    return;
  const preview = listingReviewSnapshot(owned.original);
  if (!preview) return;
  const wire = JSON.parse(preview.templateWire) as {
    content: string;
    tags: string[][];
  };
  return { ...preview, content: wire.content, tags: wire.tags };
}
export function closeEditorReview(review: EditorReview): void {
  const owned = reviews.get(review);
  owned?.close();
  reviews.delete(review);
}
export function editorPublicationOperation(
  review: EditorReview
): ListingOperationView | undefined {
  const owned = reviews.get(review);
  return owned?.active() ? owned.operation() : undefined;
}
// Existing publication pipeline only. One original review deduplicates clicks;
// explicit consent does not bypass original editor/source/draft/current-head.
export function publishEditorRevision(
  review: EditorReview,
  consent: unknown
): Promise<ListingDeliveryResult> {
  const owned = reviews.get(review);
  const preview = owned && listingReviewSnapshot(owned.original);
  if (
    !owned?.active() ||
    !preview?.revision ||
    consent !== 'publish_reviewed_revision'
  )
    return Promise.resolve({ status: 'invalid' });
  const pending = publishing.get(review);
  if (pending) return pending;
  const job = (async (): Promise<ListingDeliveryResult> => {
    const original = listingReviewOwnership(owned.original);
    if (!original || !listingEditorReviewCurrent(owned.editor, owned.original))
      return { status: 'unavailable' };
    const current = () =>
      owned.active() &&
      reviews.get(review) === owned &&
      listingEditorReviewCurrent(owned.editor, owned.original);
    try {
      const reserved = await reserveNewListing(
        owned.original,
        'publish_reviewed_listing'
      );
      if (!current()) return { status: 'stopped' };
      if (reserved.status !== 'saved' && reserved.status !== 'existing')
        return {
          status: reserved.status === 'busy' ? 'busy' : 'storage_failed'
        };
      const signing = beginListingSigning(
        owned.original,
        'sign_reviewed_listing'
      );
      if (!signing) return { status: 'unavailable' };
      owned.setSigning(signing);
      const signed = await runListingSigning(signing, 'sign_reviewed_listing');
      if (!current()) return { status: 'stopped' };
      if (signed.status !== 'prepared')
        return {
          status:
            signed.status === 'busy'
              ? 'busy'
              : signed.status === 'stopped'
                ? 'stopped'
                : 'needs_action'
        };
      const pool = getPublicPool(original.readiness.policy);
      if (!pool || !current()) return { status: 'unavailable' };
      return await deliverListing(
        signed.publication,
        pool,
        original.readiness.policy,
        'publish_stored_listing'
      );
    } catch {
      return { status: 'needs_action' };
    } finally {
      const read = await readListingOperationView(
        original.repository,
        original.readiness.identity,
        original.command,
        owned.signing()
      );
      if (read.status === 'found') {
        if (owned.active() && reviews.get(review) === owned) {
          owned.setOperation(read.view);
        } else closeListingOperationView(read.view);
      }
    }
  })().finally(() => {
    if (publishing.get(review) === job) publishing.delete(review);
  });
  publishing.set(review, job);
  return job;
}
