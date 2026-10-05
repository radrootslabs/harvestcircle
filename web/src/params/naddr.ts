import { decodeProductReference } from '../lib/nostr/references';

export function match(value: string): boolean {
  return decodeProductReference(value) !== undefined;
}
