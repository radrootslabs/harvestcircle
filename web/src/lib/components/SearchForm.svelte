<script lang="ts">
  import { onMount } from 'svelte';
  import FormField from './primitives/FormField.svelte';
  import Button from './primitives/Button.svelte';
  let {
    onsubmit,
    initialValue = '',
    error,
    disabled = false
  }: {
    onsubmit?: (input: string) => void;
    initialValue?: string;
    error?: string;
    disabled?: boolean;
  } = $props();
  const id = $props.id();
  let input = $derived(initialValue);
  let composing = $state(false);
  let ready = $state(false);
  onMount(() => {
    ready = true;
  });
  const unavailable = $derived(disabled || !onsubmit);
  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (ready && !unavailable && !composing) onsubmit?.(input);
  }
</script>

<form
  class="stack"
  onsubmit={submit}
  aria-describedby={unavailable ? `${id}-unavailable` : undefined}
>
  <FormField
    {id}
    label="What are you looking for?"
    hint="For example, carrots or carrots Victoria."
    {error}
  >
    {#snippet control({ id, describedby, invalid })}
      <textarea
        {id}
        class="input textarea"
        rows="2"
        disabled={!ready}
        aria-describedby={describedby}
        aria-invalid={invalid}
        value={input}
        oninput={(event) => {
          input = event.currentTarget.value;
        }}
        oncompositionstart={() => {
          composing = true;
        }}
        oncompositionend={() => {
          composing = false;
        }}></textarea>
    {/snippet}
  </FormField>
  <div class="cluster">
    <Button
      label="Search"
      type="submit"
      variant="primary"
      disabled={unavailable || !ready}
    />
  </div>
  {#if unavailable}
    <p class="notice" id={`${id}-unavailable`}>
      Search is unavailable during development.
    </p>
  {/if}
</form>
