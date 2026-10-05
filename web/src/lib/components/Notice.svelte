<script lang="ts">
  import type { StatusCopy } from '../presentation/status-copy';
  let { tone = 'info', text }: { tone?: StatusCopy['tone']; text: string } =
    $props();
</script>

<p
  class="notice"
  class:notice--warning={tone === 'warning'}
  class:notice--error={tone === 'error'}
  role="status"
  aria-live="polite"
  aria-atomic="true"
>
  {text}
</p>
