<script lang="ts">
  import { page } from '$app/state';
  import ConversationThread from '../../../lib/components/ConversationThread.svelte';
</script>

{#key page.params.conversationId}<ConversationThread
    conversationId={page.params.conversationId ?? ''}
  />{/key}
