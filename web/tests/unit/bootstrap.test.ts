import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import Page from '../../src/routes/+page.svelte';
import Shell from './Shell.svelte';
import { prerender } from '../../src/routes/+layout';

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
    const output = render(Page);
    expect(output.body).toContain('<h1>HarvestCircle</h1>');
    expect(output.head).toContain('<title>HarvestCircle</title>');
    expect(prerender).toBe(true);
  });
  it('composes the root layout and route with semantic guest content', () => {
    const output = render(Shell);
    expect(output.body).toMatch(
      /<main>[\s\S]*<h1>HarvestCircle<\/h1>[\s\S]*<\/main>/
    );
    expect(output.body).not.toContain('private');
  });
});
