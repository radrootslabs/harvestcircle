<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import Button from './primitives/Button.svelte';
  import {
    localDataReviewSnapshot,
    reviewLocalData,
    previewLocalDataRemoval,
    confirmLocalDataRemoval,
    cancelLocalDataRemoval,
    subscribeLocalDataReview,
    type LocalDataReview
  } from '../persistence/local-data-review.ts';
  let { review }: { review: LocalDataReview } = $props();
  let snapshot = $state.raw(untrack(() => localDataReviewSnapshot(review)));
  let selectedPublic = $state.raw<string[]>([]),
    selectedReceived = $state.raw<string[]>([]),
    selectedOutbox = $state.raw<string[]>([]);
  let feedback = $state<string | undefined>();
  const family = (value: string) =>
    value === 'public_draft'
      ? 'Listing draft'
      : value === 'public_operation'
        ? 'Publication recovery record'
        : 'Inbox preference recovery record';
  function reset() {
    selectedPublic = [];
    selectedReceived = [];
    selectedOutbox = [];
    feedback = undefined;
  }
  onMount(() => {
    snapshot = localDataReviewSnapshot(review);
    return subscribeLocalDataReview(review, () => {
      snapshot = localDataReviewSnapshot(review);
      if (snapshot.state !== 'ready') reset();
    });
  });
  function toggle(category: 'public' | 'received' | 'outbox', id: string) {
    feedback = undefined;
    if (category === 'public')
      selectedPublic = selectedPublic.includes(id)
        ? selectedPublic.filter((value) => value !== id)
        : [...selectedPublic, id];
    else if (category === 'received')
      selectedReceived = selectedReceived.includes(id)
        ? selectedReceived.filter((value) => value !== id)
        : [...selectedReceived, id];
    else
      selectedOutbox = selectedOutbox.includes(id)
        ? selectedOutbox.filter((value) => value !== id)
        : [...selectedOutbox, id];
  }
  async function inspect() {
    reset();
    await reviewLocalData(review, 'reviewed_local_inventory');
  }
  async function preview(category: 'public' | 'received' | 'outbox') {
    const result = await previewLocalDataRemoval(
      review,
      category,
      JSON.stringify(
        category === 'public'
          ? selectedPublic
          : category === 'received'
            ? selectedReceived
            : selectedOutbox
      )
    );
    feedback = result
      ? undefined
      : 'Removal preview is unavailable. Review the current data and select only eligible records.';
  }
  function keep() {
    cancelLocalDataRemoval(review);
    reset();
  }
  async function confirm() {
    reset();
    await confirmLocalDataRemoval(review, 'reviewed_local_removal');
  }
</script>

<section class="stack" aria-label="Review local data">
  <h2>Local controls</h2>
  <Button
    label="Review data in this browser"
    onclick={inspect}
    disabled={snapshot.state === 'loading' ||
      (snapshot.state === 'ready' && snapshot.busy)}
  />
  {#if snapshot.state === 'guest'}
    <p>
      Connect to review your account-local data. Guest browsing does not expose
      an account inventory.
    </p>
  {:else if snapshot.state === 'loading'}
    <p role="status">Checking this account’s local data…</p>
  {:else if snapshot.state === 'unavailable'}
    <p role="status">{snapshot.notice}</p>
  {:else if snapshot.state === 'ready'}
    <p>Account in this browser:</p>
    <p class="key" data-local-data-owner>{snapshot.owner}</p>
    <p>
      Pending or uncertain work is protected. Logical byte counts describe
      stored record text, not disk usage. No private message text is opened by
      this inventory.
    </p>
    {#if snapshot.preview}
      <h3>Review removal</h3>
      <p>
        {snapshot.preview.selected} selected {snapshot.preview.category ===
        'public'
          ? 'public records'
          : snapshot.preview.category === 'outbox'
            ? 'settled send records'
            : 'encrypted received copies'}.
      </p>
      <p>{snapshot.preview.consequences}</p>
      <div class="row">
        <Button
          label="Keep data"
          onclick={keep}
          disabled={snapshot.busy}
        /><Button
          label="Confirm removal"
          onclick={confirm}
          disabled={snapshot.busy}
        />
      </div>
    {:else}
      <h3>Listing drafts and publication records</h3>
      {#if snapshot.publicRows.length}
        <ul class="stack">
          {#each snapshot.publicRows as row (row.key)}
            <li class="stack">
              <label class="row"
                ><input
                  type="checkbox"
                  checked={selectedPublic.includes(row.key)}
                  onchange={() => toggle('public', row.key)}
                  disabled={row.state === 'protected' || snapshot.busy}
                />{family(row.family)} · {row.logicalBytes} logical bytes · {row.state ===
                'protected'
                  ? 'Protected'
                  : 'Eligible for reviewed removal'}</label
              >
              <p class="key">{row.id}</p>
            </li>
          {/each}
        </ul>
      {:else}<p>No public records for this account in this browser.</p>{/if}
      <Button
        label="Review public removal"
        onclick={() => preview('public')}
        disabled={!selectedPublic.length || snapshot.busy}
      />
      <h3>Encrypted received copies</h3>
      {#if !snapshot.receivedRemoval}<p>
          Enable messaging explicitly before reviewing received-copy removal.
          Reviewing data does not probe or enable messaging.
        </p>{/if}
      {#if snapshot.received.length}
        <ul class="stack">
          {#each snapshot.received as row (row.id)}<li class="stack">
              <label class="row"
                ><input
                  type="checkbox"
                  checked={selectedReceived.includes(row.id)}
                  onchange={() => toggle('received', row.id)}
                  disabled={!snapshot.receivedRemoval || snapshot.busy}
                />Encrypted received copy · {row.logicalBytes} logical bytes</label
              >
              <p class="key">{row.id}</p>
            </li>{/each}
        </ul>
      {:else}<p>
          No encrypted received copies for this account in this browser.
        </p>{/if}
      <Button
        label="Review received removal"
        onclick={() => preview('received')}
        disabled={!snapshot.receivedRemoval ||
          !selectedReceived.length ||
          snapshot.busy}
      />
      <h3>Private send recovery</h3>
      <p>
        {snapshot.outbox.count} retained records · {snapshot.outbox.bytes} logical
        bytes. This browser retains at most 100 send records. Settled records also
        occupy that capacity. Unsettled send records stay protected.
      </p>
      {#if !snapshot.receivedRemoval}<p>
          Enable messaging explicitly before reviewing settled-send removal.
          Reviewing data does not probe or enable messaging.
        </p>{/if}
      {#if snapshot.outboxRows.length}
        <ul class="stack">
          {#each snapshot.outboxRows as row (row.id)}
            <li class="stack">
              <label class="row"
                ><input
                  type="checkbox"
                  checked={selectedOutbox.includes(row.id)}
                  onchange={() => toggle('outbox', row.id)}
                  disabled={!snapshot.receivedRemoval ||
                    row.state === 'protected' ||
                    snapshot.busy}
                />Encrypted send recovery · {row.logicalBytes} logical bytes · {row.state ===
                'protected'
                  ? 'Protected'
                  : 'Eligible settled send'}</label
              >
              <p class="key">{row.id}</p>
            </li>
          {/each}
        </ul>
      {:else}<p>
          No private send recovery records for this account in this browser.
        </p>{/if}
      <Button
        label="Review settled send removal"
        onclick={() => preview('outbox')}
        disabled={!snapshot.receivedRemoval ||
          !selectedOutbox.length ||
          snapshot.busy}
      />
    {/if}
    {#if snapshot.busy}<p role="status">
        Checking the selected local action…
      </p>{/if}
    {#if snapshot.notice}<p role="status">{snapshot.notice}</p>{/if}
  {/if}
  {#if feedback}<p role="status">{feedback}</p>{/if}
</section>
