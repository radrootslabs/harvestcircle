<script lang="ts">
  import { getContext, onMount } from 'svelte';
  import { page } from '$app/state';
  import {
    PUBLIC_RUNTIME_CONTEXT,
    publicRuntimeReady,
    createPublicView,
    disposePublicView,
    type PublicView,
    type PublicRuntime,
    type PublicRuntimeContext
  } from '../../../lib/runtime/public-runtime.ts';
  import {
    createProductView,
    closeProductView,
    type ProductView,
    type ProductViewSnapshot
  } from '../../../lib/catalog/product-view.ts';
  import { decodeProductReference } from '../../../lib/nostr/references.ts';
  import { readPublicContact } from '../../../lib/contracts/food-availability-v1/contact-read.ts';
  import { copyProductReference } from '../../../lib/navigation-copy.ts';
  import { internalHref, navigationHref } from '../../../lib/navigation-url.ts';
  import ListingFacts from '../../../lib/components/ListingFacts.svelte';
  import PublisherIdentity from '../../../lib/components/PublisherIdentity.svelte';
  import Disclosure from '../../../lib/components/primitives/Disclosure.svelte';
  import Button from '../../../lib/components/primitives/Button.svelte';
  const context = getContext<PublicRuntimeContext>(PUBLIC_RUNTIME_CONTEXT);
  let runtime = $state.raw<PublicRuntime | undefined>(),
    result = $state.raw<ProductViewSnapshot | undefined>(),
    error = $state<string | undefined>(),
    copyNotice = $state<string | undefined>();
  const reference = $derived(decodeProductReference(page.params.naddr));
  const contact = $derived(readPublicContact(result?.food?.content));
  let disposed = false;
  onMount(() => {
    const unavailable = () => {
      if (!disposed) error = 'Food sources are unavailable.';
    };
    try {
      void publicRuntimeReady(context)
        .then((value) => {
          if (!disposed) {
            runtime = value;
            if (!value) unavailable();
          }
        })
        .catch(unavailable);
    } catch {
      unavailable();
    }
    return () => {
      disposed = true;
    };
  });
  $effect(() => {
    const selected = reference,
      shared = runtime;
    result = undefined;
    copyNotice = undefined;
    if (!selected || !shared) return;
    let active = true,
      acquired: PublicView | undefined,
      owner: ProductView | undefined;
    try {
      acquired = createPublicView(shared);
      owner = createProductView(
        acquired,
        selected.naddr,
        { nowSeconds: () => Math.floor(Date.now() / 1000) },
        (value) => {
          if (active && !disposed) result = value;
        }
      );
      error = undefined;
    } catch {
      error = 'Food sources are unavailable.';
      if (acquired)
        try {
          disposePublicView(acquired);
        } catch {
          /* Shared runtime keeps a failed disposer retryable. */
        }
    }
    return () => {
      active = false;
      if (owner) closeProductView(owner);
    };
  });
  async function copyLink() {
    const selected = reference;
    if (!selected) return;
    const copied = await copyProductReference(selected.naddr, page.url.origin);
    if (!disposed && reference?.naddr === selected.naddr)
      copyNotice = copied
        ? 'Link copied.'
        : 'Copy was unavailable. Select the public link below.';
  }
</script>

<svelte:head
  ><title>{result?.food?.title ?? 'Food'} — HarvestCircle</title></svelte:head
>
<div class="page page--reading stack">
  <a href={internalHref('/search')}>Search food</a>
  <h1>{result?.food?.title ?? 'Food'}</h1>
  {#if !reference}<p class="notice">This food reference is invalid.</p>
  {:else if error || result?.outcome === 'unavailable'}<p class="notice">
      Food sources are unavailable.
    </p>
  {:else if !result || result.outcome === 'checking'}<p>
      Checking food sources…
    </p>
  {:else if result.outcome === 'unobserved'}<p class="notice">
      This listing was not observed in the sources checked. It may be
      unavailable or missing from these sources.
    </p>
  {:else if result.outcome === 'withdrawn'}<p class="notice">
      This listing was withdrawn by its publisher.
    </p>
  {:else if result.outcome === 'unsupported'}<p class="notice">
      The latest known listing uses an unsupported food format.
    </p>
  {:else if result.outcome === 'future_quarantined' || result.outcome === 'clock_unavailable'}<p
      class="notice"
    >
      Listing time could not be confirmed. Food details are unavailable.
    </p>{/if}
  {#if result}
    <PublisherIdentity publisher={result.publisher} />
    {#if result.lastKnown}<p class="notice">
        This is last-known information. The listing status check is incomplete.
      </p>{/if}
    {#if result.food && result.createdAt !== undefined}<ListingFacts
        food={result.food}
        createdAt={result.createdAt}
      />{/if}
    {#if result.outcome === 'sold'}<p class="notice">
        The seller marked this listing sold.
      </p>{/if}
    {#if result.outcome === 'active'}<p>
        Confirm availability with the seller.
      </p>
      <Button label="Message seller" disabled={true} />
      <p class="notice">
        In-app messaging is unavailable during development.
      </p>{/if}
    <Disclosure summary="Listing and source details">
      {#if result.eventId}<p>
          Listing version: <code>{result.eventId}</code>
        </p>{/if}
      {#each result.sources as source (source.result.context)}<p>
          {source.kind}: {source.state}. Coverage: {source.result.coverage}.
        </p>
        {#each source.result.sources as relay (relay.source)}<p>
            {relay.source}: {relay.state}.
          </p>{/each}{/each}
      <p>
        These bounded checks do not prove complete coverage or current stock.
      </p>
    </Disclosure>
    {#if contact && result.outcome === 'active'}<Disclosure
        summary="Other contact details"
        ><a href={navigationHref(contact.href)}>{contact.href}</a></Disclosure
      >{/if}
  {/if}
  {#if reference}<Button label="Copy link" onclick={copyLink} /><a
      href={internalHref(`/products/${reference.naddr}`)}>Public food link</a
    >{/if}
  {#if copyNotice}<p role="status">{copyNotice}</p>{/if}
</div>
