<script lang="ts">
  /* global setInterval, clearInterval */
  import { onMount } from 'svelte';
  import Notice from './Notice.svelte';
  import {
    ownedEditSourceSnapshot,
    type OwnedEditSource
  } from '../publishing/edit-source.ts';
  import type { PublicDraftForm } from '../contracts/local-records.ts';
  let {
    source,
    proposed
  }: { source: OwnedEditSource; proposed?: PublicDraftForm } = $props();
  let observed = $derived(ownedEditSourceSnapshot(source));
  onMount(() => {
    // Observe existing bounded request state; this does not open new lookups.
    const timer = setInterval(() => {
      observed = ownedEditSourceSnapshot(source);
    }, 250);
    return () => clearInterval(timer);
  });
  const conflict = $derived(
    observed?.status === 'conflict' || observed?.status === 'withdrawn'
  );
</script>

{#if observed && conflict}
  <section class="stack" aria-label="Current and proposed listing">
    <Notice
      tone="warning"
      text="This listing changed elsewhere. Review the latest version before publishing."
    />
    <p>
      Your proposed changes remain local. These versions have not been merged.
    </p>
    <h2>Current listing</h2>
    {#if observed.current}
      <dl class="stack">
        <div>
          <dt>Product name</dt>
          <dd>{observed.current.title}</dd>
        </div>
        <div>
          <dt>Full public description</dt>
          <dd class="description">{observed.current.content}</dd>
        </div>
        <div>
          <dt>Public area</dt>
          <dd>{observed.current.location}</dd>
        </div>
        <div>
          <dt>Price</dt>
          <dd>
            {observed.current.price.amount}
            {observed.current.price.currency} per {observed.current.price.unit}
          </dd>
        </div>
        <div>
          <dt>Advertised quantity</dt>
          <dd>{observed.current.quantity?.amount ?? 'Not specified'}</dd>
        </div>
        <div>
          <dt>Availability</dt>
          <dd>{observed.current.status}</dd>
        </div>
      </dl>
    {:else}<p>
        The current listing is withdrawn or unsupported. It cannot be
        overwritten here.
      </p>{/if}
    {#if proposed}
      <h2>Your proposed listing</h2>
      <dl class="stack">
        <div>
          <dt>Product name</dt>
          <dd>{proposed.title}</dd>
        </div>
        <div>
          <dt>Description</dt>
          <dd class="description">{proposed.description}</dd>
        </div>
        <div>
          <dt>Public area</dt>
          <dd>{proposed.location}</dd>
        </div>
        <div>
          <dt>Price</dt>
          <dd>{proposed.amount} {proposed.currency} per {proposed.unit}</dd>
        </div>
        <div>
          <dt>Advertised quantity</dt>
          <dd>{proposed.quantity || 'Not specified'}</dd>
        </div>
        <div>
          <dt>Public contact</dt>
          <dd>
            {proposed.contactType
              ? proposed.contactType + ': ' + proposed.contactValue
              : 'Not included'}
          </dd>
        </div>
      </dl>
    {/if}
  </section>
{:else if observed && observed.status !== 'ready'}
  <Notice
    tone="info"
    text={observed.status === 'checking'
      ? 'Checking the current listing and removal requests. Your local values remain here.'
      : 'The current listing check is incomplete. Keep your local values; this does not prove that another revision is absent.'}
  />
{/if}
