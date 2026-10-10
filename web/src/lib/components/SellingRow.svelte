<script lang="ts">
  import type { SellingRow } from '../publishing/selling-view.ts';
  import { internalHref } from '../navigation-url.ts';
  import Disclosure from './primitives/Disclosure.svelte';
  import OperationStatus from './OperationStatus.svelte';
  let { row }: { row: SellingRow } = $props();
</script>

<article class="stack">
  <h3>{row.title}</h3>
  <p>{row.status}</p>
  <p>
    {row.provenance}{#if !row.editHref && !row.draftHref}
      · Read-only{/if}
  </p>
  <div class="cluster">
    {#if row.draftHref}<a href={internalHref(row.draftHref)}>Continue editing</a
      >{/if}
    {#if row.href}<a href={internalHref(row.href)}>View listing</a>{/if}
    {#if row.editHref}<a href={internalHref(row.editHref)}>Edit</a>{/if}
  </div>
  {#if row.operations.length}
    <Disclosure summary="Delivery details">
      {#each row.operations as operation (operation)}
        <OperationStatus publicOperation={operation} />
      {/each}
    </Disclosure>
  {/if}
</article>
