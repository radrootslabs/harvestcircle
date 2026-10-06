import { publicContact, type PublicContactInput } from './contact.ts';
import { foodPrice, foodQuantity } from './values.ts';
import {
  boundedUtf8,
  foodTextValid,
  foodIdentifierValid,
  foodContentPresent
} from './text.ts';
import {
  boundedEnvelopeNumbers,
  safeUnsignedInteger
} from '../../nostr/envelope-bounds.ts';
export type FoodDraft = Readonly<{
  identifier: string;
  title: string;
  description: string;
  location: string;
  amount: string;
  currency: string;
  unit: string;
  quantity?: string;
  published_at: number;
  created_at: number;
  status: 'active' | 'sold';
  contact?: PublicContactInput;
}>;
export type UnsignedFoodParts = Readonly<{
  kind: 30402;
  content: string;
  tags: readonly (readonly string[])[];
}>;
export type FoodDraftErrorCode =
  | 'identifier_invalid'
  | 'title_invalid'
  | 'location_invalid'
  | 'description_invalid'
  | 'summary_invalid'
  | 'price_invalid'
  | 'quantity_invalid'
  | 'timestamp_invalid'
  | 'status_invalid'
  | 'contact_invalid'
  | 'wire_too_large';
export type FoodBuildResult =
  | Readonly<{ ok: true; wire_parts: UnsignedFoodParts; summary: string }>
  | Readonly<{
      ok: false;
      error: Readonly<{ code: FoodDraftErrorCode; message: string }>;
    }>;
function failure(code: FoodDraftErrorCode): FoodBuildResult {
  return {
    ok: false,
    error: {
      code,
      message:
        'Public food draft does not satisfy the selected field or size limit'
    }
  };
}
function text(value: string, maximum: number): boolean {
  return (
    typeof value === 'string' &&
    foodTextValid(value) &&
    Array.from(value).length <= maximum
  );
}
function normalizedDescription(value: string): string {
  return value.split('\r\n').join('\n').split('\r').join('\n').trim();
}
function summary(value: string): string {
  // The signed content retains its normalized line layout. Only this derived
  // review field collapses whitespace; complete Unicode code points survive.
  return Array.from(
    Array.from(value)
      .map((character) =>
        /\p{White_Space}/u.test(character) ? ' ' : character
      )
      .join('')
      .split(' ')
      .filter((word) => word.length > 0)
      .join(' ')
  )
    .slice(0, 240)
    .join('')
    .trimEnd();
}
export function buildFoodTemplate(input: FoodDraft): FoodBuildResult {
  // Snapshot only known fields once; accessors cannot swap validated values.
  const {
    identifier,
    title,
    description: sourceDescription,
    location,
    amount,
    currency,
    unit,
    quantity: sourceQuantity,
    published_at,
    created_at,
    status,
    contact: sourceContact
  } = input;
  if (typeof identifier !== 'string' || !foodIdentifierValid(identifier))
    return failure('identifier_invalid');
  if (!text(title, 160)) return failure('title_invalid');
  if (!text(location, 160)) return failure('location_invalid');
  if (
    typeof sourceDescription !== 'string' ||
    !boundedUtf8(sourceDescription, 16384)
  )
    return failure('description_invalid');
  const description = normalizedDescription(sourceDescription);
  if (!foodContentPresent(description)) return failure('description_invalid');
  const derived = summary(description);
  if (!foodTextValid(derived)) return failure('summary_invalid');
  const price = foodPrice(amount, currency, unit);
  if (price === undefined) return failure('price_invalid');
  const quantity =
    sourceQuantity === undefined
      ? undefined
      : foodQuantity(sourceQuantity, unit);
  if (sourceQuantity !== undefined && quantity === undefined)
    return failure('quantity_invalid');
  const numbers = boundedEnvelopeNumbers(30402, created_at);
  const publishedAt = safeUnsignedInteger(published_at);
  if (
    numbers === undefined ||
    publishedAt === undefined ||
    publishedAt === 0 ||
    publishedAt > numbers.created_at
  )
    return failure('timestamp_invalid');
  if (status !== 'active' && status !== 'sold')
    return failure('status_invalid');
  const contact =
    sourceContact === undefined ? undefined : publicContact(sourceContact);
  if (sourceContact !== undefined && contact === undefined)
    return failure('contact_invalid');
  const content =
    description +
    (contact === undefined ? '' : `\n\nPublic contact: ${contact.href}`);
  if (!boundedUtf8(content, 16384)) return failure('description_invalid');
  const tags: readonly (readonly string[])[] = [
    ['d', identifier],
    ['title', title],
    ['summary', derived],
    ['published_at', String(publishedAt)],
    ['location', location],
    ['price', price.amount, price.currency],
    ['radroots:price_unit', price.unit],
    ...(quantity === undefined
      ? []
      : [['radroots:quantity', quantity.amount, quantity.unit]]),
    ['status', status]
  ];
  // Bound the prospective compact signed envelope using fixed-size public hex
  // placeholders. No signature, key, image or caller extra is manufactured.
  const maximumWire = JSON.stringify({
    id: '0'.repeat(64),
    pubkey: '0'.repeat(64),
    created_at: numbers.created_at,
    kind: 30402,
    tags,
    content,
    sig: '0'.repeat(128)
  });
  if (!boundedUtf8(maximumWire, 262144)) return failure('wire_too_large');
  return {
    ok: true,
    wire_parts: { kind: 30402, content, tags },
    summary: derived
  };
}
