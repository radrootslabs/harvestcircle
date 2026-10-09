<script lang="ts">
  import Button from './primitives/Button.svelte';
  import {
    inboxConversationRow,
    type InboxView
  } from '../messaging/inbox-view.ts';
  import type { InboxListRow } from '../messaging/inbox-list.ts';
  let {
    controller,
    row,
    onopen
  }: {
    controller: InboxView;
    row: InboxListRow;
    onopen?: (peer: string) => void;
  } = $props();
  // A detached row is only an invalidation/peer selection hint. Its body, unread
  // and role are never rendered without the same current genuine controller.
  const current = $derived(
    row ? inboxConversationRow(controller, row.peer) : undefined
  );
  const preview = $derived(
    current
      ? Array.from(current.latest.content).slice(0, 160).join('') +
          (Array.from(current.latest.content).length > 160 ? '…' : '')
      : ''
  );
  const time = $derived(
    current ? new Date(current.latest.createdAt * 1000) : undefined
  );
</script>

{#if current}
  <article class="stack" aria-label="Local conversation">
    <div class="cluster">
      <span class="key"
        >Peer {current.peer.slice(0, 8)}…{current.peer.slice(-8)}</span
      >{#if current.unread > 0}<span>New</span>{/if}<span class="text-small"
        >{time && !Number.isNaN(time.getTime())
          ? time.toLocaleString()
          : 'Time unavailable'}</span
      >
    </div>
    <p>{current.latest.role === 'self_archive' ? 'You: ' : ''}{preview}</p>
    <Button
      label={'Open conversation with ' + current.peer}
      disabled={!onopen}
      onclick={() => {
        if (current) onopen?.(current.peer);
      }}
    />
  </article>
{/if}
