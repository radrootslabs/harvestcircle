import SearchForm from '../../src/lib/components/SearchForm.svelte';
import { prerender as editPrerender } from '../../src/routes/products/[naddr=naddr]/edit/+page';
import { params } from '../../src/params';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';

import Sell from '../../src/routes/sell/+page.svelte';
import Selling from '../../src/routes/selling/+page.svelte';
import Draft from '../../src/routes/selling/drafts/[draftId=local_id]/+page.svelte';
import Edit from '../../src/routes/products/[naddr=naddr]/edit/+page.svelte';
import Messages from '../../src/routes/messages/+page.svelte';
import Conversation from '../../src/routes/messages/[conversationId=local_id]/+page.svelte';
import Product from '../../src/routes/products/[naddr=naddr]/+page.svelte';
import About from '../../src/routes/about/+page.svelte';
import Privacy from '../../src/routes/privacy/+page.svelte';
import ErrorPage from '../../src/routes/+error.svelte';
import { prerender as productPrerender } from '../../src/routes/products/[naddr=naddr]/+page';
import { prerender as draftPrerender } from '../../src/routes/selling/drafts/[draftId=local_id]/+page';
import { prerender as conversationPrerender } from '../../src/routes/messages/[conversationId=local_id]/+page';

const errorState = vi.hoisted(() => ({
  status: 404,
  error: { message: 'PRIVATE_TEST_ONLY' },
  params: { naddr: 'invalid-reference' }
}));
vi.mock('$app/state', () => ({ page: errorState }));

const routeRoot = fileURLToPath(new URL('../../src/routes/', import.meta.url));
const expectedPages = [
  '+page.svelte',
  'search/+page.svelte',
  'products/[naddr=naddr]/+page.svelte',
  'sell/+page.svelte',
  'selling/+page.svelte',
  'selling/drafts/[draftId=local_id]/+page.svelte',
  'products/[naddr=naddr]/edit/+page.svelte',
  'messages/+page.svelte',
  'messages/[conversationId=local_id]/+page.svelte',
  'about/+page.svelte',
  'privacy/+page.svelte'
].sort();

describe('approved route inventory', () => {
  it('owns exactly eleven page files with the existing bounded parameter matchers', () => {
    const pages = readdirSync(routeRoot, { recursive: true })
      .filter((name): name is string => typeof name === 'string')
      .filter((name) => name.endsWith('+page.svelte'))
      .sort();
    expect(pages).toEqual(expectedPages);
  });
});

describe('thin protected route SSR', () => {
  for (const [name, component] of Object.entries({
    Sell,
    Selling,
    Draft,
    Edit,
    Messages,
    Conversation
  })) {
    it(
      name +
        ' renders only a generic unavailable connection/unlock gate without browser effects',
      () => {
        const output = render(component);
        expect(output.head).toContain('<title>HarvestCircle</title>');
        expect(output.head).toMatch(/name="robots" content="noindex"/);
        expect(output.body).toContain('Connect or unlock');
        expect(output.body).toContain(
          'Private views and editing are unavailable during development.'
        );
        expect(output.body).toMatch(
          /<button[^>]*disabled[^>]*>Connect extension/
        );
        expect(output.body).toMatch(/<button[^>]*disabled[^>]*>Unlock/);
        expect(output.body).not.toMatch(
          /<form|<textarea|<input|PRIVATE_TEST_ONLY|naddr1/
        );
        // The shared SSR setup throws on IDB, extension, socket or fetch access.
      }
    );
  }
  it('dynamic routes inherit SSR and use navigation fallback rather than fake prerendered identities', () => {
    expect([
      productPrerender,
      draftPrerender,
      conversationPrerender,
      editPrerender
    ]).toEqual([false, false, false, false]);
  });
  it('public shells honestly disclose unavailable content without invented listings or support', () => {
    for (const component of [About, Privacy]) {
      const output = render(component);
      expect(output.body).toContain('unavailable during development.');
      expect(output.body).not.toMatch(/<form|<textarea|<input|mailto:|tel:/);
      expect(output.head).not.toContain('noindex');
    }
    expect(render(About).body).toContain('id="help"');
    const details = render(Product);
    expect(details.body).toContain('This food reference is invalid.');
    expect(details.body).not.toMatch(/<form|<textarea|<input|mailto:|tel:/);
  });
  for (const status of [404, 500]) {
    it(
      'error ' +
        status +
        ' never renders arbitrary error details or route state',
      () => {
        errorState.status = status;
        const output = render(ErrorPage);
        expect(output.head).toContain('<title>HarvestCircle</title>');
        expect(output.head).toContain('noindex');
        expect(output.body).not.toContain('PRIVATE_TEST_ONLY');
        expect(output.body).toContain(
          status === 404 ? 'Not Found' : 'This page is unavailable.'
        );
      }
    );
  }
  it('robots excludes private indexes and opaque edit paths without inventing a sitemap or access control', () => {
    expect(
      readFileSync(new URL('../../static/robots.txt', import.meta.url), 'utf8')
    ).toBe(
      'User-agent: *\nDisallow: /sell\nDisallow: /selling\nDisallow: /messages\nDisallow: /products/*/edit\n'
    );
  });
});

it('the pinned Kit3 consolidated matcher adapter preserves existing bounded syntax admission', async () => {
  const valid = '12345678-1234-4234-8234-123456789abc';
  expect(await params.local_id['~standard'].validate(valid)).toEqual({
    value: valid
  });
  expect(
    await params.local_id['~standard'].validate('unknown-draft')
  ).toHaveProperty('issues');
  expect(
    await params.naddr['~standard'].validate('unsupported-coordinate')
  ).toHaveProperty('issues');
});

it('public search SSR cannot accept input or submit before client initialization', () => {
  const output = render(SearchForm, {
    props: {
      onsubmit: () => {
        throw new Error('SSR must not submit');
      }
    }
  });
  expect(output.body).toMatch(/<textarea[^>]*disabled/);
  expect(output.body).toMatch(/<button[^>]*disabled/);
  expect(output.body).not.toContain(
    'Search is unavailable during development.'
  );
});
