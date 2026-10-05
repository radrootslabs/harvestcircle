import { decodeProductReference } from './nostr/references';
import { internalHref } from './navigation-url';

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
