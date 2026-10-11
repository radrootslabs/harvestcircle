<script lang="ts">
  import { onMount } from 'svelte';
  import {
    subscribeInboxView,
    blockInboxConversation,
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
  let reviewing = $state(false),
    busy = $state(false),
    notice = $state<string | undefined>();
  async function confirmBlock() {
    if (!reviewing || busy) return;
    busy = true;
    const result = await blockInboxConversation(
      controller,
      conversationId,
      'reviewed_local_private_block'
    );
    busy = false;
    reviewing = false;
    notice =
      result.status === 'saved' || result.status === 'existing'
        ? 'Person blocked in this browser.'
        : 'Block was not confirmed. Existing local data has not been reset.';
  }
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
    <button
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
        else reviewing = true;
      }}>Block person</button
    >
    {#if reviewing}
      <p>
        Blocking hides messages and previews for this account in this browser
        and may discard unsaved private text. Encrypted evidence, delivery
        receipts and remote copies remain. It sends no report or sender
        notification and does not accept or reject an order. Undo is available
        in Privacy.
      </p>
      <button type="button" disabled={busy} onclick={() => void confirmBlock()}
        >Confirm local block</button
      >
      <button
        type="button"
        disabled={busy}
        onclick={() => {
          reviewing = false;
        }}>Cancel</button
      >
    {/if}
    {#if notice}<p role="status">{notice}</p>{/if}
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
