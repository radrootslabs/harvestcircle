<script lang="ts">
  import ProductEnquiry from '../../../src/lib/components/ProductEnquiry.svelte';
  import type {
    ProductView,
    ProductViewSnapshot
  } from '../../../src/lib/catalog/product-view.ts';
  import type { ProductEnquiry as Draft } from '../../../src/lib/messaging/product-enquiry.ts';
  let {
    view,
    observe,
    capture
  }: {
    view: ProductView;
    observe: (callback: (value: ProductViewSnapshot) => void) => () => void;
    capture: (draft: Draft) => void;
  } = $props();
  let observation = $state.raw<ProductViewSnapshot | undefined>();
  $effect(() =>
    observe((value) => {
      observation = value;
    })
  );
</script>

<section data-enquiry-fixture>
  <ProductEnquiry {view} {observation} oncapture={capture} />
</section>
