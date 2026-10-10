import { expect, it } from 'vitest';
import { render } from 'svelte/server';
import ListingEditor from '../../src/lib/components/ListingEditor.svelte';
import * as editor from '../../src/lib/publishing/listing-editor.ts';
import { draftHref } from '../../src/lib/routes.ts';
import type { ListingEditorRoute } from '../../src/lib/publishing/listing-editor.ts';

it('HCP116 SSR never offers save-close before genuine protected owner admission', () => {
  for (const route of [
    { mode: 'new' },
    { mode: 'draft', draftId: '00000000-0000-4000-8000-000000000001' },
    { mode: 'edit', naddr: 'unsupported' }
  ] as ListingEditorRoute[]) {
    const html = render(ListingEditor, { props: { route } }).body;
    expect(html).toContain('Connect to continue');
    expect(html).not.toMatch(/Save and close|<input|<textarea|<form/);
  }
});
it('HCP116 draft href admits only canonical durable native id, never arbitrary navigation', () => {
  expect(draftHref('00000000-0000-4000-8000-000000000001')).toBe(
    '/selling/drafts/00000000-0000-4000-8000-000000000001'
  );
  for (const value of ['../selling', 'https://outside.invalid', '', {}, null])
    expect(draftHref(value)).toBeUndefined();
});
it('HCP116 copied presentation cannot gain original save-close custody', async () => {
  const action: unknown = Reflect.get(editor, 'saveAndCloseListingEditor');
  expect(typeof action).toBe('function');
  if (typeof action !== 'function') return;
  const close = action as (value: object) => Promise<boolean>;
  expect(await close({ mode: 'draft', editable: true, saved: true })).toBe(
    false
  );
});
