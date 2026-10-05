<script lang="ts">
  import type { Snippet } from 'svelte';
  let { summary, children }: { summary: string; children?: Snippet } = $props();
</script>

<details class="disclosure">
  <summary>{summary}</summary>
  {#if children}<div class="stack">{@render children()}</div>{/if}
</details>
