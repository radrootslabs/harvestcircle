<script lang="ts">
  import FormField from './primitives/FormField.svelte';
  import Button from './primitives/Button.svelte';
  let {
    onsubmit,
    disabled = false
  }: {
    onsubmit?: (input: string) => void;
    disabled?: boolean;
  } = $props();
  const id = $props.id();
  let input = $state('');
  let composing = $state(false);
  const unavailable = $derived(disabled || !onsubmit);
  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!unavailable && !composing) onsubmit?.(input);
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
  >
    {#snippet control({ id, describedby, invalid })}
      <textarea
        {id}
        class="input textarea"
        rows="2"
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
      disabled={unavailable}
    />
  </div>
  {#if unavailable}
    <p class="notice" id={`${id}-unavailable`}>
      Search is unavailable during development.
    </p>
  {/if}
</form>
