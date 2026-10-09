<script lang="ts">
  import { onMount } from 'svelte';
  import {
    subscribeInboxView,
    type InboxView
  } from '../messaging/inbox-view.ts';
  import {
    conversationOptionsSnapshot,
    requestConversationOption
  } from '../messaging/cited-product.ts';
  let {
    controller,
    conversationId,
    onblock,
    onreport
  }: {
    controller: InboxView;
    conversationId: string;
    onblock?: (peer: string) => void;
    onreport?: (peer: string) => void;
  } = $props();
  let revision = $state(0);
  const actual = $derived.by(() => {
    void revision;
    return conversationOptionsSnapshot(controller, conversationId);
  });
  onMount(() =>
    subscribeInboxView(controller, () => {
      revision++;
    })
  );
</script>

{#if actual}
  <details>
    <summary>Identity and conversation options</summary>
    <p>Authenticated conversation peer: {actual.peer}</p>
    {#if onblock}<button
        type="button"
        onclick={() => {
          if (onblock)
            requestConversationOption(
              controller,
              conversationId,
              'block',
              onblock,
              'reviewed_conversation_option'
            );
        }}>Block person</button
      >{/if}
    {#if onreport}<button
        type="button"
        onclick={() => {
          if (onreport)
            requestConversationOption(
              controller,
              conversationId,
              'report',
              onreport,
              'reviewed_conversation_option'
            );
        }}>Report conversation</button
      >{/if}
  </details>
{/if}
