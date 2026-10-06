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

  const publicContext = createPublicRuntimeContext();
  setContext(PUBLIC_RUNTIME_CONTEXT, publicContext);
  onMount(() => {
    mountPublicRuntime(publicContext);
    return () => closePublicRuntime(publicContext);
  });

  let { children }: { children: Snippet } = $props();
</script>

<AppShell
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
