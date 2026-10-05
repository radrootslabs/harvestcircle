import { canonicalLocalId } from '../lib/private-handles.ts';

export function match(value: string): boolean {
  return canonicalLocalId(value) !== undefined;
}
