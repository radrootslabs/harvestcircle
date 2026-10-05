import { boundedUtf8 } from './text.ts';
export type ImageDiagnostic =
  | 'food_image_shape_invalid'
  | 'food_image_url_invalid'
  | 'food_image_dimensions_missing'
  | 'food_image_dimensions_invalid'
  | 'food_image_duplicate_url'
  | 'food_image_duplicate_digest'
  | 'food_image_count_exceeded';
export type InboundImage = Readonly<{
  raw_tag: readonly string[];
  url: string | null;
  dimensions: Readonly<{ width: number; height: number }> | null;
  diagnostics: readonly ImageDiagnostic[];
  qualifies: boolean;
}>;
function structuralUrl(value: string): URL | undefined {
  if (
    !boundedUtf8(value, 4096) ||
    !value.includes('://') ||
    /[\p{White_Space}\p{Cc}\p{Cf}]/u.test(value)
  )
    return undefined;
  try {
    const url = new URL(value);
    if (
      !['http:', 'https:'].includes(url.protocol) ||
      url.username ||
      url.password
    )
      return undefined;
    const rest = value.slice(value.indexOf('://') + 3);
    const end = rest.search(/[/?#]/u);
    const authority = end < 0 ? rest : rest.slice(0, end);
    if (!authority || authority.includes('@')) return undefined;
    const suffix = end < 0 ? '' : rest.slice(end);
    const path = suffix.split(/[?#]/u)[0];
    if (!path.startsWith('/')) return undefined;
    let host: string;
    if (authority.startsWith('[')) {
      const close = authority.indexOf(']');
      if (close < 0) return undefined;
      const after = authority.slice(close + 1);
      if (after && !after.startsWith(':')) return undefined;
      host = authority.slice(1, close);
    } else {
      const colon = authority.lastIndexOf(':');
      host = colon < 0 ? authority : authority.slice(0, colon);
      if (host.includes(':')) return undefined;
    }
    if (
      !host ||
      Array.from(host).some((character) => character.codePointAt(0)! > 127)
    )
      return undefined;
    const ip =
      url.hostname.startsWith('[') ||
      /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$/u.test(url.hostname);
    if (
      !ip &&
      (host.length > 253 ||
        !host
          .split('.')
          .every(
            (label) =>
              label.length <= 63 &&
              /^[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?$/u.test(label)
          ))
    )
      return undefined;
    return url;
  } catch {
    return undefined;
  }
}
function dimension(value: string): number | undefined {
  if (
    value.length === 0 ||
    value.length > 10 ||
    !/^[1-9][0-9]*$/u.test(value) ||
    (value.length === 10 && value > '4294967295')
  )
    return undefined;
  return Number(value);
}
function dimensions(value: string): { width: number; height: number } | null {
  if (value.length > 21) return null;
  const parts = value.split('x');
  if (parts.length !== 2) return null;
  const width = dimension(parts[0]);
  const height = dimension(parts[1]);
  return width === undefined || height === undefined ? null : { width, height };
}
function digest(url: URL): string | undefined {
  // Public tolerant reader extracts only a structural root Blossom hash path.
  // Query/fragment do not change the path; no download or byte proof occurs.
  const match = /^\/([0-9a-f]{64})(?:\.([a-zA-Z0-9_.-]+))?$/u.exec(
    url.pathname
  );
  if (
    !match ||
    (match[2] && match[2].split('.').some((part) => part.length === 0))
  )
    return undefined;
  return match[1];
}
export function projectImages(tags: readonly (readonly string[])[]): Readonly<{
  images: readonly InboundImage[];
  diagnostics: readonly ImageDiagnostic[];
}> {
  const bounded = tags.slice(0, 64);
  const images: InboundImage[] = bounded.map((tag, index) => {
    const row: ImageDiagnostic[] = [];
    if (tag.length !== 3) row.push('food_image_shape_invalid');
    const parsed = tag[1] === undefined ? undefined : structuralUrl(tag[1]);
    if (!parsed) row.push('food_image_url_invalid');
    const size = tag[2] === undefined ? null : dimensions(tag[2]);
    if (tag[2] === undefined) row.push('food_image_dimensions_missing');
    else if (!size) row.push('food_image_dimensions_invalid');
    const previous = bounded.slice(0, index);
    if (tag[1] !== undefined) {
      if (previous.some((old) => old[1] === tag[1]))
        row.push('food_image_duplicate_url');
      const hash = parsed ? digest(parsed) : undefined;
      if (
        hash &&
        previous.some((old) => {
          const url = old[1] === undefined ? undefined : structuralUrl(old[1]);
          return url !== undefined && digest(url) === hash;
        })
      )
        row.push('food_image_duplicate_digest');
    }
    return {
      raw_tag: [...tag],
      url: parsed ? tag[1] : null,
      dimensions: size,
      diagnostics: row,
      qualifies: row.length === 0
    };
  });
  const overflow: ImageDiagnostic[] =
    tags.length > 64 ? ['food_image_count_exceeded'] : [];
  return {
    images,
    diagnostics: [...overflow, ...images.flatMap((image) => image.diagnostics)]
  };
}
