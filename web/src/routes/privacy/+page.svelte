<script lang="ts">
  import { getContext, onMount } from 'svelte';
  import Button from '../../lib/components/primitives/Button.svelte';
  import {
    LOCAL_PUBLIC_HIDES_CONTEXT,
    PUBLIC_HIDE_DISCLOSURE,
    localPublicHidesSnapshot,
    subscribeLocalPublicHides,
    undoLocalPublicPublisher,
    type LocalPublicHides
  } from '../../lib/persistence/local-public-hides.ts';
  const hides = getContext<LocalPublicHides | undefined>(
    LOCAL_PUBLIC_HIDES_CONTEXT
  );
  let snapshot = $state.raw(
    hides ? localPublicHidesSnapshot(hides) : { state: 'guest' as const }
  );
  let notice = $state<string | undefined>(),
    busy = $state(false);
  let alive = false;
  onMount(() => {
    alive = true;
    const off = hides
      ? subscribeLocalPublicHides(hides, () => {
          snapshot = localPublicHidesSnapshot(hides);
          notice = undefined;
        })
      : () => {};
    if (hides) snapshot = localPublicHidesSnapshot(hides);
    return () => {
      alive = false;
      off();
    };
  });
  async function undo(publisher: string) {
    if (!hides || busy || snapshot.state !== 'ready') return;
    const owner = snapshot.owner;
    busy = true;
    const result = await undoLocalPublicPublisher(
      hides,
      publisher,
      'reviewed_local_public_undo'
    );
    if (!alive) return;
    busy = false;
    const current = localPublicHidesSnapshot(hides);
    if (current.state !== 'ready' || current.owner !== owner) return;
    notice =
      result.status === 'saved' || result.status === 'existing'
        ? 'Hide undone. Eligible public listings can be shown again; operator policy still applies.'
        : 'Undo was not confirmed. Existing local data has not been reset.';
  }
</script>

<svelte:head><title>Privacy — HarvestCircle</title></svelte:head>
<div class="page page--reading stack">
  <h1>Privacy</h1>
  <p>
    Listings are public. Relay queries can reveal your search or publisher
    interest; search URLs may remain in browser history and hosting logs. This
    prototype uses no tracking analytics.
  </p>
  <p>{PUBLIC_HIDE_DISCLOSURE}</p>
  <h2>Local publisher controls</h2>
  {#if snapshot.state === 'ready'}
    {#if snapshot.publishers.length}
      <ul class="stack">
        {#each snapshot.publishers as publisher (publisher)}
          <li class="stack">
            <p class="key">{publisher}</p>
            <Button
              label="Undo hide"
              onclick={() => undo(publisher)}
              disabled={busy}
            />
          </li>
        {/each}
      </ul>
    {:else}<p>No publishers hidden for this account in this browser.</p>{/if}
  {:else if snapshot.state === 'guest'}
    <p>
      Connect to review your account-local publisher preferences. Guest browsing
      does not expose an account inventory.
    </p>
  {:else if snapshot.state === 'loading'}
    <p>Checking local publisher preferences…</p>
  {:else}
    <p class="notice">
      Local preferences could not be checked. You can still browse public food.
      Existing local data has not been reset; previous hides are unconfirmed.
    </p>
  {/if}
  {#if notice}<p role="status">{notice}</p>{/if}
</div>
