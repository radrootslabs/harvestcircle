import { describe, expect, it } from 'vitest';
import {
  normalizePublicQuery,
  readPublicQuery
} from '../../src/lib/catalog/query-input';
import { searchHref, safeContextBack } from '../../src/lib/routes';

describe('bounded public search copies', () => {
  it('normalizes only a search copy using NFKC, lowercase and whitespace', () => {
    const raw = '  ＣＡＲＲＯＴＳ\nVICTORIA　菜  ';
    expect(normalizePublicQuery(raw)).toEqual({
      ok: true,
      text: 'carrots victoria 菜',
      terms: ['carrots', 'victoria', '菜']
    });
    expect(raw).toBe('  ＣＡＲＲＯＴＳ\nVICTORIA　菜  ');
    expect(searchHref(raw)).toBe('/search?q=carrots+victoria+%E8%8F%9C');
  });
  for (const raw of ['', ' \n　 '])
    it('omits empty q for bounded browse', () => {
      expect(searchHref(raw)).toBe('/search');
      expect(normalizePublicQuery(raw)).toEqual({
        ok: true,
        text: '',
        terms: []
      });
    });
  it('admits the byte boundary and rejects excess before search execution', () => {
    expect(normalizePublicQuery('a'.repeat(512)).ok).toBe(true);
    expect(normalizePublicQuery('菜'.repeat(170)).ok).toBe(true);
    for (const raw of [
      'a'.repeat(513),
      '菜'.repeat(171),
      'İ'.repeat(171),
      '㍿'.repeat(128),
      'x'.repeat(100000)
    ]) {
      expect(normalizePublicQuery(raw).ok).toBe(false);
      expect(searchHref(raw)).toBeUndefined();
    }
  });
  it('admits twelve terms but rejects thirteen', () => {
    expect(normalizePublicQuery(Array(12).fill('a').join(' ')).ok).toBe(true);
    expect(normalizePublicQuery(Array(13).fill('a').join(' ')).ok).toBe(false);
  });
  for (const raw of [
    undefined,
    null,
    {},
    { body: 'private', peer: 'alice' },
    '\ud800',
    'carrots\0private'
  ])
    it(`rejects malformed public input ${JSON.stringify(raw)}`, () => {
      expect(normalizePublicQuery(raw).ok).toBe(false);
      expect(searchHref(raw)).toBeUndefined();
    });
  it('encodes text as q without creating extra private parameters or fragments', () => {
    const href = searchHref('carrots&peer=alice#body');
    const url = new URL(href!, 'https://example.invalid');
    expect([...url.searchParams.keys()]).toEqual(['q']);
    expect(url.searchParams.get('q')).toBe('carrots&peer=alice#body');
    expect(url.hash).toBe('');
  });
});

describe('public URL query context', () => {
  it('restores the one bounded allowed q and canonicalizes back links', () => {
    const url = new URL('https://example.invalid/search?q=CARROTS%0A%E8%8F%9C');
    expect(readPublicQuery(url)).toEqual({
      ok: true,
      text: 'carrots 菜',
      terms: ['carrots', '菜']
    });
    expect(safeContextBack('/search?q=CARROTS%0A%E8%8F%9C')).toBe(
      '/search?q=carrots+%E8%8F%9C'
    );
  });
  for (const context of [
    '/search?q=carrots&body=private',
    '/search?q=a&q=b',
    '/search?q=a#peer',
    '//evil/search?q=a',
    '/search/../messages?q=a'
  ])
    it(`rejects arbitrary or private URL context ${context}`, () => {
      expect(safeContextBack(context)).toBeUndefined();
    });
  it('rejects excess URL queries and does not expose them as executable search', () => {
    for (const value of ['x'.repeat(513), Array(13).fill('a').join(' ')])
      expect(
        readPublicQuery(
          new URL(
            '/search?q=' + encodeURIComponent(value),
            'https://example.invalid'
          )
        ).ok
      ).toBe(false);
    expect(
      readPublicQuery(
        new URL('/search?body=private', 'https://example.invalid')
      ).ok
    ).toBe(false);
  });
});
