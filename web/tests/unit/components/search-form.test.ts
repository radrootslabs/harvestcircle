import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import SearchForm from '../../../src/lib/components/SearchForm.svelte';

describe('shared labelled search form SSR', () => {
  it('renders a public URL query and linked field error without invoking navigation', () => {
    const html = render(SearchForm, {
      props: {
        initialValue: 'carrots',
        error: 'Use at most 12 words.',
        onsubmit: () => {
          throw new Error('SSR navigation');
        }
      }
    }).body;
    expect(html).toContain('carrots');
    expect(html).toContain('Use at most 12 words.');
    expect(html).toContain('aria-invalid="true"');
    const id = html.match(/<textarea[^>]* id="([^"]+)"/)?.[1];
    expect(html).toContain(`id="${id}-error"`);
    expect(html).toContain(`aria-describedby="${id}-hint ${id}-error"`);
  });
  it('renders one two-row textarea, visible associated label and example hint', () => {
    const html = render(SearchForm).body;
    expect(html.match(/<textarea\b/g)).toHaveLength(1);
    expect(html).toContain('rows="2"');
    expect(html).toContain('What are you looking for?');
    expect(html).toContain('For example, carrots or carrots Victoria.');
    const id = html.match(/<textarea[^>]* id="([^"]+)"/)?.[1];
    expect(id).toBeTruthy();
    expect(html).toContain(`for="${id}"`);
    expect(html).toContain(`aria-describedby="${id}-hint"`);
    expect(html).toContain(`id="${id}-hint"`);
  });
  it('uses native form submission and visibly disables absent callers', () => {
    const html = render(SearchForm).body;
    expect(html).toContain('<form');
    expect(html).toMatch(/<button[^>]*type="submit"[^>]*disabled[^>]*>Search/);
    expect(html).toContain('Search is unavailable during development.');
    expect(html).not.toMatch(/action=|placeholder=|maxlength=|required/);
  });
  it('enables an explicit typed caller without invoking it during SSR', () => {
    const html = render(SearchForm, {
      props: {
        onsubmit: (input: string) => {
          throw new Error(`SSR invoked search caller: ${input}`);
        }
      }
    }).body;
    expect(html).not.toMatch(/<button[^>]*disabled/);
    expect(html).not.toContain('Search is unavailable');
  });
  it('honors explicit disabled even with a caller', () => {
    const html = render(SearchForm, {
      props: {
        disabled: true,
        onsubmit: () => {
          throw new Error('SSR invoked disabled caller');
        }
      }
    }).body;
    expect(html).toMatch(/<button[^>]*disabled/);
    expect(html).toContain('Search is unavailable');
  });
  it('does not add search policy, onboarding or browser effects', () => {
    const html = render(SearchForm).body;
    expect(html).not.toMatch(
      /<h1|location|signup|AI|WebSocket|nostr|\/search\?/
    );
  });
});
