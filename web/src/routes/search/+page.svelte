<script lang="ts">
  import { getContext, onMount, tick } from 'svelte';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import SearchForm from '../../lib/components/SearchForm.svelte';
  import ListingRow from '../../lib/components/ListingRow.svelte';
  import SourceStatus from '../../lib/components/SourceStatus.svelte';
  import Button from '../../lib/components/primitives/Button.svelte';
  import {
    normalizePublicQuery,
    queryErrorMessage,
    readPublicQuery,
    type PublicQuery
  } from '../../lib/catalog/query-input.ts';
  import { searchHref } from '../../lib/routes';
  import { internalHref } from '../../lib/navigation-url.ts';
  import {
    PUBLIC_RUNTIME_CONTEXT,
    publicRuntimeReady,
    createPublicView,
    disposePublicView,
    type PublicView,
    type PublicRuntimeContext
  } from '../../lib/runtime/public-runtime.ts';
  import {
    createSearchView,
    startSearchView,
    moreSearchView,
    olderSearchView,
    closeSearchView,
    type SearchView,
    type SearchViewSnapshot
  } from '../../lib/catalog/search-view.ts';
  import {
    captureSearchScroll,
    restoreSearchScroll
  } from '../../lib/navigation-scroll.ts';
  let pendingScroll = $state.raw<
    { query: string; position: number } | undefined
  >();
  let mounted = false;
  export const snapshot = {
    capture: () => {
      const current = readPublicQuery(page.url);
      return {
        query: current.ok ? current.text : undefined,
        position: captureSearchScroll()
      };
    },
    restore: (value: unknown) => {
      if (
        typeof value !== 'object' ||
        value === null ||
        !('query' in value) ||
        typeof value.query !== 'string' ||
        !('position' in value) ||
        typeof value.position !== 'number'
      )
        return;
      const query = normalizePublicQuery(value.query);
      if (!query.ok || query.text !== value.query) return;
      pendingScroll = { query: query.text, position: value.position };
    }
  };
  const context = getContext<PublicRuntimeContext>(PUBLIC_RUNTIME_CONTEXT);
  let query = $state<PublicQuery>(normalizePublicQuery('')),
    error = $state<string | undefined>();
  let owner = $state.raw<SearchView | undefined>(),
    result = $state.raw<SearchViewSnapshot | undefined>();
  let activeQuery: string | undefined;
  onMount(() => {
    mounted = true;
    let disposed = false;
    const unavailable = () => {
      if (!disposed) {
        owner = undefined;
        error = 'Search sources are unavailable.';
      }
    };
    try {
      void publicRuntimeReady(context)
        .then((runtime) => {
          if (disposed || !runtime) return;
          let acquired: PublicView | undefined;
          try {
            acquired = createPublicView(runtime);
            owner = createSearchView(
              acquired,
              { nowSeconds: () => Math.floor(Date.now() / 1000) },
              (value) => {
                result = value;
              }
            );
          } catch {
            // Only this page's partial view is owned here; the shared runtime
            // retains its public budget and any retryable failed disposer.
            if (acquired) disposePublicView(acquired);
            unavailable();
          }
        })
        .catch(unavailable);
    } catch {
      unavailable();
    }
    return () => {
      disposed = true;
      mounted = false;
      pendingScroll = undefined;
      if (owner) closeSearchView(owner);
    };
  });
  $effect(() => {
    query = readPublicQuery(page.url);
    error = undefined;
  });
  $effect(() => {
    if (owner && query.ok && activeQuery !== query.text) {
      const next = query.text;
      activeQuery = next;
      try {
        startSearchView(owner, next);
      } catch {
        error = 'Search could not start. Please try again.';
      }
    }
  });
  $effect(() => {
    const saved = pendingScroll,
      current = result;
    if (
      !saved ||
      !current?.listings.length ||
      !query.ok ||
      saved.query !== query.text ||
      current.query !== saved.query
    )
      return;
    void tick().then(() => {
      const currentQuery = readPublicQuery(page.url);
      if (
        mounted &&
        pendingScroll === saved &&
        result?.generation === current.generation &&
        page.url.pathname === '/search' &&
        currentQuery.ok &&
        currentQuery.text === saved.query &&
        result.query === saved.query
      ) {
        restoreSearchScroll(saved.position);
        pendingScroll = undefined;
      }
    });
  });
  function search(input: string) {
    pendingScroll = undefined;
    const next = normalizePublicQuery(input);
    if (!next.ok) {
      error = queryErrorMessage(next.error);
      return;
    }
    const target = internalHref(searchHref(next.text));
    if (target === undefined) {
      error = queryErrorMessage('invalid_query');
      return;
    }
    error = undefined;
    if (owner && query.ok && query.text === next.text) {
      try {
        startSearchView(owner, next);
      } catch {
        error = 'Search could not start. Please try again.';
      }
      return;
    }
    if (target !== undefined) void goto(target);
  }
  function more() {
    pendingScroll = undefined;
    if (owner) moreSearchView(owner);
  }
  function older() {
    pendingScroll = undefined;
    if (owner) {
      try {
        olderSearchView(owner);
      } catch {
        error =
          'Older listings could not be checked. These results may be incomplete.';
      }
    }
  }
  const noSources = $derived(
    result?.scopes
      .filter((scope) => scope.kind === 'search')
      .every(
        (scope) =>
          scope.result.sources.length === 0 ||
          scope.result.sources.every(
            (source) => source.state === 'error' || source.state === 'closed'
          )
      ) ?? false
  );
  const refresh = $derived(
    (noSources || (result?.refresh === 'error' && !result.scopes.length)) &&
      !result?.listings.length
      ? 'unavailable'
      : (result?.refresh ?? 'unavailable')
  );
</script>

<svelte:head><title>Search food — HarvestCircle</title></svelte:head>
<div class="page page--reading stack">
  <h1>Search food</h1>
  <SearchForm
    onsubmit={search}
    initialValue={query.ok ? query.text : ''}
    error={error ?? (query.ok ? undefined : queryErrorMessage(query.error))}
  />
  <SourceStatus
    {refresh}
    gap={result?.continuation.gap ?? false}
    scopes={result?.scopes ?? []}
  />
  {#if result?.listings.length}
    <p>
      {result.listings.length} listings shown in the sources checked. Newest updates
      first.
    </p>
    <ul class="stack">
      {#each result.listings as listing (listing.href)}<li>
          <ListingRow {listing} />
        </li>{/each}
    </ul>
  {:else if refresh === 'bounded-eose'}<p>
      No matches in the sources checked. Try fewer words or search older
      listings.
    </p>{/if}
  <div class="cluster">
    {#if result?.hasMore}<Button label="Show more" onclick={more} />{/if}
    {#if result?.continuation.until !== undefined}<Button
        label="Search older listings"
        onclick={older}
        disabled={result.continuation.active || result.continuation.gap}
      />{/if}
  </div>
</div>
