import { render } from 'svelte/server';
import { describe, it, expect } from 'vitest';
import Notice from '../../../src/lib/components/Notice.svelte';
import ErrorSummary from '../../../src/lib/components/ErrorSummary.svelte';
import Harness from './NoticeHarness.svelte';
describe('persistent accessible status and form summary SSR', () => {
  it('escapes plain text and politely announces without focus/dismissal', () => {
    const html = render(Notice, {
      props: { tone: 'error', text: '<img src=x>' }
    }).body;
    expect(html).toContain('&lt;img src=x>');
    expect(html).toContain('aria-live="polite"');
    expect(html).not.toMatch(/autofocus|onclick|setTimeout|<button/);
  });
  it('links only safe same-page fields and associates a heading', () => {
    const html = render(Harness).body;
    expect(html).toContain('href="#food"');
    expect(html).not.toMatch(/href="https:|href="javascript:/);
    expect(html).toContain('Unsafe destination');
    expect(html).toContain('aria-labelledby=');
    expect(html).toContain('sender archive');
    expect(html).toContain('event-A');
    expect(html).toContain('target-A');
  });
  it('does not flag fields before any errors exist', () => {
    expect(render(ErrorSummary, { props: { errors: [] } }).body).not.toContain(
      'Check the following fields'
    );
  });
});
