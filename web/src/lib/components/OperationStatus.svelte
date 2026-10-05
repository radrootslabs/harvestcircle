<script lang="ts">
  import Notice from './Notice.svelte';
  import {
    statusCopy,
    receiptCopy,
    type OperationFact,
    type TargetReceipt
  } from '../presentation/status-copy';
  let {
    fact,
    receipts = []
  }: { fact: OperationFact; receipts?: readonly TargetReceipt[] } = $props();
  const summary = $derived(statusCopy(fact));
</script>

<div class="stack stack--tight">
  <Notice tone={summary.tone} text={summary.text} />
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
</div>
