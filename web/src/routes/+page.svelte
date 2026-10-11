<script lang="ts">
  import SearchForm from '../lib/components/SearchForm.svelte';
  import PageHeading from '../lib/components/primitives/PageHeading.svelte';
  import {
    normalizePublicQuery,
    queryErrorMessage
  } from '../lib/catalog/query-input';
  import { searchHref } from '../lib/routes';
  import { internalHref } from '../lib/navigation-url';
  let error = $state<string | undefined>();
  function search(input: string) {
    const query = normalizePublicQuery(input);
    if (!query.ok) {
      error = queryErrorMessage(query.error);
      return;
    }
    error = undefined;
    const target = internalHref(searchHref(query.text));
    if (target === undefined) {
      error = queryErrorMessage('invalid_query');
      return;
    }
    globalThis.location.assign(target);
  }
</script>

<svelte:head>
  <title>HarvestCircle</title>
</svelte:head>

<div class="page page--reading stack">
  <PageHeading
    title="Find food"
    description="Browse without connecting. Connect when you want to list food or send a message."
  />
  <SearchForm onsubmit={search} {error} />
</div>
