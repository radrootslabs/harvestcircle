import { decodeProductReference } from './nostr/references';
import { internalHref } from './navigation-url';
import { canonicalLocalId } from './private-handles';
import { normalizePublicQuery, readPublicQuery } from './catalog/query-input';

export function searchHref(input: unknown): string | undefined {
  const query = normalizePublicQuery(input);
  if (!query.ok) return undefined;
  const params = new URLSearchParams();
  if (query.text !== '') params.set('q', query.text);
  return internalHref(
    params.size === 0 ? '/search' : `/search?${params.toString()}`
  );
}

export function staticHref(input: unknown): string | undefined {
  switch (input) {
    case '/':
    case '/search':
    case '/sell':
    case '/selling':
    case '/messages':
    case '/about':
    case '/privacy':
      return internalHref(input);
    default:
      return undefined;
  }
}

export function draftHref(input: unknown): string | undefined {
  const id = canonicalLocalId(input);
  return id === undefined ? undefined : internalHref(`/selling/drafts/${id}`);
}

export function conversationHref(input: unknown): string | undefined {
  const id = canonicalLocalId(input);
  return id === undefined ? undefined : internalHref(`/messages/${id}`);
}

export function safeContextBack(input: unknown): string | undefined {
  if (
    typeof input === 'string' &&
    input.length <= 8192 &&
    input.startsWith('/search?') &&
    !input.includes('#')
  ) {
    try {
      const query = readPublicQuery(
        new URL(input, 'https://navigation.invalid')
      );
      return query.ok ? searchHref(query.text) : undefined;
    } catch {
      return undefined;
    }
  }
  if (input === '/search' || input === '/selling' || input === '/messages')
    return internalHref(input);
  if (
    typeof input !== 'string' ||
    input.length > '/products/'.length + 2048 ||
    !input.startsWith('/products/')
  )
    return undefined;
  return productHref(input.slice('/products/'.length));
}

export function productHref(input: unknown): string | undefined {
  const reference = decodeProductReference(input);
  return reference === undefined
    ? undefined
    : internalHref(`/products/${reference.naddr}`);
}

export function productEditHref(input: unknown): string | undefined {
  const href = productHref(input);
  return href === undefined ? undefined : internalHref(`${href}/edit`);
}
