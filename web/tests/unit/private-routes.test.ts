import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';
import { canonicalLocalId, newLocalId } from '../../src/lib/private-handles';
import {
  conversationHref,
  draftHref,
  safeContextBack,
  staticHref
} from '../../src/lib/routes';
import { match } from '../../src/params/local_id';
import ContextBack from '../../src/lib/components/ContextBack.svelte';
import { naddrEncode } from 'applesauce-core/helpers/pointers';
import oracle from './reference-oracle.json';

const id = '6b65d774-26f1-4e7e-a414-387d347b3106';

describe('opaque private resource URLs', () => {
  it('generates canonical UUIDs with the platform randomUUID operation', () => {
    const random = vi.spyOn(globalThis.crypto, 'randomUUID');
    const ids = Array.from({ length: 32 }, () => newLocalId());
    expect(random).toHaveBeenCalledTimes(32);
    expect(new Set(ids).size).toBe(32);
    for (const value of ids) {
      expect(canonicalLocalId(value)).toBe(value);
      expect(match(value!)).toBe(true);
    }
    random.mockRestore();
  });
  it('fails safely when platform randomness is unavailable', () => {
    const random = vi
      .spyOn(globalThis.crypto, 'randomUUID')
      .mockImplementation(() => {
        throw new Error('Unavailable');
      });
    expect(newLocalId()).toBeUndefined();
    random.mockRestore();
  });
  for (const value of [
    undefined,
    null,
    {},
    '',
    id.toUpperCase(),
    ` ${id}`,
    `${id}/edit`,
    `${id}?body=hello`,
    `${id}#peer`,
    id.replace('-4e7e-', '-7e7e-'),
    id.replace('-a414-', '-0414-'),
    '0'.repeat(64),
    'x'.repeat(8193)
  ])
    it(`rejects a noncanonical private handle ${JSON.stringify(value)?.slice(0, 80) ?? 'undefined'}`, () => {
      expect(canonicalLocalId(value)).toBeUndefined();
      expect(draftHref(value)).toBeUndefined();
      expect(conversationHref(value)).toBeUndefined();
      if (typeof value === 'string') expect(match(value)).toBe(false);
    });
  it('puts only the opaque token into both private resource paths', () => {
    expect(draftHref(id)).toBe(`/selling/drafts/${id}`);
    expect(conversationHref(id)).toBe(`/messages/${id}`);
    for (const href of [draftHref(id), conversationHref(id)]) {
      const url = new URL(href!, 'https://example.invalid');
      expect(url.search).toBe('');
      expect(url.hash).toBe('');
      expect(url.pathname.endsWith(id)).toBe(true);
    }
  });
  it('treats unknown but well-formed handles as syntax, never owner/data authority', () => {
    expect(match(id)).toBe(true);
    expect(draftHref({ id, owner: 'alice', body: 'secret' })).toBeUndefined();
    expect(
      conversationHref({ id, peer: 'bob', quantity: '2', listing: 'carrots' })
    ).toBeUndefined();
    expect(safeContextBack(`/selling/drafts/${id}`)).toBeUndefined();
    expect(safeContextBack(`/messages/${id}`)).toBeUndefined();
  });
});

describe('explicit contextual navigation', () => {
  it('keeps only canonical public listing context and strips relay hints', () => {
    const coordinate = {
      kind: 30402,
      pubkey: oracle.key_cases[0].pubkey,
      identifier: 'Carrots:菜'
    };
    const canonical = naddrEncode(coordinate);
    const hinted = naddrEncode({
      ...coordinate,
      relays: ['wss://example.invalid']
    });
    expect(safeContextBack(`/products/${hinted.toUpperCase()}`)).toBe(
      `/products/${canonical}`
    );
    const output = render(ContextBack, {
      props: { href: `/products/${hinted}` }
    }).body;
    expect(output).toContain(`href="/products/${canonical}"`);
    expect(output).toContain('Back to listing');
    for (const href of [
      `/products/${hinted}/edit`,
      `/products/${hinted}?peer=alice`,
      '/products/' + 'x'.repeat(2049)
    ])
      expect(safeContextBack(href)).toBeUndefined();
  });
  for (const href of [
    '/',
    '/search',
    '/sell',
    '/selling',
    '/messages',
    '/about',
    '/privacy'
  ])
    it(`constructs the declared static route ${href}`, () => {
      expect(staticHref(href)).toBe(href);
    });
  for (const href of [
    'https://example.invalid/',
    '//example.invalid/',
    '/orders',
    '/search?returnTo=https://example.invalid',
    '/messages?peer=alice',
    '/selling#body',
    '/search/../messages',
    '/%2f%2fexample.invalid',
    '\\evil',
    {}
  ])
    it(`rejects arbitrary return context ${JSON.stringify(href)}`, () => {
      expect(safeContextBack(href)).toBeUndefined();
      expect(staticHref(href)).toBeUndefined();
      const output = render(ContextBack, {
        props: { href, fallback: '/messages' }
      }).body;
      expect(output).toContain('href="/messages"');
      expect(output).toContain('Back to messages');
    });
  it('renders a safe fallback without acquiring randomness or creating local data', () => {
    const random = vi.spyOn(globalThis.crypto, 'randomUUID');
    const output = render(ContextBack, {
      props: { href: '/messages/unknown', fallback: '//evil' }
    }).body;
    expect(output).toContain('href="/search"');
    expect(output).toContain('Search food');
    expect(random).not.toHaveBeenCalled();
    random.mockRestore();
  });
});
