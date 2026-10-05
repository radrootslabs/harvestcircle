import { markerPartition } from './partition.ts';
import {
  projectImages,
  type InboundImage,
  type ImageDiagnostic
} from './media.ts';
import {
  boundedUtf8,
  foodContentPresent,
  foodIdentifierValid,
  foodTextValid,
  incomingAmount
} from './text.ts';
import {
  foodCurrency,
  foodUnit,
  type FoodPrice,
  type FoodQuantity
} from './values.ts';
import {
  boundedEnvelopeNumbers,
  foodPublishedAt
} from '../../nostr/envelope-bounds.ts';
import {
  boundedEnvelopeTags,
  verifyEnvelope,
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from '../../nostr/verified-envelope.ts';
export type FoodParts = Readonly<{
  kind: number;
  created_at: number;
  content: string;
  tags: readonly (readonly string[])[];
}>;
export type FoodProjection = Readonly<{
  content: string;
  identifier: string;
  title: string;
  summary: string;
  published_at: number;
  location: string;
  price: FoodPrice;
  quantity: FoodQuantity | null;
  status: 'active' | 'sold';
  images: readonly InboundImage[];
  diagnostics: readonly ImageDiagnostic[];
}>;
export type FoodError = Readonly<{ code: string; message: string }>;
export type FoodProjectionOutcome =
  | Readonly<{ outcome: 'focused'; projection: FoodProjection }>
  | Readonly<{
      outcome: 'excluded';
      partition: 'operational_listing' | 'generic_nip99';
    }>
  | Readonly<{ outcome: 'rejected'; error: FoodError }>;
export type FoodAdmission =
  | Readonly<{
      outcome: 'admitted';
      event_id: string;
      verified: VerifiedEnvelope;
      projection: FoodProjection;
    }>
  | Readonly<{
      outcome: 'excluded';
      event_id: string;
      verified: VerifiedEnvelope;
      partition: 'operational_listing' | 'generic_nip99';
    }>
  | Readonly<{ outcome: 'rejected'; error: FoodError }>;
const prohibited = [
  'buyer',
  'checkout',
  'delivery',
  'exception',
  'group',
  'invite',
  'order',
  'payment',
  'pickup',
  'proof',
  'provenance',
  'receipt',
  'route',
  'route_stop',
  'task'
];
function rejected(code: string, message: string): FoodProjectionOutcome {
  return { outcome: 'rejected', error: { code, message } };
}
// Pure untrusted-parts projection for contract conformance. Its result carries
// no event id or verified proof and cannot substitute for readFoodEvent admission.
export function projectFoodParts(event: FoodParts): FoodProjectionOutcome {
  if (!boundedEnvelopeNumbers(event.kind, event.created_at))
    return rejected(
      'envelope_invalid',
      'invalid or unsupported bounded NIP-01 envelope'
    );
  if (!boundedUtf8(event.content, 262144) || !boundedEnvelopeTags(event.tags))
    return rejected(
      'envelope_invalid',
      'invalid or unsupported bounded NIP-01 envelope'
    );
  if (event.kind !== 30402)
    return rejected(
      'invalid_kind',
      `FoodAvailability event kind must be 30402, got ${event.kind}`
    );
  const partition = markerPartition(event.tags);
  if (partition === 'ambiguous')
    return rejected(
      'food_profile_ambiguous',
      'classified listing mixes focused and operational markers'
    );
  if (partition !== 'focused_food_availability')
    return { outcome: 'excluded', partition };
  for (const tag of event.tags)
    if (prohibited.includes(tag[0]))
      return rejected(
        'prohibited_capability',
        `FoodAvailability tag \`${tag[0]}\` is a prohibited capability`
      );
  const matching = (name: string) =>
    event.tags.filter((tag) => tag[0] === name);
  const names = ['d', 'title', 'summary', 'published_at', 'location', 'status'];
  for (const name of names) {
    const tags = matching(name);
    if (tags.length !== 1 || tags[0].length !== 2)
      return rejected(
        'food_tag_invalid',
        'FoodAvailability core tag shape is invalid'
      );
  }
  const [identifier, title, summary, publishedValue, location, status] =
    names.map((name) => matching(name)[0][1]);
  if (!foodContentPresent(event.content))
    return rejected(
      'food_content_missing',
      'FoodAvailability content must be non-whitespace'
    );
  if (!boundedUtf8(event.content, 131072))
    return rejected(
      'food_content_too_large',
      'FoodAvailability content exceeds 131072 bytes'
    );
  if (!foodIdentifierValid(identifier))
    return rejected(
      'food_identifier_invalid',
      'FoodAvailability identifier must be nonempty and contain no whitespace, control, or format characters'
    );
  if (!foodTextValid(title) || !foodTextValid(summary))
    return rejected(
      'food_text_invalid',
      'FoodAvailability text must be trimmed, nonempty, and contain no control or format characters'
    );
  const publishedAt = foodPublishedAt(publishedValue);
  if (publishedAt === undefined)
    return rejected(
      'food_published_at_invalid',
      'FoodAvailability published_at must be a canonical nonzero u64 timestamp'
    );
  if (publishedAt > event.created_at)
    return rejected(
      'food_published_at_future',
      `FoodAvailability published_at ${publishedAt} exceeds created_at ${event.created_at}`
    );
  if (!foodTextValid(location))
    return rejected(
      'food_text_invalid',
      'FoodAvailability text must be trimmed, nonempty, and contain no control or format characters'
    );
  const prices = matching('price');
  if (prices.length !== 1)
    return rejected(
      'price_invalid',
      'FoodAvailability price must be a canonical unsigned decimal with at most 28 digits'
    );
  if (prices[0].length > 3)
    return rejected(
      'price_frequency_forbidden',
      'FoodAvailability price frequency is forbidden'
    );
  const amount =
    prices[0].length === 3 ? incomingAmount(prices[0][1]) : undefined;
  if (amount === undefined)
    return rejected(
      'price_invalid',
      'FoodAvailability price must be a canonical unsigned decimal with at most 28 digits'
    );
  const currency = foodCurrency(
    prices[0][2].length === 3 && /^[a-zA-Z]{3}$/u.test(prices[0][2])
      ? prices[0][2].toUpperCase()
      : ''
  );
  if (currency === undefined)
    return rejected(
      'price_currency_invalid',
      'FoodAvailability price currency must be three uppercase ASCII letters'
    );
  const units = matching('radroots:price_unit');
  if (units.length === 0)
    return rejected(
      'price_unit_missing',
      'FoodAvailability price unit is missing'
    );
  const unit =
    units.length === 1 && units[0].length === 2
      ? foodUnit(units[0][1])
      : undefined;
  if (unit === undefined)
    return rejected(
      'price_unit_invalid',
      'FoodAvailability price unit is not governed'
    );
  const quantities = matching('radroots:quantity');
  let quantity: FoodQuantity | null = null;
  if (quantities.length > 0) {
    const tag = quantities[0];
    const value =
      quantities.length === 1 && tag.length === 3
        ? incomingAmount(tag[1])
        : undefined;
    if (value === undefined || foodUnit(tag[2]) !== unit)
      return rejected(
        'quantity_invalid',
        'FoodAvailability quantity must be canonical and use the price unit'
      );
    if (value === '0')
      return rejected(
        'quantity_zero',
        'FoodAvailability quantity must be positive'
      );
    quantity = { amount: value, unit };
  }
  if (status !== 'active' && status !== 'sold')
    return rejected(
      'food_status_invalid',
      'FoodAvailability status must be active or sold'
    );
  const media = projectImages(matching('image'));
  return {
    outcome: 'focused',
    projection: {
      content: event.content,
      identifier,
      title,
      summary,
      published_at: publishedAt,
      location,
      price: { amount, currency, unit },
      quantity,
      status,
      images: media.images,
      diagnostics: media.diagnostics
    }
  };
}
export function readFoodEvent(raw: unknown): FoodAdmission {
  const result = verifyEnvelope(raw);
  if (!result.ok) return { outcome: 'rejected', error: result.error };
  const event = verifiedEnvelopeSnapshot(result.value);
  if (!event)
    return {
      outcome: 'rejected',
      error: {
        code: 'envelope_invalid',
        message: 'invalid verified envelope proof'
      }
    };
  const projected = projectFoodParts(event);
  if (projected.outcome === 'focused')
    return {
      outcome: 'admitted',
      event_id: event.id,
      verified: result.value,
      projection: projected.projection
    };
  if (projected.outcome === 'excluded')
    return { ...projected, event_id: event.id, verified: result.value };
  return projected;
}
