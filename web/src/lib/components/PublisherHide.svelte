<script lang="ts">
  import { getContext, onMount } from 'svelte';
  import Button from './primitives/Button.svelte';
  import {
    productViewPublisherProof,
    type ProductView
  } from '../catalog/product-view.ts';
  import {
    LOCAL_PUBLIC_HIDES_CONTEXT,
    PUBLIC_HIDE_DISCLOSURE,
    localPublicHidesSnapshot,
    subscribeLocalPublicHides,
    hideLocalPublicPublisher,
    type LocalPublicHides
  } from '../persistence/local-public-hides.ts';
  let { view }: { view: ProductView } = $props();
  const hides = getContext<LocalPublicHides | undefined>(
    LOCAL_PUBLIC_HIDES_CONTEXT
  );
  let snapshot = $state.raw(
    hides ? localPublicHidesSnapshot(hides) : { state: 'guest' as const }
  );
  let busy = $state(false),
    notice = $state<string | undefined>();
  let alive = false;
  onMount(() => {
    alive = true;
    const off = hides
      ? subscribeLocalPublicHides(hides, () => {
          snapshot = localPublicHidesSnapshot(hides);
        })
      : () => {};
    if (hides) snapshot = localPublicHidesSnapshot(hides);
    return () => {
      alive = false;
      off();
    };
  });
  async function hide() {
    const selected = view;
    const proof = productViewPublisherProof(selected);
    if (!hides || !proof || busy || snapshot.state !== 'ready') return;
    busy = true;
    const result = await hideLocalPublicPublisher(
      hides,
      proof,
      'reviewed_local_public_hide'
    );
    if (!alive || view !== selected) return;
    busy = false;
    const current = localPublicHidesSnapshot(hides);
    if (
      current.state !== 'ready' ||
      snapshot.state !== 'ready' ||
      current.owner !== snapshot.owner
    ) {
      notice = undefined;
      return;
    }
    notice =
      result.status === 'saved' || result.status === 'existing'
        ? 'Publisher hidden in this browser. Undo is available in Privacy.'
        : 'The hide was not confirmed. Existing local data has not been reset; review Privacy before trying again.';
  }
</script>

<div class="stack">
  <p>{PUBLIC_HIDE_DISCLOSURE}</p>
  {#if snapshot.state === 'ready'}
    <Button label="Hide publisher locally" onclick={hide} disabled={busy} />
  {:else if snapshot.state === 'guest'}
    <p>Connect to use account-local publisher controls.</p>
  {:else if snapshot.state === 'loading'}
    <p>Checking local publisher preferences…</p>
  {:else}
    <p class="notice">
      Local preferences could not be checked. You can browse public food;
      existing local data has not been reset.
    </p>
  {/if}
  {#if notice}<p role="status">{notice}</p>{/if}
</div>
