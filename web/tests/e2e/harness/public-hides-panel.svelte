<script lang="ts">
  import { onMount } from 'svelte';
  import PublisherHide from '../../../src/lib/components/PublisherHide.svelte';
  import ListingRow from '../../../src/lib/components/ListingRow.svelte';
  import Privacy from '../../../src/routes/privacy/+page.svelte';
  import type { createFixture } from './public-hides.ts';
  let { fixture }: { fixture: Awaited<ReturnType<typeof createFixture>> } =
    $props();
  let view = $state.raw<ReturnType<typeof fixture.view>>(),
    observation = $state.raw<ReturnType<typeof fixture.publicSnapshot>>(),
    results = $state.raw<ReturnType<typeof fixture.searchSnapshot>>();
  onMount(() => {
    view = fixture.view();
    observation = fixture.publicSnapshot();
    results = fixture.searchSnapshot();
    return fixture.subscribe(() => {
      view = fixture.view();
      observation = fixture.publicSnapshot();
      results = fixture.searchSnapshot();
    });
  });
</script>

<section class="page page--reading stack">
  {#if observation?.food}<h2>{observation.food.title}</h2>{/if}
  {#if view}<PublisherHide {view} />{/if}
  <div data-public-search>
    <ul>
      {#each results?.listings ?? [] as listing (listing.href)}<li>
          <ListingRow {listing} />
        </li>{/each}
    </ul>
  </div>
  <Privacy />
</section>
