<script lang="ts">
  import ConversationThread from '../../../src/lib/components/ConversationThread.svelte';
  import type { InboxView } from '../../../src/lib/messaging/inbox-view.ts';
  let {
    controller,
    conversationId
  }: { controller: InboxView; conversationId: string } = $props();
</script>

<button id="reading-marker" type="button">Reading position marker</button>
<ConversationThread {controller} {conversationId} />
