<script lang="ts">
  import { deploymentSupportPolicy } from '../config/deployment-support.ts';
  import { readSupportPolicy, type SupportPolicy } from '../config/support.ts';
  import { navigationHref } from '../navigation-url.ts';
  let { policy = deploymentSupportPolicy }: { policy?: SupportPolicy } =
    $props();
  const actual = $derived.by(() => {
    try {
      return readSupportPolicy(policy);
    } catch {
      return undefined;
    }
  });
</script>

{#if actual?.owner && actual.destination}
  <p>Support is operated by {actual.owner}.</p>
  <p>
    This opens an external support destination. No report is attached
    automatically.
  </p>
  <a
    href={navigationHref(actual.destination)}
    rel="noopener noreferrer"
    referrerpolicy="no-referrer">Contact support</a
  >
{:else}
  <p class="notice">
    Support has not been configured. Reporting to an operator is unavailable.
  </p>
{/if}
