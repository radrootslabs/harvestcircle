<script lang="ts">
  import FormField from '../../../src/lib/components/primitives/FormField.svelte';
  import Button from '../../../src/lib/components/primitives/Button.svelte';
  import Disclosure from '../../../src/lib/components/primitives/Disclosure.svelte';
  import PageHeading from '../../../src/lib/components/primitives/PageHeading.svelte';
  import ActionGroup from '../../../src/lib/components/primitives/ActionGroup.svelte';
  import EmptyState from '../../../src/lib/components/primitives/EmptyState.svelte';
  import ConfirmPanel from '../../../src/lib/components/primitives/ConfirmPanel.svelte';
  let commands = $state(0);
  let open = $state(false);
  let pending = $state(false);
  const reference = 'TEST_PUBLIC_REFERENCE_' + '0123456789abcdef'.repeat(32);
</script>

<div class="page page--reading stack">
  <PageHeading
    title="Primitive qualification fixture"
    description="Trusted test presentation only"
  />
  <FormField
    id="terms"
    label="Public terms"
    hint="Describe the public offer"
    error="Enter public terms"
  >
    {#snippet control({ id, describedby, invalid })}
      <input
        class="input"
        {id}
        aria-describedby={describedby}
        aria-invalid={invalid}
        value={reference}
      />
    {/snippet}
  </FormField>
  <ActionGroup>
    <Button
      label="Fixture command"
      variant="primary"
      onclick={() => commands++}
    />
    <Button label="Disabled command" disabled onclick={() => commands++} />
    <Button kind="link" href="#fixture-empty" label="Fixture navigation" />
  </ActionGroup>
  <p role="status" aria-live="polite">Commands: {commands}</p>
  <Disclosure summary="Public fixture details"
    ><p class="key">{reference}</p></Disclosure
  >
  <div id="fixture-empty">
    <EmptyState
      title="No fixture results"
      description="Try another fixture query"
    />
  </div>
  <button
    type="button"
    class="button button--secondary"
    id="fixture-review"
    onclick={() => (open = true)}>Review discard</button
  >
  {#if open}
    <ConfirmPanel
      id="discard"
      title="Discard this draft?"
      impact="This removes this browser draft. Remote copies remain."
      confirmLabel="Discard draft"
      cancelLabel="Keep editing"
      {pending}
      onconfirm={() => {
        commands++;
        pending = true;
      }}
      oncancel={() => {
        open = false;
        pending = false;
      }}
      returnFocus={() =>
        globalThis.document.getElementById('fixture-review')?.focus()}
    />
  {/if}
</div>
