<script lang="ts">
  import type { ListingView } from '../catalog/listing-view.ts';
  import { internalHref } from '../navigation-url.ts';
  import Disclosure from './primitives/Disclosure.svelte';
  let { listing }: { listing: ListingView } = $props();
  function timestamp(seconds: number): string {
    const milliseconds = seconds * 1000;
    return Number.isFinite(milliseconds) &&
      Math.abs(milliseconds) <= 8640000000000000
      ? new Date(milliseconds).toISOString()
      : `${seconds} seconds since the Unix epoch`;
  }
</script>

<article class="stack">
  <h2><a href={internalHref(listing.href)}>{listing.title}</a></h2>
  <p>{listing.price.currency} {listing.price.amount} / {listing.price.unit}</p>
  <p>{listing.publisher.label} · {listing.location}</p>
  {#if listing.quantity}<p>
      Advertised quantity: {listing.quantity.amount}
      {listing.quantity.unit}
    </p>{:else}<p>Advertised quantity not specified</p>{/if}
  <p>{listing.summary}</p>
  <p>Seller updated: {timestamp(listing.createdAt)}</p>
  {#if listing.lastKnown}<p class="notice">
      Last known listing. Availability has not been confirmed.
    </p>{/if}
  <Disclosure summary="Publisher details">
    {#if listing.publisher.assertedName}<p>
        This name is asserted by the publisher.
      </p>{/if}
    <p>Publisher key: <code>{listing.publisher.pubkey}</code></p>
  </Disclosure>
</article>
