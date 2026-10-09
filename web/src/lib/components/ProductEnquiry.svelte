<script module lang="ts">
  declare function setInterval(
    callback: () => void,
    milliseconds: number
  ): number;
  declare function clearInterval(timer: number): void;
</script>

<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type {
    ProductView,
    ProductViewSnapshot
  } from '../catalog/product-view.ts';
  import {
    captureProductEnquiry,
    productEnquirySnapshot,
    updateProductEnquiry,
    closeProductEnquiry,
    type ProductEnquiry
  } from '../messaging/product-enquiry.ts';
  import Button from './primitives/Button.svelte';
  let {
    view,
    observation,
    oncapture
  }: {
    view: ProductView;
    observation?: ProductViewSnapshot;
    oncapture?: (draft: ProductEnquiry) => void;
  } = $props();
  let draft = $state.raw<ProductEnquiry | undefined>(),
    captured = $state<ReturnType<typeof productEnquirySnapshot>>();
  onMount(() => {
    const timer = setInterval(() => {
      if (draft) captured = productEnquirySnapshot(draft);
    }, 250);
    return () => clearInterval(timer);
  });
  $effect(() => {
    const original = view;
    return () =>
      untrack(() => {
        void original;
        if (draft) closeProductEnquiry(draft);
        draft = undefined;
        captured = undefined;
      });
  });
  $effect(() => {
    void observation;
    if (draft) captured = productEnquirySnapshot(draft);
  });
  function open() {
    if (draft) closeProductEnquiry(draft);
    draft = captureProductEnquiry(view, 'reviewed_product_enquiry');
    captured = draft ? productEnquirySnapshot(draft) : undefined;
    if (draft) oncapture?.(draft);
  }
  function edit(value: string) {
    if (!draft) return;
    if (updateProductEnquiry(draft, value, 'reviewed_product_enquiry_edit'))
      captured = productEnquirySnapshot(draft);
  }
</script>

{#if captured}
  <section class="stack" aria-label="Product request">
    <h2>Message about {captured.title}</h2>
    <p>Publisher: <code>{captured.peer}</code></p>
    <p>Advertised price (seller assertion): {captured.advertisedPrice}</p>
    <label
      >Your message<textarea
        value={captured.text}
        oninput={(event) => edit(event.currentTarget.value)}></textarea></label
    >
    <details>
      <summary>Included product references</summary>
      <p>Product: <code>nostr:{captured.naddr}</code></p>
      <p>Viewed listing: <code>nostr:{captured.nevent}</code></p>
    </details>
    <p>Not yet saved. Unsent text is lost when this page closes.</p>
    {#if !captured.listingCurrent}<p role="status">
        Listing status needs review. Your unsent text is kept in memory.
      </p>{/if}
    <p>
      This is a message, not a confirmed order. It does not reserve the food.
    </p>
    <Button label="Send request" disabled={true} />
    <p>
      Connect and prepare your inbox to send a request. No message has been
      sent.
    </p>
  </section>
{:else}
  <Button
    label="Message seller"
    onclick={open}
    disabled={observation?.outcome !== 'active' || observation.lastKnown}
  />
{/if}
