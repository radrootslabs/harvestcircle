<script lang="ts">
  import { page } from '$app/state';
</script>

<svelte:head>
  <title>HarvestCircle</title>
  <meta name="robots" content="noindex" />
</svelte:head>
<div class="page page--reading stack">
  <h1>{page.status === 404 ? '404' : 'Unavailable'}</h1>
  <p>{page.status === 404 ? 'Not Found' : 'This page is unavailable.'}</p>
</div>
