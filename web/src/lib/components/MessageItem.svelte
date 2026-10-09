<script module lang="ts">
  declare function setInterval(
    callback: () => void,
    milliseconds: number
  ): number;
  declare function clearInterval(timer: number): void;
</script>

<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { PublicRuntime } from '../runtime/public-runtime.ts';
  import {
    captureCitedProduct,
    citedProductSnapshot,
    closeCitedProduct,
    type CitedProduct
  } from '../messaging/cited-product.ts';
  import {
    inboxMessageSnapshot,
    inboxThreadRevision,
    subscribeInboxView,
    type InboxView
  } from '../messaging/inbox-view.ts';
  import type { ThreadMessage } from '../messaging/conversation-thread.ts';
  import { internalHref } from '../navigation-url.ts';
  let {
    controller,
    conversationId,
    message,
    onreply,
    runtime
  }: {
    controller: InboxView;
    conversationId: string;
    message: ThreadMessage;
    onreply?: (parent: string) => void;
    runtime?: PublicRuntime;
  } = $props();
  // Copied rows select only a hash. Render fresh current original room bytes.
  let observation = $state(0);
  const current = $derived.by(() => {
    void observation;
    return inboxMessageSnapshot(
      controller,
      conversationId,
      message.data.rumorId
    );
  });
  $effect(() => {
    const view = controller;
    let previous = -2,
      changes = 0;
    return untrack(() =>
      subscribeInboxView(view, () => {
        const next = inboxThreadRevision(view);
        if (next === previous) return;
        previous = next;
        changes += 1;
        observation = changes;
      })
    );
  });
  const time = $derived(
    current ? new Date(current.data.createdAt * 1000) : undefined
  );
  let citation = $state.raw<CitedProduct | undefined>(),
    context = $state<ReturnType<typeof citedProductSnapshot>>();
  function verify(value: string) {
    if (citation) closeCitedProduct(citation);
    citation =
      current && runtime
        ? captureCitedProduct(
            controller,
            conversationId,
            current.data.rumorId,
            value,
            runtime,
            'reviewed_product_citation'
          )
        : undefined;
    context = citation ? citedProductSnapshot(citation) : undefined;
  }
  onMount(() => {
    const timer = setInterval(() => {
      if (citation) context = citedProductSnapshot(citation);
    }, 250);
    return () => {
      clearInterval(timer);
      if (citation) closeCitedProduct(citation);
    };
  });
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
      {#if runtime}<button type="button" onclick={() => verify(citation.naddr)}
          >Verify product citation</button
        >{/if}
    {/each}
    {#if context}
      {#if context.outcome === 'checking'}<p>Checking listing citation…</p>
      {:else if context.outcome === 'withdrawn'}<p>Listing is withdrawn</p>
      {:else if context.food}<p>{context.food.title}</p>
        <p>
          {context.outcome === 'sold'
            ? 'Listing is sold'
            : 'Signed listing observed'}
        </p>
      {:else}<p>Listing citation remains unresolved</p>{/if}
      {#if context.lastKnownFood}<p>{context.lastKnownFood.title}</p>
        <p>
          Last-known signed listing; current head and withdrawal checks are
          incomplete.
        </p>{/if}
      {#if context.includedVersion === 'matched'}<p>
          Included version verified
        </p>{:else if context.includedVersion === 'different'}<p>
          Included version differs from the observed listing
        </p>{/if}
    {/if}
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
          const actual = inboxMessageSnapshot(
            controller,
            conversationId,
            message.data.rumorId
          );
          if (actual) onreply?.(actual.data.rumorId);
        }}>Reply to this message</button
      >{/if}
  </article>
{/if}
