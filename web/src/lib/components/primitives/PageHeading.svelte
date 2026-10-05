<script lang="ts">
  let { title, description }: { title: string; description?: string } =
    $props();
</script>

<header class="stack">
  <h1>{title}</h1>
  {#if description}<p>{description}</p>{/if}
</header>
