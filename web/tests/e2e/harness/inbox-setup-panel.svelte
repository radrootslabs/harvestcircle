<script lang="ts">
  import InboxSetup from '../../../src/lib/components/InboxSetup.svelte';
  import type { InboxSetupView } from '../../../src/lib/messaging/inbox-setup-view.ts';
  let { controller }: { controller: InboxSetupView } = $props();
  let draft = $state('Memory-only enquiry text');
  let sends = $state(0);
</script>

<section class="page stack">
  <h1>HC_TEST_ONLY_INBOX_SETUP</h1>
  <label for="fixture-draft">Memory-only enquiry</label>
  <textarea id="fixture-draft" bind:value={draft}></textarea>
  <p role="status">Explicit enquiry sends: {sends}</p>
  <button
    class="button"
    onclick={() => {
      sends++;
    }}>Explicit fixture Send</button
  >
  <InboxSetup {controller} />
</section>
