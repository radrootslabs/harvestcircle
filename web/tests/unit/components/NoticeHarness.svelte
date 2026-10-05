<script lang="ts">
  import Notice from '../../../src/lib/components/Notice.svelte';
  import OperationStatus from '../../../src/lib/components/OperationStatus.svelte';
  import ErrorSummary from '../../../src/lib/components/ErrorSummary.svelte';
</script>

<Notice tone="error" text="<img src=x>" />
<OperationStatus
  fact="unknown"
  receipts={[
    {
      role: 'archive',
      outcome: 'accepted',
      readback: 'confirmed',
      eventId: 'event-A',
      destination: 'target-A'
    }
  ]}
/>
<ErrorSummary
  errors={[
    { href: '#food', text: 'Enter food.' },
    { href: 'https://example.com', text: 'Unsafe destination' },
    { href: 'javascript:alert(1)', text: 'Unsafe scheme' }
  ]}
/>
