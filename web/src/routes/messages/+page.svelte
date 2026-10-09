<script lang="ts">
  import { onMount } from 'svelte';
  import AccountGate from '../../lib/components/AccountGate.svelte';
  import InboxStatus from '../../lib/components/InboxStatus.svelte';
  let mounted = $state(false);
  onMount(() => {
    mounted = true;
  });
</script>

{#if mounted}<InboxStatus />{:else}<AccountGate />{/if}
