<script lang="ts">
  import { getContext, onMount } from 'svelte';
  import CapabilityGate from './CapabilityGate.svelte';
  import InboxSetup from './InboxSetup.svelte';
  import {
    IDENTITY_VIEW_CONTEXT,
    identityViewSnapshot,
    subscribeIdentityView,
    connectIdentityView,
    disconnectIdentityView,
    probeIdentityViewMessaging,
    type IdentityViewContext
  } from '../runtime/view-context.ts';
  const context = getContext<IdentityViewContext>(IDENTITY_VIEW_CONTEXT);
  let view = $state(identityViewSnapshot(context));
  onMount(() =>
    subscribeIdentityView(context, (next) => {
      view = next;
    })
  );
  function connect() {
    return connectIdentityView(context);
  }
  function disconnect() {
    disconnectIdentityView(context);
  }
  function probe() {
    return probeIdentityViewMessaging(context, 'reviewed_self_copy');
  }
</script>

<svelte:head
  ><title>HarvestCircle</title><meta
    name="robots"
    content="noindex"
  /></svelte:head
>
<div class="page page--reading stack">
  <h1>Connect or unlock</h1>
  <CapabilityGate
    focusTarget="protected"
    ondisconnect={disconnect}
    identity={view.identity}
    required="messaging"
    onconnect={view.mounted ? connect : undefined}
    onprobe={view.mounted ? probe : undefined}
  />
  <p class="notice">
    Private views and editing are unavailable during development.
  </p>
  <InboxSetup />
</div>
