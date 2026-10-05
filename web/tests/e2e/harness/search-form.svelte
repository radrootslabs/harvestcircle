<script lang="ts">
  import SearchForm from '../../../src/lib/components/SearchForm.svelte';
  let calls = $state<string[]>([]);
  let enabled = $state(true);
  let disabled = $state(false);
  function submit(input: string) {
    calls = [...calls, input];
  }
</script>

<div class="page page--reading stack">
  <h1>HC_TEST_ONLY_SEARCH_FORM</h1>
  <SearchForm onsubmit={enabled ? submit : undefined} {disabled} />
  <p role="status">{JSON.stringify(calls)}</p>
  <button
    type="button"
    class="button button--secondary"
    onclick={() => {
      enabled = !enabled;
    }}>Toggle caller</button
  >
  <button
    type="button"
    class="button button--secondary"
    onclick={() => {
      disabled = !disabled;
    }}>Toggle disabled</button
  >
</div>
