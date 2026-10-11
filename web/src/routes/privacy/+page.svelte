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
  import {
    LOCAL_PRIVATE_BLOCKS_CONTEXT,
    PRIVATE_BLOCK_DISCLOSURE,
    localPrivateBlocksViewSnapshot,
    subscribeLocalPrivateBlocksView,
    undoLocalPrivateBlocksView,
    type LocalPrivateBlocksView
  } from '../../lib/persistence/local-private-blocks.ts';
  const blocks = getContext<LocalPrivateBlocksView | undefined>(
    LOCAL_PRIVATE_BLOCKS_CONTEXT
  );
  let privateSnapshot = $state.raw(
    blocks
      ? localPrivateBlocksViewSnapshot(blocks)
      : { state: 'guest' as const }
  );
  let privateNotice = $state<string | undefined>();
  let privateBusy = $state(false);
  onMount(() => {
    if (!blocks) return;
    privateSnapshot = localPrivateBlocksViewSnapshot(blocks);
    return subscribeLocalPrivateBlocksView(blocks, () => {
      privateSnapshot = localPrivateBlocksViewSnapshot(blocks);
      privateNotice = undefined;
    });
  });
  async function undoBlock(peer: string) {
    if (!blocks || privateBusy || privateSnapshot.state !== 'ready') return;
    const owner = privateSnapshot.owner;
    privateBusy = true;
    const result = await undoLocalPrivateBlocksView(
      blocks,
      peer,
      'reviewed_local_private_undo'
    );
    if (!alive) return;
    privateBusy = false;
    const current = localPrivateBlocksViewSnapshot(blocks);
    if (current.state !== 'ready' || current.owner !== owner) return;
    privateNotice =
      result.status === 'saved' || result.status === 'existing'
        ? 'Block undone. Eligible local views can be shown again. Another explicit bounded decryption action may be needed; Undo starts no message work.'
        : 'Undo was not confirmed. Existing local data has not been reset.';
  }
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
  <p>{PRIVATE_BLOCK_DISCLOSURE}</p>
  <h2>Local private blocks</h2>
  {#if privateSnapshot.state === 'ready'}
    {#if privateSnapshot.peers.length}
      <ul class="stack">
        {#each privateSnapshot.peers as peer (peer)}
          <li class="stack">
            <p class="key">{peer}</p>
            <Button
              label="Undo block"
              onclick={() => undoBlock(peer)}
              disabled={privateBusy}
            />
          </li>
        {/each}
      </ul>
    {:else}<p>No people blocked for this account in this browser.</p>{/if}
  {:else if privateSnapshot.state === 'guest'}
    <p>
      Connect to review account-local blocks. Guest browsing does not expose a
      private inventory.
    </p>
  {:else if privateSnapshot.state === 'loading'}
    <p>Checking local private block preferences…</p>
  {:else}
    <p class="notice">
      Local private preferences could not be checked. Previous blocks are
      unconfirmed; private rendering and decryption continuation are
      unavailable. Existing local data has not been reset.
    </p>
  {/if}
  {#if privateNotice}<p role="status">{privateNotice}</p>{/if}
</div>
