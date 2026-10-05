export type PublicQuery =
  | {
      readonly ok: true;
      readonly text: string;
      readonly terms: readonly string[];
    }
  | {
      readonly ok: false;
      readonly error: 'invalid_query' | 'query_too_long' | 'too_many_terms';
    };

const encoder = new TextEncoder();

export function normalizePublicQuery(input: unknown): PublicQuery {
  if (typeof input !== 'string') return { ok: false, error: 'invalid_query' };
  if (input.length > 512) return { ok: false, error: 'query_too_long' };
  if (!input.isWellFormed()) return { ok: false, error: 'invalid_query' };
  for (const character of input) {
    const code = character.charCodeAt(0);
    if (code <= 8 || (code >= 14 && code <= 31) || (code >= 127 && code <= 159))
      return { ok: false, error: 'invalid_query' };
  }
  if (encoder.encode(input).length > 512)
    return { ok: false, error: 'query_too_long' };
  const normalized = input.normalize('NFKC').toLowerCase().trim();
  const terms = normalized === '' ? [] : normalized.split(/\s+/u);
  if (terms.length > 12) return { ok: false, error: 'too_many_terms' };
  const text = terms.join(' ');
  if (encoder.encode(text).length > 512)
    return { ok: false, error: 'query_too_long' };
  return { ok: true, text, terms };
}

export function readPublicQuery(
  url: Readonly<{
    pathname: string;
    hash: string;
    search: string;
    searchParams: Readonly<Pick<URLSearchParams, 'keys' | 'get'>>;
  }>
): PublicQuery {
  if (url.pathname !== '/search' || url.hash !== '' || url.search.length > 8192)
    return { ok: false, error: 'invalid_query' };
  const keys = [...url.searchParams.keys()];
  if (keys.length > 1 || keys.some((key) => key !== 'q'))
    return { ok: false, error: 'invalid_query' };
  return normalizePublicQuery(url.searchParams.get('q') ?? '');
}

export function queryErrorMessage(
  error: 'invalid_query' | 'query_too_long' | 'too_many_terms'
): string {
  switch (error) {
    case 'query_too_long':
      return 'Use at most 512 UTF-8 bytes for your search.';
    case 'too_many_terms':
      return 'Use at most 12 words for your search.';
    case 'invalid_query':
      return 'Enter plain search words without private URL context.';
  }
}
