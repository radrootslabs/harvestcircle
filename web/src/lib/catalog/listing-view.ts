import type { HeadResolution } from './resolve-head.ts';
import type { FoodProjection } from '../contracts/food-availability-v1/read.ts';
import { encodeProductReference } from '../nostr/references.ts';
import { internalHref } from '../navigation-url.ts';
import type { PublisherView } from './publishers.ts';
export type ListingView = Readonly<{
  eventId: string;
  createdAt: number;
  identifier: string;
  href: string;
  title: string;
  summary: string;
  content: string;
  location: string;
  price: FoodProjection['price'];
  quantity: FoodProjection['quantity'];
  publisher: PublisherView;
  lastKnown: boolean;
  coverage: HeadResolution['coverage'];
}>;
// Only the genuine resolver supplies these rows. Ordering follows winner,
// deletion, future and focused admission and local matching, before pagination.
export function orderFoodRows(
  rows: readonly HeadResolution[]
): readonly HeadResolution[] {
  return rows.toSorted(
    (a, b) =>
      b.state.head.created_at - a.state.head.created_at ||
      (a.state.head.id < b.state.head.id
        ? -1
        : a.state.head.id > b.state.head.id
          ? 1
          : 0)
  );
}
export function listingView(
  row: HeadResolution,
  publisher?: PublisherView
): ListingView | undefined {
  const food = row.state.food,
    head = row.state.head;
  if (
    !food ||
    food.status !== 'active' ||
    row.deletion.outcome === 'suppressed'
  )
    return undefined;
  const naddr = encodeProductReference({
    kind: 30402,
    pubkey: head.pubkey,
    identifier: food.identifier
  });
  const href =
    naddr === undefined ? undefined : internalHref(`/products/${naddr}`);
  if (href === undefined) return undefined;
  return {
    eventId: head.id,
    createdAt: head.created_at,
    identifier: food.identifier,
    href,
    title: food.title,
    summary: food.summary,
    content: food.content,
    location: food.location,
    price: { ...food.price },
    quantity: food.quantity === null ? null : { ...food.quantity },
    publisher:
      publisher?.pubkey === head.pubkey
        ? { ...publisher }
        : { pubkey: head.pubkey, label: head.pubkey, assertedName: false },
    lastKnown: row.lastKnown,
    coverage: row.coverage
  };
}
