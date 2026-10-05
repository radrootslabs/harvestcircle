const maximumBytes = 8192;
const sentinel = 'https://navigation.invalid';

function text(value: unknown): value is string {
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    value.length > maximumBytes ||
    !value.isWellFormed() ||
    /\s/u.test(value)
  )
    return false;
  if (new TextEncoder().encode(value).length > maximumBytes) return false;
  for (const character of value) {
    const code = character.charCodeAt(0);
    if (code <= 32 || (code >= 127 && code <= 159) || character === '\\')
      return false;
  }
  try {
    const decoded = decodeURIComponent(value);
    for (const character of decoded) {
      const code = character.charCodeAt(0);
      if (code < 32 || (code >= 127 && code <= 159) || character === '\\')
        return false;
    }
  } catch {
    return false;
  }
  return true;
}

export function internalHref(value: unknown): string | undefined {
  if (!text(value)) return undefined;
  if (!(
    (value[0] === '/' && value[1] !== '/') ||
    value[0] === '?' ||
    value[0] === '#'
  ))
    return undefined;
  try {
    const url = new URL(value, sentinel);
    if (url.origin !== sentinel || url.username || url.password)
      return undefined;
    return value;
  } catch {
    return undefined;
  }
}

export function navigationHref(value: unknown): string | undefined {
  if (!text(value)) return undefined;
  const internal = internalHref(value);
  if (internal !== undefined) return internal;
  if (!/^(https:\/\/|mailto:|tel:)/i.test(value)) return undefined;
  if (/^https:/i.test(value) && !/^https:\/\/[^/?#]/i.test(value))
    return undefined;
  try {
    const url = new URL(value, sentinel);
    if (url.username || url.password || /^https:\/\/[^/?#]*@/i.test(value))
      return undefined;
    if (url.protocol === 'https:' && !url.hostname) return undefined;
    if (
      (url.protocol === 'mailto:' || url.protocol === 'tel:') &&
      (!url.pathname || url.host !== '')
    )
      return undefined;
    return value;
  } catch {
    return undefined;
  }
}
