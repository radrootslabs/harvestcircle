<script lang="ts">
  import Button from './primitives/Button.svelte';
</script>

<svelte:head>
  <title>HarvestCircle</title>
  <meta name="robots" content="noindex" />
</svelte:head>
<div class="page page--reading stack">
  <h1>Connect or unlock</h1>
  <p>Connect an extension and unlock your account to continue.</p>
  <p class="notice">Account access is unavailable during development.</p>
  <div class="cluster">
    <Button label="Connect extension" disabled />
    <Button label="Unlock" disabled />
  </div>
</div>
