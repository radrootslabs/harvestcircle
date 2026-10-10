<script lang="ts">
  import ListingEditor from '../../lib/components/ListingEditor.svelte';
  import { page } from '$app/state';
  import { canonicalLocalId } from '../../lib/private-handles.ts';
  import type { ListingEditorRoute } from '../../lib/publishing/listing-editor.ts';
  // A history return can restore the sell page under its shallow draft URL.
  // Admit that original ID on mount; the first shallow save never swaps the
  // mounted editor's resource or DOM while the user is typing.
  const resumed = canonicalLocalId(page.shallow?.params?.draftId);
  const route: ListingEditorRoute = resumed
    ? { mode: 'draft', draftId: resumed }
    : { mode: 'new' };
  import { goto, beforeNavigate } from '$app/navigation';
  import { draftHref } from '../../lib/routes.ts';
  import { internalHref } from '../../lib/navigation-url.ts';
  let pendingCurrent: (() => boolean) | undefined;
  beforeNavigate((navigation) => {
    if (navigation.shallow && pendingCurrent && !pendingCurrent())
      navigation.cancel();
  });
  async function firstSave(id: string, current: () => boolean) {
    if (!current()) return;
    pendingCurrent = current;
    try {
      const href = internalHref(draftHref(id));
      if (href !== undefined)
        await goto(href, { shallow: true, replace: true, reset: false });
      else throw Error('Unavailable draft route');
    } finally {
      if (pendingCurrent === current) pendingCurrent = undefined;
    }
  }
  async function savedClose() {
    await goto('/selling');
  }
</script>

<ListingEditor {route} onfirstsave={firstSave} onsavedclose={savedClose} />
