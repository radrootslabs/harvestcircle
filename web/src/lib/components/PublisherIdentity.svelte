<script lang="ts">
  import type { PublisherView } from '../catalog/publishers.ts';
  import Disclosure from './primitives/Disclosure.svelte';
  let { publisher }: { publisher: PublisherView } = $props();
</script>

<p>{publisher.label}</p>
<Disclosure summary="Publisher identity">
  {#if publisher.assertedName}<p>
      This name is asserted by the publisher.
    </p>{/if}
  <p>Publisher key: <code>{publisher.pubkey}</code></p>
</Disclosure>
