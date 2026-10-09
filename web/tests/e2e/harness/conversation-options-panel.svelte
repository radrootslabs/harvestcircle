<script lang="ts">
  import ConversationOptions from '../../../src/lib/components/ConversationOptions.svelte';
  import MessageItem from '../../../src/lib/components/MessageItem.svelte';
  import type { InboxView } from '../../../src/lib/messaging/inbox-view.ts';
  import type { PublicRuntime } from '../../../src/lib/runtime/public-runtime.ts';
  import type { ThreadMessage } from '../../../src/lib/messaging/conversation-thread.ts';
  let {
    controller,
    conversationId,
    runtime,
    message,
    hook
  }: {
    controller: InboxView;
    conversationId: string;
    runtime: PublicRuntime;
    message: ThreadMessage;
    hook: (action: string, peer: string) => void;
  } = $props();
</script>

<section data-options-fixture>
  <ConversationOptions
    {controller}
    {conversationId}
    onblock={(peer: string) => hook('block', peer)}
    onreport={(peer: string) => hook('report', peer)}
  />
  <MessageItem
    {controller}
    {conversationId}
    {message}
    {runtime}
    onreply={(parent: string) => hook('reply', parent)}
  />
</section>
