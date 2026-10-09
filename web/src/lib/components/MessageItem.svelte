<script lang="ts">
  import {
    inboxMessageSnapshot,
    type InboxView
  } from '../messaging/inbox-view.ts';
  import type { ThreadMessage } from '../messaging/conversation-thread.ts';
  import { internalHref } from '../navigation-url.ts';
  let {
    controller,
    conversationId,
    message,
    onreply
  }: {
    controller: InboxView;
    conversationId: string;
    message: ThreadMessage;
    onreply?: (parent: string) => void;
  } = $props();
  // Copied rows select only a hash. Render fresh current original room bytes.
  const current = $derived(
    inboxMessageSnapshot(controller, conversationId, message.data.rumorId)
  );
  const time = $derived(
    current ? new Date(current.data.createdAt * 1000) : undefined
  );
</script>

{#if current}
  <article class="stack" data-message-id={current.data.rumorId}>
    <div class="cluster">
      <span
        >{current.data.role === 'self_archive'
          ? 'You'
          : 'Peer ' + current.data.peer.slice(0, 8) + '…'}</span
      >
      {#if time && !Number.isNaN(time.getTime())}<time
          datetime={time.toISOString()}>{time.toLocaleString()}</time
        >{/if}
    </div>
    {#each current.data.content.split('\n') as line, index (index)}<p>
        {line}
      </p>{/each}
    {#if current.parent === 'missing'}<p class="text-small">
        Earlier message unavailable in this browser.
      </p>{/if}
    {#each current.citations as citation (citation.naddr)}
      <p>
        <a href={internalHref('/products/' + citation.naddr)}
          >Product citation: {citation.identifier || 'listing'} (unverified)</a
        >
      </p>
    {/each}
    {#if current.citations.length || /quoted price/i.test(current.data.content)}<p
        class="text-small"
      >
        A quoted price is not a current or locked quote.
      </p>{/if}
    {#each current.delivery ?? [] as label (label)}<p class="text-small">
        {label}
      </p>{/each}
    {#if onreply}<button
        type="button"
        class="button button--secondary"
        onclick={() => {
          if (current) onreply?.(current.data.rumorId);
        }}>Reply to this message</button
      >{/if}
  </article>
{/if}
