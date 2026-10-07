<script lang="ts">
  import '../theme.css';
  import '../app.css';
  import { onMount, setContext, type Snippet } from 'svelte';
  import {
    createPublicRuntimeContext,
    mountPublicRuntime,
    closePublicRuntime,
    PUBLIC_RUNTIME_CONTEXT
  } from '../lib/runtime/public-runtime.ts';
  import { page } from '$app/state';
  import AppShell from '../lib/components/AppShell.svelte';
  import {
    createIdentityViewContext,
    IDENTITY_VIEW_CONTEXT,
    identityViewSnapshot,
    mountIdentityView,
    subscribeIdentityView,
    connectIdentityView,
    disconnectIdentityView,
    closeIdentityView,
    invalidateIdentityView
  } from '../lib/runtime/view-context.ts';
  const identityContext = createIdentityViewContext();
  setContext(IDENTITY_VIEW_CONTEXT, identityContext);
  let identityView = $state(identityViewSnapshot(identityContext));
  $effect(() => {
    if (page.url.pathname) invalidateIdentityView(identityContext);
  });
  function connect() {
    return connectIdentityView(identityContext);
  }
  function disconnect() {
    disconnectIdentityView(identityContext);
  }
  const shellIdentity = $derived(
    identityView.identity.state === 'guest' ||
      identityView.identity.state === 'pending'
      ? {
          kind: 'guest' as const,
          onconnect: identityView.mounted ? connect : undefined
        }
      : {
          kind: 'connected' as const,
          publicKey: identityView.identity.publicKey,
          ondisconnect: disconnect
        }
  );

  const publicContext = createPublicRuntimeContext();
  setContext(PUBLIC_RUNTIME_CONTEXT, publicContext);
  onMount(() => {
    mountPublicRuntime(publicContext);
    return () => closePublicRuntime(publicContext);
  });
  onMount(() => {
    const off = subscribeIdentityView(identityContext, (next) => {
      identityView = next;
    });
    mountIdentityView(identityContext);
    return () => {
      off();
      closeIdentityView(identityContext);
    };
  });

  let { children }: { children: Snippet } = $props();
</script>

<AppShell
  identity={shellIdentity}
  capability={identityView.identity}
  ondisconnect={disconnect}
  currentPath={page.url.pathname}
  availableRoutes={[
    '/search',
    '/sell',
    '/selling',
    '/messages',
    '/about',
    '/privacy'
  ]}
>
  {@render children()}
</AppShell>
