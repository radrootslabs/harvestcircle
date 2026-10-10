import { expect, it } from 'vitest';
import { render } from 'svelte/server';
import ListingEditor from '../../src/lib/components/ListingEditor.svelte';
import ListingReview from '../../src/lib/components/ListingReview.svelte';
import {
  validateEditorForm,
  editorReviewSnapshot,
  closeEditorReview,
  type EditorReview
} from '../../src/lib/publishing/editor-workflow.ts';

const form = {
  title: '菜',
  description: '  Complete\r\npublic description 菜  ',
  location: 'Town',
  amount: '2.5',
  currency: 'GBP',
  unit: 'kg',
  quantity: '3',
  contactType: '' as const,
  contactValue: ''
};
it('HCP117 validation preserves raw values and uses the exact Food writer bounds', () => {
  const before = JSON.stringify(form);
  expect(validateEditorForm(form)).toEqual([]);
  expect(JSON.stringify(form)).toBe(before);
  for (const [change, field] of [
    [{ title: 'x'.repeat(161) }, 'title'],
    [{ description: ' ' }, 'description'],
    [{ description: '菜'.repeat(6000) }, 'description'],
    [{ amount: '2,50' }, 'amount'],
    [{ amount: '2.50' }, 'amount'],
    [{ quantity: '03' }, 'quantity'],
    [{ contactValue: 'unapproved private contact' }, 'contactValue'],
    [
      { contactType: 'https', contactValue: 'javascript:alert(1)' },
      'contactValue'
    ]
  ] as const) {
    const input = { ...form, ...change };
    const original = JSON.stringify(input);
    expect(validateEditorForm(input)[0]?.field).toBe(field);
    expect(JSON.stringify(input)).toBe(original);
  }
});
it('HCP117 copied display values cannot mint a review or disclose a fabricated template', () => {
  const fake = {
    owner: 'a'.repeat(64),
    content: 'FABRICATED_PUBLIC_REVIEW'
  } as unknown as EditorReview;
  expect(editorReviewSnapshot(fake)).toBeUndefined();
  closeEditorReview(fake);
  const html = render(ListingReview, {
    props: { review: fake, onedit: () => {} }
  }).body;
  expect(html).not.toContain('FABRICATED_PUBLIC_REVIEW');
  expect(html).not.toMatch(/Publish|<form|<input|<textarea/);
});
it('HCP117 SSR connection gate exposes no protected review or inbox effects', () => {
  const html = render(ListingEditor, {
    props: { route: { mode: 'new' } }
  }).body;
  expect(html).toContain('Connect to continue');
  expect(html).not.toMatch(
    /Review listing|Public review|Inbox setup|<form|<textarea|<input/
  );
});
