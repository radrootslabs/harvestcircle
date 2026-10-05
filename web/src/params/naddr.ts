import { decodeProductReference } from '../lib/nostr/references.ts';

export function match(value: string): boolean {
  return decodeProductReference(value) !== undefined;
}
