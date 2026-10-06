import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';
import { incomingAmount } from '../../../src/lib/contracts/food-availability-v1/text.ts';
import ListingFacts from '../../../src/lib/components/ListingFacts.svelte';
import PublisherIdentity from '../../../src/lib/components/PublisherIdentity.svelte';
import Product from '../../../src/routes/products/[naddr=naddr]/+page.svelte';
import { encodeProductReference } from '../../../src/lib/nostr/references.ts';
import {
  projectFoodParts,
  type FoodParts
} from '../../../src/lib/contracts/food-availability-v1/read.ts';
const host = vi.hoisted(() => ({
  params: { naddr: 'invalid' },
  url: new URL('https://harvest.example/products/invalid')
}));
vi.mock('$app/state', () => ({ page: host }));
const corpus = JSON.parse(
  readFileSync(
    new URL(
      '../../../../contracts/interop/food_availability/corpus.v1.json',
      import.meta.url
    ),
    'utf8'
  )
) as { vectors: { id: string; signed_wires: Record<string, string> }[] };
const event = JSON.parse(
  corpus.vectors.find((v) => v.id.endsWith('_032'))!.signed_wires.previous
) as FoodParts;
const projected = projectFoodParts(event);
if (projected.outcome !== 'focused') throw Error('fixture food');
describe('truthful public detail presentation', () => {
  it('renders escaped exact advertised values without remote media or link discovery', () => {
    const food = {
      ...projected.projection,
      content: '<img src=https://track.example> mailto:private@example.org',
      price: {
        ...projected.projection.price,
        amount: incomingAmount('4.0000000000000000000001')!
      },
      quantity: null
    };
    const result = render(ListingFacts, {
      props: { food, createdAt: 999999999999999 }
    }).body;
    expect(result).toContain('4.0000000000000000000001');
    expect(result).toContain('Advertised quantity not specified');
    expect(result).toContain('&lt;img');
    expect(result).toContain('seconds since the Unix epoch');
    expect(result).not.toMatch(/<img|<a|<iframe|<script/);
  });
  it('discloses asserted name and key without managed identity or farm certification', () => {
    const publisher = {
      pubkey: 'ab'.repeat(32),
      label: '<img src=https://track.example>',
      assertedName: true
    };
    const result = render(PublisherIdentity, { props: { publisher } }).body;
    expect(result).toContain(publisher.pubkey);
    expect(result).toContain('asserted by the publisher');
    expect(result).toContain('&lt;img');
    expect(result).not.toMatch(/<img|<a|verified farm|Manage listing/);
  });
  it('distinguishes invalid from valid checking SSR without reads or fabricated facts', () => {
    host.params.naddr = 'invalid';
    expect(render(Product).body).toContain('This food reference is invalid.');
    host.params.naddr = encodeProductReference({
      kind: 30402,
      pubkey: 'ab'.repeat(32),
      identifier: 'carrots'
    })!;
    const result = render(Product).body;
    expect(result).toContain('Checking food sources');
    expect(result).toContain('Copy link');
    expect(result).not.toMatch(
      /<img|mailto:|tel:|Message seller|Manage listing|Cedar Farm/
    );
  });
});
