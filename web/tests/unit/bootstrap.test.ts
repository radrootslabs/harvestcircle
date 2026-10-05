import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';
import Shell from './Shell.svelte';
import { prerender, ssr } from '../../src/routes/+layout';

vi.mock('$app/state', () => ({
  page: { url: new URL('https://ssr.invalid/') }
}));

describe('static guest shell', () => {
  it('keeps browser globals absent and enforces effect tripwires', () => {
    expect('window' in globalThis).toBe(false);
    expect('document' in globalThis).toBe(false);
    for (const name of ['indexedDB', 'WebSocket', 'nostr']) {
      expect(() => {
        Reflect.get(globalThis, name);
      }).toThrow(`SSR accessed ${name}`);
    }
    expect(() => fetch('https://invalid.example')).toThrow(
      'SSR attempted network access'
    );
  });
  it('imports and renders on the server without browser capabilities', () => {
    const output = render(Shell);
    expect(output.body).toMatch(/<a[^>]*href="\/"[^>]*>HarvestCircle<\/a>/);
    expect(output.head).toContain('<title>HarvestCircle</title>');
    expect(prerender).toBe(true);
    expect(ssr).toBe(true);
  });
  it('renders the shared anonymous form exactly once with an explicit search caller', () => {
    const html = render(Shell).body;
    expect(html.match(/<textarea\b/g)).toHaveLength(1);
    expect(html).toContain('What are you looking for?');
    expect(html).toContain('For example, carrots or carrots Victoria.');
    expect(html).toContain('rows="2"');
    expect(html).not.toMatch(
      /<button[^>]*type="submit"[^>]*disabled[^>]*>Search/
    );
    expect(html).not.toContain('Search is unavailable during development.');
    expect(html).not.toContain('<h1>');
  });
  it('composes the actual shared shell and preserves the route body without a global h1', () => {
    const output = render(Shell);
    expect(output.body).toMatch(
      /<main id="main-content" tabindex="-1"[^>]*>[\s\S]*<\/main>/
    );
    expect(output.body).toContain('aria-label="Primary"');
    expect(output.body).toContain('aria-label="Footer"');
    expect(output.body).toContain('Unavailable during development');
    expect(output.body).not.toContain('<h1>HarvestCircle');
    expect(output.body).not.toContain('private');
  });
});
