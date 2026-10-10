import { expect, it } from 'vitest';
import { render } from 'svelte/server';
import ListingEditor from '../../src/lib/components/ListingEditor.svelte';
import {
  openListingEditor,
  listingEditorSnapshot,
  editListingEditor,
  flushListingEditor,
  closeListingEditor,
  subscribeListingEditor,
  type ListingEditor as Editor,
  type ListingEditorRoute
} from '../../src/lib/publishing/listing-editor.ts';
import type { IdentityViewContext } from '../../src/lib/runtime/view-context.ts';
it('HCP115 copied unmounted identity cannot admit or hydrate a protected editor', async () => {
  expect(
    await openListingEditor({} as IdentityViewContext, { mode: 'new' })
  ).toEqual({ status: 'unavailable', reason: 'owner' });
});
it('HCP115 copied success data cannot become an editor or borrow edit effects', async () => {
  const copied = {
    mode: 'new',
    editable: true,
    owner: '0'.repeat(64)
  } as unknown as Editor;
  expect(listingEditorSnapshot(copied)).toBeUndefined();
  expect(await editListingEditor(copied, {})).toBe(false);
  expect(await flushListingEditor(copied)).toBe(false);
});
it('HCP115 copied Close and subscriptions cannot retire an original owner', () => {
  const copied = {} as Editor;
  let calls = 0;
  const off = subscribeListingEditor(copied, () => {
    calls++;
  });
  closeListingEditor(copied);
  off();
  expect(calls).toBe(0);
});
it('HCP115 all three SSR routes show connection before protected values or effects', () => {
  for (const route of [
    { mode: 'new' },
    { mode: 'draft', draftId: '00000000-0000-4000-8000-000000000001' },
    { mode: 'edit', naddr: 'unsupported' }
  ] as ListingEditorRoute[]) {
    const html = render(ListingEditor, { props: { route } }).body;
    expect(html).toContain('Connect to continue');
    expect(html).not.toMatch(/<input|<textarea|<form|<select/);
    expect(html).not.toMatch(
      /Public contact details|Saved in this browser|Account-specific draft/
    );
  }
});
