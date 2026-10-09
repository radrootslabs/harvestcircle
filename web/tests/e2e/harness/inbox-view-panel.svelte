<script lang="ts">
  import InboxStatus from '../../../src/lib/components/InboxStatus.svelte';
  import type { InboxView } from '../../../src/lib/messaging/inbox-view.ts';
  let { controller }: { controller: InboxView } = $props();
</script>

<InboxStatus {controller} />
