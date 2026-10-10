<script lang="ts">
  import { getContext, onMount } from 'svelte';
  import CapabilityGate from '../../lib/components/CapabilityGate.svelte';
  import SellingRow from '../../lib/components/SellingRow.svelte';
  import InboxSetup from '../../lib/components/InboxSetup.svelte';
  import Disclosure from '../../lib/components/primitives/Disclosure.svelte';
  import { internalHref } from '../../lib/navigation-url.ts';
  import {
    IDENTITY_VIEW_CONTEXT,
    identityViewSnapshot,
    identityViewSession,
    subscribeIdentityView,
    connectIdentityView,
    disconnectIdentityView,
    type IdentityViewContext
  } from '../../lib/runtime/view-context.ts';
  import {
    PUBLIC_RUNTIME_CONTEXT,
    publicRuntimeReady,
    type PublicRuntimeContext,
    type PublicRuntime
  } from '../../lib/runtime/public-runtime.ts';
  import {
    openSellingView,
    sellingViewSnapshot,
    subscribeSellingView,
    closeSellingView,
    type SellingView,
    type SellingSnapshot
  } from '../../lib/publishing/selling-view.ts';
  const identityContext = getContext<IdentityViewContext>(
    IDENTITY_VIEW_CONTEXT
  );
  const publicContext = getContext<PublicRuntimeContext>(
    PUBLIC_RUNTIME_CONTEXT
  );
  let identity = $state(identityViewSnapshot(identityContext));
  let result = $state.raw<SellingSnapshot | undefined>();
  let unavailable = $state(false);
  onMount(() => {
    let disposed = false,
      runtimeReady = false,
      runtime: PublicRuntime | undefined;
    let owner: string | undefined,
      generation = Symbol();
    let session: ReturnType<typeof identityViewSession>;
    let view: SellingView | undefined,
      off = () => {};
    function acquire() {
      const observed = identityViewSnapshot(identityContext);
      identity = observed;
      const selected =
        observed.mounted && 'publicKey' in observed.identity
          ? observed.identity.publicKey
          : undefined;
      const selectedSession = identityViewSession(identityContext);
      if (!runtimeReady || (selected === owner && selectedSession === session))
        return;
      owner = selected;
      session = selectedSession;
      generation = Symbol();
      const original = generation;
      off();
      if (view) closeSellingView(view);
      view = undefined;
      result = undefined;
      unavailable = false;
      if (!selected) return;
      void openSellingView(identityContext, runtime)
        .then((next) => {
          if (disposed || generation !== original) {
            if (next) closeSellingView(next);
            return;
          }
          if (!next) {
            unavailable = true;
            return;
          }
          view = next;
          result = sellingViewSnapshot(next);
          off = subscribeSellingView(next, () => {
            result = sellingViewSnapshot(next);
            unavailable = !result;
          });
        })
        .catch(() => {
          if (!disposed && generation === original) unavailable = true;
        });
    }
    const identityOff = subscribeIdentityView(identityContext, acquire);
    void publicRuntimeReady(publicContext)
      .then((next) => {
        if (disposed) return;
        runtime = next;
        runtimeReady = true;
        acquire();
      })
      .catch(() => {
        if (!disposed) {
          runtimeReady = true;
          acquire();
        }
      });
    return () => {
      disposed = true;
      generation = Symbol();
      identityOff();
      off();
      if (view) closeSellingView(view);
    };
  });
  function connect() {
    return connectIdentityView(identityContext);
  }
  function disconnect() {
    disconnectIdentityView(identityContext);
  }
  const empty = $derived(
    result?.local === 'ready' &&
      result.remote === 'bounded' &&
      !result.attention.length &&
      !result.drafts.length &&
      !result.listings.length
  );
</script>

<svelte:head
  ><title>HarvestCircle</title><meta
    name="robots"
    content="noindex"
  /></svelte:head
>
<div class="page page--reading stack">
  <div class="cluster">
    <h1>Selling</h1>
    <a href={internalHref('/sell')}>List food</a>
  </div>
  <CapabilityGate
    identity={identity.identity}
    required="connection"
    focusTarget="protected"
    onconnect={identity.mounted ? connect : undefined}
    ondisconnect={disconnect}
  />
  {#if unavailable}<p class="notice">
      Saved seller work is unavailable. Existing records have not been cleared.
    </p>{/if}
  {#if empty}<p>
      No listings yet. Create a listing so people can ask about your food.
    </p>{/if}
  {#if result}
    {#if result.attention.length}<section class="stack">
        <h2>Needs attention</h2>
        {#each result.attention as row (row.key)}<SellingRow {row} />{/each}
      </section>{/if}
    {#if result.drafts.length}<section class="stack">
        <h2>Drafts</h2>
        {#each result.drafts as row (row.key)}<SellingRow {row} />{/each}
      </section>{/if}
    {#if result.listings.length}<section class="stack">
        <h2>Listings</h2>
        {#each result.listings as row (row.key)}<SellingRow {row} />{/each}
      </section>{/if}
    <p>
      {result.remote === 'checking'
        ? 'Checking bounded remote listing sources.'
        : result.remote === 'bounded'
          ? 'Remote discovery is bounded. Other relay copies may exist.'
          : 'Remote listing lookup is incomplete. Local records remain separate from remote discovery.'}
    </p>
  {/if}
  <Disclosure summary="Inbox settings"><InboxSetup /></Disclosure>
</div>
