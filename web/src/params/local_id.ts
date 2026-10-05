import { canonicalLocalId } from '../lib/private-handles';

export function match(value: string): boolean {
  return canonicalLocalId(value) !== undefined;
}
