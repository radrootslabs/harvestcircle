<script lang="ts">
  import type { Snippet } from 'svelte';
  let { children }: { children?: Snippet } = $props();
</script>

<div class="cluster">
  {#if children}{@render children()}{/if}
</div>
