<script lang="ts">
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import ListingEditor from '../../../../lib/components/ListingEditor.svelte';
  import type { ListingEditorRoute } from '../../../../lib/publishing/listing-editor.ts';
  const route = $derived({
    mode: 'edit',
    naddr: page.params.naddr ?? ''
  } satisfies ListingEditorRoute);
</script>

<ListingEditor
  {route}
  onsavedclose={async () => {
    await goto('/selling');
  }}
/>
