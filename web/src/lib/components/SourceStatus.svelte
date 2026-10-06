<script lang="ts">
  import type { SearchRefresh, SearchScope } from '../catalog/search-state.ts';
  import Disclosure from './primitives/Disclosure.svelte';
  let {
    refresh,
    gap,
    scopes
  }: { refresh: SearchRefresh; gap: boolean; scopes: readonly SearchScope[] } =
    $props();
  const failedSources = $derived(
    scopes
      .flatMap((scope) =>
        scope.result.sources
          .filter(
            (source) => source.state === 'error' || source.state === 'closed'
          )
          .map((source) => source.source)
      )
      .filter((source, index, values) => values.indexOf(source) === index)
  );
</script>

<div class="stack">
  {#if refresh === 'unavailable'}<p class="notice">
      Search sources are unavailable.
    </p>
  {:else if failedSources.length === 1}<p class="notice">
      One source could not complete the search. These results may be incomplete.
    </p>
  {:else if failedSources.length > 1}<p class="notice">
      Some sources could not complete the search. These results may be
      incomplete.
    </p>
  {:else if refresh === 'refreshing' || refresh === 'idle'}<p>
      Checking search sources…
    </p>
  {:else if refresh === 'cancelled'}<p class="notice">
      Search was cancelled. These results may be incomplete.
    </p>
  {:else if refresh === 'deadline'}<p class="notice">
      The search time limit was reached. These results may be incomplete.
    </p>
  {:else if refresh === 'limit'}<p class="notice">
      The search resource limit was reached. These results may be incomplete.
    </p>
  {:else if refresh === 'error'}<p class="notice">
      The search could not finish. These results may be incomplete.
    </p>
  {:else if refresh !== 'bounded-eose'}<p class="notice">
      These results may be incomplete.
    </p>
  {:else}<p>Results are limited to the sources checked.</p>{/if}
  {#if gap}<p class="notice">
      The sources returned too many listings at the same timestamp. Older
      results may be missing.
    </p>{/if}
  <Disclosure summary="Source details">
    {#each scopes as scope (scope.result.context)}<p>
        Source request: {scope.state}. Coverage: {scope.result.coverage}.
      </p>
      {#each scope.result.sources as source (source.source)}<p>
          {source.source}: {source.state}. {source.accepted} verified deliveries admitted.
        </p>{/each}
    {/each}
    <p>
      Searches check a bounded set of sources. They do not prove complete
      coverage or current stock.
    </p>
  </Disclosure>
</div>
