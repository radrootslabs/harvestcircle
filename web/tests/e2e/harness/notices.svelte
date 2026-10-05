<script lang="ts">
  import Notice from '../../../src/lib/components/Notice.svelte';
  import OperationStatus from '../../../src/lib/components/OperationStatus.svelte';
  import ErrorSummary from '../../../src/lib/components/ErrorSummary.svelte';
  import FormField from '../../../src/lib/components/primitives/FormField.svelte';
  let input = $state('preserved food');
  let failed = $state(false);
  let text = $state(
    'Waiting for your extension. Another approval may be needed.'
  );
</script>

<div class="page page--reading stack">
  <h1>HC_TEST_ONLY_NOTICES</h1>
  <form
    class="stack"
    onsubmit={(event) => {
      event.preventDefault();
      failed = true;
    }}
  >
    <ErrorSummary
      errors={failed
        ? [
            { href: '#food', text: 'Enter a supported food description.' },
            {
              href: 'https://example.com',
              text: 'Unsafe target remains plain text.'
            }
          ]
        : []}
    />
    <FormField
      id="food"
      label="Food"
      error={failed ? 'Enter a supported food description.' : undefined}
    >
      {#snippet control({ id, describedby, invalid })}<textarea
          {id}
          class="input textarea"
          bind:value={input}
          aria-describedby={describedby}
          aria-invalid={invalid}></textarea>{/snippet}
    </FormField>
    <button type="submit" class="button button--primary">Review</button>
  </form>
  <Notice {text} />
  <button
    type="button"
    class="button button--secondary"
    onclick={() => {
      text = 'One source did not respond. These results may be incomplete.';
    }}>Update information</button
  >
  <OperationStatus
    fact="unknown"
    receipts={[
      {
        role: 'recipient',
        outcome: 'unknown',
        readback: 'confirmed',
        eventId: 'event-A',
        destination: 'recipient-target'
      },
      {
        role: 'archive',
        outcome: 'accepted',
        readback: 'unknown',
        eventId: 'event-B',
        destination: 'archive-target'
      }
    ]}
  />
</div>
