<script lang="ts">
  import AppShell from '../../../src/lib/components/AppShell.svelte';
  let {
    connected = false,
    available = false,
    messages = true,
    currentPath = '/',
    publicKey = 'HC_TEST_ONLY_PUBLIC_KEY_' + 'a'.repeat(64),
    onconnect,
    ondisconnect
  }: {
    connected?: boolean;
    available?: boolean;
    messages?: boolean;
    currentPath?: string;
    publicKey?: string;
    onconnect?: () => void;
    ondisconnect?: () => void;
  } = $props();
</script>

<AppShell
  {currentPath}
  availableRoutes={available
    ? [
        '/search',
        '/sell',
        ...(connected ? ['/selling' as const] : []),
        ...(connected && messages ? ['/messages' as const] : []),
        '/about',
        '/privacy'
      ]
    : []}
  identity={connected
    ? { kind: 'connected', publicKey, ondisconnect }
    : { kind: 'guest', onconnect }}
>
  <h1>Fixture route heading</h1>
</AppShell>
