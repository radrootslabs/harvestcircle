<script lang="ts">
  import '../theme.css';
  import '../app.css';
  import { onMount, setContext, type Snippet } from 'svelte';
  import {
    createPublicRuntimeContext,
    mountPublicRuntime,
    closePublicRuntime,
    bindPublicRuntimeHides,
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
  import {
    createLocalPublicHides,
    mountLocalPublicHides,
    closeLocalPublicHides,
    LOCAL_PUBLIC_HIDES_CONTEXT
  } from '../lib/persistence/local-public-hides.ts';
  const identityContext = createIdentityViewContext();
  import {
    createLocalPrivateBlocks,
    mountLocalPrivateBlocksView,
    closeLocalPrivateBlocksView,
    LOCAL_PRIVATE_BLOCKS_CONTEXT
  } from '../lib/persistence/local-private-blocks.ts';
  const privateBlocks = createLocalPrivateBlocks(identityContext);
  setContext(LOCAL_PRIVATE_BLOCKS_CONTEXT, privateBlocks);
  const publicHides = createLocalPublicHides(identityContext);
  setContext(LOCAL_PUBLIC_HIDES_CONTEXT, publicHides);
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
    const runtime = mountPublicRuntime(publicContext);
    if (runtime) bindPublicRuntimeHides(runtime, publicHides);
    void mountLocalPublicHides(publicHides);
    void mountLocalPrivateBlocksView(privateBlocks);
    return () => {
      closeLocalPublicHides(publicHides);
      closeLocalPrivateBlocksView(privateBlocks);
      closePublicRuntime(publicContext);
    };
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
