<script lang="ts">
  import AppShell from '../../../src/lib/components/AppShell.svelte';
  let connected = $state(false);
  let commands = $state(0);
  let enabled = $state(true);
  const publicKey = 'HC_TEST_ONLY_PUBLIC_KEY_' + 'a'.repeat(64);
  function connect() {
    commands++;
    connected = true;
  }
  function disconnect() {
    commands++;
    connected = false;
  }
</script>

<AppShell
  currentPath="/search"
  availableRoutes={['/search', '/sell', '/selling', '/about', '/privacy']}
  identity={connected
    ? {
        kind: 'connected',
        publicKey,
        ondisconnect: enabled ? disconnect : undefined
      }
    : { kind: 'guest', onconnect: enabled ? connect : undefined }}
>
  <h1>HC_TEST_ONLY_SHELL</h1>
  <p role="status">Commands: {commands}</p>
  <button
    type="button"
    class="button button--secondary"
    onclick={() => {
      enabled = !enabled;
    }}>Toggle command availability</button
  >
</AppShell>
