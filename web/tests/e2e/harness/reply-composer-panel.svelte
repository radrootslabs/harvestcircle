<script lang="ts">
  import MessageComposer from '../../../src/lib/components/MessageComposer.svelte';
  import type { ReplyController } from '../../../src/lib/messaging/reply-controller.ts';
  let { controller }: { controller: ReplyController } = $props();
</script>

<section data-reply-fixture>
  <MessageComposer {controller} />
</section>
