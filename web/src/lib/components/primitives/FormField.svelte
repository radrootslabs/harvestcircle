<script lang="ts">
  import type { Snippet } from 'svelte';
  let {
    id,
    label,
    hint,
    error,
    control
  }: {
    id: string;
    label: string;
    hint?: string;
    error?: string;
    control: Snippet<
      [{ id: string; describedby: string | undefined; invalid: boolean }]
    >;
  } = $props();
  const describedby = $derived(
    [hint ? `${id}-hint` : '', error ? `${id}-error` : '']
      .filter(Boolean)
      .join(' ') || undefined
  );
</script>

<div class="field">
  <label class="label" for={id}>{label}</label>
  {#if hint}<p class="text-small text-muted" id={`${id}-hint`}>{hint}</p>{/if}
  {#if error}<p class="text-error" id={`${id}-error`}>{error}</p>{/if}
  {@render control({
    id,
    describedby,
    invalid: Boolean(error)
  })}
</div>
