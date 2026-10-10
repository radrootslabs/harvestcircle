<script lang="ts">
  import Notice from './Notice.svelte';
  import {
    listingOperationSnapshot,
    subscribeListingOperationView,
    type ListingOperationView
  } from '../publishing/listing-operation-view.ts';
  import {
    statusCopy,
    receiptCopy,
    type OperationFact,
    type TargetReceipt
  } from '../presentation/status-copy';
  let {
    fact = 'unknown',
    receipts = [],
    publicOperation
  }: {
    fact?: OperationFact;
    receipts?: readonly TargetReceipt[];
    publicOperation?: ListingOperationView;
  } = $props();
  let retired = $state(false);
  const publicSnapshot = $derived(
    publicOperation && !retired
      ? listingOperationSnapshot(publicOperation)
      : undefined
  );
  const summary = $derived(
    publicOperation
      ? (publicSnapshot?.summary ?? statusCopy('unknown'))
      : statusCopy(fact)
  );
  $effect(() => {
    retired = false;
    if (!publicOperation) return;
    return subscribeListingOperationView(publicOperation, () => {
      retired = true;
    });
  });
</script>

<div class="stack stack--tight">
  <Notice tone={summary.tone} text={summary.text} />
  {#if publicSnapshot}
    {#if publicSnapshot.purpose === 'withdrawal'}<p>
        Removal request evidence remains here even when the listing is
        suppressed. It cannot erase all other copies.
      </p>{/if}
    <p>
      Relay acceptance recorded for {publicSnapshot.acceptanceCount} of {publicSnapshot
        .targets.length} targets.
    </p>
    <p>
      Exact read-back recorded for {publicSnapshot.readbackCount} of {publicSnapshot
        .targets.length} targets.
    </p>
    <p>
      {publicSnapshot.purpose === 'withdrawal'
        ? 'This request has its own retained local operation. Listing visibility does not determine its delivery outcome.'
        : publicSnapshot.localSource === 'retained-prototype'
          ? 'Retained prototype source is available in this browser. Editing requires a separate current-head check.'
          : 'No supported retained prototype source is available for editing. Keep this listing read-only.'}
    </p>
    {#if publicSnapshot.needsAction}
      <Notice
        tone="warning"
        text="Needs action. Earlier acceptance and exact read-back facts are preserved."
      />
    {/if}
    {#each publicSnapshot.targets as target (target.origin)}
      <div class="stack stack--tight">
        <p class="key">Event: {target.eventId} · Target: {target.origin}</p>
        <p>
          {target.accepted
            ? 'Acceptance recorded for this event and target.'
            : 'No acceptance recorded for this event and target.'}
        </p>
        <p>
          {target.readback === 'confirmed'
            ? 'Exact read-back is confirmed for this event and target.'
            : 'Exact read-back was not observed for this event and target.'}
        </p>
        {#each target.receipts as receipt, index (index)}
          <p>
            Attempt {receipt.attempt} · Recorded outcome: {receipt.status ===
            'timed_out'
              ? 'timed out'
              : receipt.status}.
          </p>
        {/each}
      </div>
    {/each}
  {:else if !publicOperation}
    {#each receipts as receipt, index (index)}
      {@const copy = receiptCopy(receipt)}
      <div class="stack stack--tight">
        <p class="key">
          Event: {receipt.eventId} · Target: {receipt.destination}
        </p>
        <Notice tone={copy.tone} text={copy.text} />
        <p>{copy.readback}</p>
      </div>
    {/each}
  {/if}
</div>
