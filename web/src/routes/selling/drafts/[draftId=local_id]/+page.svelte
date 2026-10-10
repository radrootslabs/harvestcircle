<script lang="ts">
  import { page } from '$app/state';
  import ListingEditor from '../../../../lib/components/ListingEditor.svelte';
  import type { ListingEditorRoute } from '../../../../lib/publishing/listing-editor.ts';
  const route = $derived({
    mode: 'draft',
    draftId: page.params.draftId ?? ''
  } satisfies ListingEditorRoute);
</script>

<ListingEditor {route} />
