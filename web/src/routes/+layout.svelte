<script lang="ts">
  import '../theme.css';
  import '../app.css';
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();
</script>

<main>
  <h1>HarvestCircle</h1>
  {@render children()}
</main>
