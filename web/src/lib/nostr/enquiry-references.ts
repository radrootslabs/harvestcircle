import { neventEncode, decodePointer } from 'applesauce-core/helpers/pointers';
import { encodeProductReference } from './references.ts';
import {
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from './verified-envelope.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
export type EnquiryReferences = Readonly<{
  peer: string;
  eventId: string;
  identifier: string;
  naddr: string;
  nevent: string;
}>;
// A body pointer is only a bounded public-version selector, never proof. Strip
// relay hints and require its asserted kind and author before any public query.
export function includedProductVersion(
  value: string,
  author: string
): string | undefined {
  if (!boundedUtf8(value, 2048)) return undefined;
  try {
    const pointer = decodePointer(value);
    return pointer.type === 'nevent' &&
      pointer.data.kind === 30402 &&
      pointer.data.author === author &&
      /^[a-f0-9]{64}$/.test(pointer.data.id)
      ? pointer.data.id
      : undefined;
  } catch {
    return undefined;
  }
}
// Public signed listing proofs only. No body links, private parent IDs or relay
// hints enter this projection; references never perform a lookup or fetch.
export function enquiryReferences(
  proof: VerifiedEnvelope
): EnquiryReferences | undefined {
  const event = verifiedEnvelopeSnapshot(proof);
  if (!event || event.kind !== 30402) return undefined;
  const identifier = event.tags.find((tag) => tag[0] === 'd')?.[1];
  if (typeof identifier !== 'string') return undefined;
  const naddr = encodeProductReference({
    kind: 30402,
    pubkey: event.pubkey,
    identifier
  });
  if (!naddr) return undefined;
  try {
    const nevent = neventEncode({
      id: event.id,
      author: event.pubkey,
      kind: 30402
    });
    const decoded = decodePointer(nevent);
    if (
      !boundedUtf8(nevent, 2048) ||
      decoded.type !== 'nevent' ||
      decoded.data.id !== event.id ||
      decoded.data.author !== event.pubkey ||
      decoded.data.kind !== 30402 ||
      decoded.data.relays?.length
    )
      return undefined;
    return { peer: event.pubkey, eventId: event.id, identifier, naddr, nevent };
  } catch {
    return undefined;
  }
}
