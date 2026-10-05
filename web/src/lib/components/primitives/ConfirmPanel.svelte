<script lang="ts">
  import Button from './Button.svelte';
  let {
    id,
    title,
    impact,
    confirmLabel,
    cancelLabel,
    onconfirm,
    oncancel,
    returnFocus,
    pending = false
  }: {
    id: string;
    title: string;
    impact: string;
    confirmLabel: string;
    cancelLabel: string;
    onconfirm: () => void;
    oncancel: () => void;
    returnFocus?: () => void;
    pending?: boolean;
  } = $props();
  function cancel() {
    oncancel();
    returnFocus?.();
  }
  function confirm() {
    if (!pending) onconfirm();
  }
</script>

<section
  class="notice stack"
  aria-labelledby={`${id}-title`}
  aria-describedby={`${id}-impact`}
>
  <h2 id={`${id}-title`}>{title}</h2>
  <p id={`${id}-impact`}>{impact}</p>
  <div class="cluster">
    <Button label={cancelLabel} onclick={cancel} />
    <Button
      label={confirmLabel}
      variant="primary"
      disabled={pending}
      onclick={confirm}
    />
  </div>
</section>
