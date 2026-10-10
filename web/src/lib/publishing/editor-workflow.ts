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
  type ListingEditor
} from './listing-editor.ts';
import {
  listingReviewSnapshot,
  type ListingReview,
  type ListingPreview
} from './prepare-listing.ts';

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
type Owned = Readonly<{ editor: ListingEditor; original: ListingReview }>;
const reviews = new WeakMap<EditorReview, Owned>();
export type EditorReviewSnapshot = ListingPreview &
  Readonly<{ content: string; tags: readonly (readonly string[])[] }>;
export async function openEditorReview(
  editor: ListingEditor,
  setup: InboxSetupView | undefined,
  form: PublicDraftForm
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
  const result = await reviewListingEditor(editor, setup, form);
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
  reviews.set(review, { editor, original: result.review });
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
  reviews.delete(review);
}
