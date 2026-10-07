<script lang="ts">
  import CapabilityGate from '../../../src/lib/components/CapabilityGate.svelte';
  import type { IdentitySnapshot } from '../../../src/lib/runtime/identity-session.ts';
  let {
    identity = { state: 'guest', reason: 'disconnected' },
    required = 'connection',
    onconnect,
    onprobe
  }: {
    identity?: IdentitySnapshot;
    required?: 'connection' | 'messaging';
    onconnect?: () => void;
    onprobe?: () => void;
  } = $props();
</script>

<CapabilityGate {identity} {required} {onconnect} {onprobe} />
