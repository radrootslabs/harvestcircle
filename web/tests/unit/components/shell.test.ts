import { render } from 'svelte/server';
import type { ComponentProps } from 'svelte';
import { describe, expect, it } from 'vitest';
import AppShellHarness from './AppShellHarness.svelte';

function body(props: ComponentProps<typeof AppShellHarness> = {}) {
  return render(AppShellHarness, { props }).body;
}

describe('shared application shell SSR', () => {
  it('keeps guest order, semantic landmarks, skip target and route heading', () => {
    const html = body({ available: true });
    const labels = [
      'HarvestCircle',
      'Search',
      'List food',
      'Connect extension'
    ];
    for (let index = 1; index < labels.length; index++)
      expect(html.indexOf(labels[index - 1])).toBeLessThan(
        html.indexOf(labels[index])
      );
    expect(html).toContain('<header');
    expect(html).toContain('aria-label="Primary"');
    expect(html).toContain('href="#main-content"');
    expect(html).toContain('id="main-content" tabindex="-1"');
    expect(html).toContain('<h1>Fixture route heading</h1>');
    expect(html).not.toContain('<h1>HarvestCircle');
    expect(html).toContain('<footer');
    expect(html).toContain('href="/about#help"');
  });
  it('keeps connected order, unchanged public key, native disclosure and no unread promise', () => {
    const publicKey = 'HC_TEST_ONLY_<' + 'b'.repeat(64);
    const html = body({ connected: true, available: true, publicKey });
    const labels = [
      'HarvestCircle',
      'Search',
      'Messages',
      'Selling',
      'Identity'
    ];
    for (let index = 1; index < labels.length; index++)
      expect(html.indexOf(labels[index - 1])).toBeLessThan(
        html.indexOf(labels[index])
      );
    expect(html).toContain('href="/selling"');
    expect(html).toContain('<summary>Identity</summary>');
    expect(html).toContain('HC_TEST_ONLY_&lt;' + 'b'.repeat(64));
    expect(html).toContain('Disconnect');
    expect(html).not.toMatch(
      /unread|aria-live|account|settings|Connect extension/
    );
  });
  it('marks only the real current route and never navigates an invalid current path', () => {
    const html = body({ available: true, currentPath: '/search' });
    expect(html).toMatch(/href="\/search"[^>]*aria-current="page"/);
    expect(html.match(/aria-current="page"/g)).toHaveLength(1);
    expect(body({ currentPath: '//invalid.example' })).not.toContain(
      'href="//invalid.example'
    );
  });
  it('keeps unfinished routes and missing callbacks visibly unavailable without links', () => {
    const html = body({ connected: true });
    expect(html).toContain('Unavailable during development');
    expect(html).not.toMatch(
      /href="\/(search|sell|selling|messages|about|privacy)/
    );
    expect(html).toMatch(/<button[^>]*disabled[^>]*>Disconnect/);
    expect(html).toContain('aria-disabled="true"');
    expect(html).toContain('Messages');
  });
  it('reports only currently visible unavailable routes and commands', () => {
    const forbidden = () => {
      throw new Error('SSR invoked command');
    };
    expect(body({ available: true, onconnect: forbidden })).not.toContain(
      'Unavailable during development'
    );
    const html = body({
      connected: true,
      available: true,
      messages: false,
      ondisconnect: forbidden
    });
    expect(html).toContain('Unavailable during development');
    expect(html).toContain('Messages');
    expect(html).not.toContain('href="/messages"');
  });
  it('never invokes callbacks or browser effects while server rendering', () => {
    const forbidden = () => {
      throw new Error('SSR invoked command');
    };
    expect(() => body({ onconnect: forbidden })).not.toThrow();
    expect(() =>
      body({ connected: true, ondisconnect: forbidden })
    ).not.toThrow();
  });
});
