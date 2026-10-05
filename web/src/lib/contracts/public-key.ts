import { secp256k1 } from '@noble/curves/secp256k1';

// Admission matches the pinned Radroots x-only public-key type. This module
// validates public data only; it never generates keys, signs or derives secrets.
export function canonicalPublicKey(value: unknown): string | undefined {
  if (typeof value !== 'string' || !/^[0-9a-f]{64}$/.test(value))
    return undefined;
  try {
    secp256k1.ProjectivePoint.fromHex('02' + value);
    return value;
  } catch {
    return undefined;
  }
}
