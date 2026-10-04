<script lang="ts">
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();
</script>

<main>
  <h1>HarvestCircle</h1>
  {@render children()}
</main>
