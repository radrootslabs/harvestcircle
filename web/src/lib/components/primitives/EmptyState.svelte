<script lang="ts">
  import type { Snippet } from 'svelte';
  let {
    title,
    description,
    children
  }: { title: string; description: string; children?: Snippet } = $props();
</script>

<section class="stack">
  <h2>{title}</h2>
  <p>{description}</p>
  {#if children}{@render children()}{/if}
</section>
