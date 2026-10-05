<script lang="ts">
  import { page } from '$app/state';
  import SearchForm from '../../lib/components/SearchForm.svelte';
  import {
    normalizePublicQuery,
    queryErrorMessage,
    readPublicQuery
  } from '../../lib/catalog/query-input';
  import type { PublicQuery } from '../../lib/catalog/query-input';
  import { searchHref } from '../../lib/routes';
  import { internalHref } from '../../lib/navigation-url';
  let query = $state<PublicQuery>(normalizePublicQuery(''));
  let error = $state<string | undefined>();
  $effect(() => {
    query = readPublicQuery(page.url);
    error = undefined;
  });
  function search(input: string) {
    const next = normalizePublicQuery(input);
    if (!next.ok) {
      error = queryErrorMessage(next.error);
      return;
    }
    error = undefined;
    const target = internalHref(searchHref(next.text));
    if (target === undefined) {
      error = queryErrorMessage('invalid_query');
      return;
    }
    globalThis.location.assign(target);
  }
</script>

<svelte:head><title>Search food — HarvestCircle</title></svelte:head>
<div class="page page--reading stack">
  <h1>Search food</h1>
  <SearchForm
    onsubmit={search}
    initialValue={query.ok ? query.text : ''}
    error={error ?? (query.ok ? undefined : queryErrorMessage(query.error))}
  />
  <p class="notice">Search data is unavailable during development.</p>
</div>
