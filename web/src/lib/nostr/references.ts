import { decodePointer, naddrEncode } from 'applesauce-core/helpers/pointers';
import { bech32 } from '@scure/base';
import { canonicalPublicKey } from '../contracts/public-key';

const maximumRouteBytes = 2048;
const encoder = new TextEncoder();
const strictIdentifier = new TextDecoder('utf-8', {
  fatal: true,
  ignoreBOM: true
});

export type ProductCoordinate = Readonly<{
  kind: 30402;
  pubkey: string;
  identifier: string;
}>;
export type ProductReference = ProductCoordinate & Readonly<{ naddr: string }>;

function boundedText(value: unknown): value is string {
  return (
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= maximumRouteBytes &&
    value.isWellFormed() &&
    encoder.encode(value).length <= maximumRouteBytes
  );
}

// Applesauce owns semantic pointer decoding. This bounded preflight checks the
// first identifier's raw bytes because its selected decoder replaces malformed
// UTF8 and removes one leading BOM. Unknown well-framed TLVs remain ignorable.
function identifierBytes(value: string): Uint8Array {
  const { prefix, words } = bech32.decode(value, maximumRouteBytes);
  if (prefix !== 'naddr') throw new Error('Unsupported reference');
  const bytes = Uint8Array.from(bech32.fromWords(words));
  let identifier: Uint8Array | undefined;
  for (let offset = 0; offset < bytes.length;) {
    if (offset + 2 > bytes.length) throw new Error('Truncated reference');
    const type = bytes[offset];
    const length = bytes[offset + 1];
    const end = offset + 2 + length;
    if (end > bytes.length) throw new Error('Truncated reference');
    if (type === 0 && identifier === undefined)
      identifier = bytes.slice(offset + 2, end);
    offset = end;
  }
  if (identifier === undefined) throw new Error('Missing identifier');
  return identifier;
}

export function decodeProductReference(
  value: unknown
): ProductReference | undefined {
  if (
    !boundedText(value) ||
    !/^[a-zA-Z0-9]+$/.test(value) ||
    (value !== value.toLowerCase() && value !== value.toUpperCase())
  )
    return undefined;
  try {
    const originalBytes = identifierBytes(value);
    const identifier = strictIdentifier.decode(originalBytes);
    const decoded = decodePointer(value);
    if (decoded.type !== 'naddr' || decoded.data.kind !== 30402)
      return undefined;
    const pubkey = canonicalPublicKey(decoded.data.pubkey);
    if (pubkey === undefined) return undefined;
    // The only accepted mismatch is the inspected upstream decoder's removal
    // of exactly the first leading BOM. Keep the byte-derived identifier.
    const expected = identifier.startsWith('\ufeff')
      ? identifier.slice(1)
      : identifier;
    if (decoded.data.identifier !== expected) return undefined;
    const naddr = naddrEncode({ kind: 30402, pubkey, identifier });
    if (!boundedText(naddr)) return undefined;
    const encodedBytes = identifierBytes(naddr);
    if (
      encodedBytes.length !== originalBytes.length ||
      encodedBytes.some((byte, index) => byte !== originalBytes[index])
    )
      return undefined;
    return { kind: 30402, pubkey, identifier, naddr };
  } catch {
    return undefined;
  }
}

export function encodeProductReference(
  value: Readonly<{ kind: number; pubkey: string; identifier: string }>
): string | undefined {
  const { kind, pubkey, identifier } = value;
  if (
    kind !== 30402 ||
    canonicalPublicKey(pubkey) === undefined ||
    typeof identifier !== 'string' ||
    !identifier.isWellFormed() ||
    identifier.length > 255 ||
    encoder.encode(identifier).length > 255
  )
    return undefined;
  try {
    const encoded = naddrEncode({ kind: 30402, pubkey, identifier });
    const decoded = decodeProductReference(encoded);
    return decoded?.identifier === identifier ? decoded.naddr : undefined;
  } catch {
    return undefined;
  }
}
