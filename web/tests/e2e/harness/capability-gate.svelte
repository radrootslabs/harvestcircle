<script lang="ts">
  import { onMount } from 'svelte';
  import CapabilityGate from '../../../src/lib/components/CapabilityGate.svelte';
  import {
    createIdentityViewContext,
    identityViewSnapshot,
    mountIdentityView,
    subscribeIdentityView,
    connectIdentityView,
    disconnectIdentityView,
    probeIdentityViewMessaging,
    closeIdentityView
  } from '../../../src/lib/runtime/view-context.ts';
  const context = createIdentityViewContext();
  let view = $state(identityViewSnapshot(context));
  let sends = $state(0);
  onMount(() => {
    const off = subscribeIdentityView(context, (next) => {
      view = next;
    });
    mountIdentityView(context);
    return () => {
      off();
      closeIdentityView(context);
    };
  });
  function connect() {
    return connectIdentityView(context);
  }
  function probe() {
    return probeIdentityViewMessaging(context, 'reviewed_self_copy');
  }
</script>

<h1>HC_TEST_ONLY_INLINE_GATE</h1>
<p role="status">Earlier sends: {sends}</p>
<button
  type="button"
  class="button button--secondary"
  onclick={() => {
    sends++;
  }}>Explicit test Send</button
>
<CapabilityGate
  identity={view.identity}
  required="messaging"
  onconnect={view.mounted ? connect : undefined}
  onprobe={view.mounted ? probe : undefined}
/>
<button
  type="button"
  class="button button--secondary"
  onclick={() => disconnectIdentityView(context)}>Disconnect fixture</button
>
