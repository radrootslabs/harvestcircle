import { navigationHref } from '../../navigation-url.ts';
import { boundedUtf8 } from './text.ts';
export type PublicContactInput = Readonly<{
  type: 'email' | 'phone' | 'https';
  value: string;
  public: true;
}>;
export type PublicContact = Readonly<{
  type: 'email' | 'phone' | 'https';
  href: string;
}>;
function email(value: string): boolean {
  // Deliberately bounded ASCII mailbox input, without headers or URI queries.
  if (
    value.length > 254 ||
    /[?#%]/u.test(value) ||
    !/^[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9.-]+$/u.test(value)
  )
    return false;
  const [local, domain] = value.split('@');
  return (
    local.length <= 64 &&
    !local.startsWith('.') &&
    !local.endsWith('.') &&
    !local.includes('..') &&
    domain
      .split('.')
      .every(
        (label) =>
          label.length <= 63 &&
          /^[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?$/u.test(label)
      )
  );
}
export function publicContact(input: unknown): PublicContact | undefined {
  if (
    typeof input !== 'object' ||
    input === null ||
    Array.isArray(input) ||
    !('public' in input) ||
    !('type' in input) ||
    !('value' in input)
  )
    return undefined;
  // Access known scalar fields once before validating or composing a URI.
  const { public: disclosed, type, value } = input;
  if (
    disclosed !== true ||
    typeof value !== 'string' ||
    !boundedUtf8(value, 8192)
  )
    return undefined;
  try {
    if (
      /[\p{White_Space}\p{Cc}\p{Cf}]/u.test(value) ||
      /[\p{Cc}\p{Cf}]/u.test(decodeURIComponent(value))
    )
      return undefined;
  } catch {
    return undefined;
  }
  let href: string;
  if (type === 'email') {
    if (!email(value)) return undefined;
    href = `mailto:${value}`;
  } else if (type === 'phone') {
    if (!/^\+[1-9][0-9]{1,14}$/u.test(value)) return undefined;
    href = `tel:${value}`;
  } else if (type === 'https') {
    if (!/^https:\/\//u.test(value)) return undefined;
    href = value;
  } else return undefined;
  const safe = navigationHref(href);
  if (safe === undefined) return undefined;
  return { type, href: safe };
}
