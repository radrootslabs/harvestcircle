import { render } from 'svelte/server';
import { createRawSnippet } from 'svelte';
import { describe, expect, it } from 'vitest';
import FormField from '../../../src/lib/components/primitives/FormField.svelte';
import Button from '../../../src/lib/components/primitives/Button.svelte';
import Disclosure from '../../../src/lib/components/primitives/Disclosure.svelte';
import PageHeading from '../../../src/lib/components/primitives/PageHeading.svelte';
import ActionGroup from '../../../src/lib/components/primitives/ActionGroup.svelte';
import EmptyState from '../../../src/lib/components/primitives/EmptyState.svelte';
import ConfirmPanel from '../../../src/lib/components/primitives/ConfirmPanel.svelte';

describe('presentation primitives SSR', () => {
  it('associates visible labels, hints and above-control errors', () => {
    const output = render(FormField, {
      props: {
        id: 'terms',
        label: 'Terms',
        hint: 'Public terms',
        error: 'Enter terms',
        control: createRawSnippet<
          [{ id: string; describedby: string | undefined; invalid: boolean }]
        >((get) => ({
          render() {
            const control = get();
            return `<input id="${control.id}" aria-describedby="${control.describedby}" aria-invalid="${control.invalid}">`;
          }
        }))
      }
    }).body;
    expect(output).toContain('for="terms"');
    expect(output).toContain('id="terms-hint"');
    expect(output).toContain('id="terms-error"');
    expect(output.indexOf('Enter terms')).toBeLessThan(
      output.indexOf('<input')
    );
    expect(output).toContain('aria-describedby="terms-hint terms-error"');
    expect(output).toContain('aria-invalid="true"');
  });
  it('keeps safe button semantics and finite variants', () => {
    const output = render(Button, {
      props: { label: 'Save', disabled: true }
    }).body;
    expect(output).toContain('<button');
    expect(output).toContain('type="button"');
    expect(output).toContain('disabled');
    expect(output).not.toContain('<a');
  });
  it('validates actual navigation sinks even for untyped callers', () => {
    for (const href of [
      '/search',
      'https://example.org/',
      'mailto:support@example.org'
    ]) {
      const output = render(Button, {
        props: { kind: 'link', label: 'Help', href }
      }).body;
      expect(output).toContain('<a');
      expect(output).toContain(`href="${href}"`);
    }
    const output = render(Button, {
      props: { kind: 'link', label: 'Invalid target', href: '//example.org' }
    }).body;
    expect(output).not.toContain('href=');
    expect(output).not.toContain('<a');
  });
  it('uses native disclosure and semantic presentation', () => {
    expect(
      render(Disclosure, { props: { summary: 'Details' } }).body
    ).toContain('<summary>Details</summary>');
    expect(
      render(PageHeading, {
        props: { title: 'Review', description: 'Check public terms' }
      }).body
    ).toContain('<h1>Review</h1>');
    expect(render(ActionGroup).body).toContain('class="cluster"');
    expect(
      render(EmptyState, {
        props: { title: 'No results', description: 'Try other terms' }
      }).body
    ).toContain('No results');
  });
  it('states specific impact and keeps an explicit safe exit', () => {
    const output = render(ConfirmPanel, {
      props: {
        id: 'discard',
        title: 'Discard draft?',
        impact: 'This removes this browser draft. Remote copies remain.',
        confirmLabel: 'Discard draft',
        cancelLabel: 'Keep editing',
        onconfirm: () => {},
        oncancel: () => {}
      }
    }).body;
    expect(output).toContain('aria-labelledby="discard-title"');
    expect(output).toContain('Remote copies remain.');
    expect(output).toContain('Keep editing');
    expect(output).not.toContain('role="dialog"');
  });
});
