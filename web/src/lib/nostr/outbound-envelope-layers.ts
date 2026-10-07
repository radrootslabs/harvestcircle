import { getEventHash, type UnsignedEvent } from 'applesauce-core/helpers';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import {
  messageFromWireParts,
  messageToWireParts
} from '../contracts/message-v1/index.ts';
import { safeUnsignedInteger } from './envelope-bounds.ts';
import { verifyEnvelope } from './verified-envelope.ts';

function fields(
  value: unknown,
  names: string[]
): value is Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    return false;
  let count = 0;
  for (const name in value) {
    if (!names.includes(name)) return false;
    count++;
  }
  return count === names.length && names.every((name) => name in value);
}
// Detached structural/signature inspection only. Ciphertext correspondence and
// genuine original reservation custody belong to the factory-owned proof.
export function verifyOutboundLayerData(
  original: unknown,
  rumorWire: unknown,
  sealWire: unknown,
  outerWire: unknown,
  author: unknown,
  recipient: unknown,
  role: unknown
): boolean {
  const owner = canonicalPublicKey(author),
    peer = canonicalPublicKey(recipient);
  if (
    !owner ||
    !peer ||
    owner === peer ||
    (role !== 'peer' && role !== 'self') ||
    typeof original !== 'string' ||
    typeof rumorWire !== 'string' ||
    original !== rumorWire ||
    !boundedUtf8(rumorWire, 8192) ||
    typeof sealWire !== 'string' ||
    !boundedUtf8(sealWire, 16384) ||
    typeof outerWire !== 'string' ||
    !boundedUtf8(outerWire, 32768)
  )
    return false;
  try {
    const rumor: unknown = JSON.parse(rumorWire),
      seal: unknown = JSON.parse(sealWire),
      outer: unknown = JSON.parse(outerWire);
    const unsigned = ['id', 'pubkey', 'created_at', 'kind', 'tags', 'content'],
      signed = [...unsigned, 'sig'];
    if (
      !fields(rumor, unsigned) ||
      !fields(seal, signed) ||
      !fields(outer, signed) ||
      rumor.pubkey !== owner ||
      rumor.kind !== 14 ||
      safeUnsignedInteger(rumor.created_at) === undefined ||
      seal.pubkey !== owner ||
      seal.kind !== 13 ||
      !Array.isArray(seal.tags) ||
      seal.tags.length !== 0 ||
      outer.kind !== 1059 ||
      !Array.isArray(outer.tags) ||
      JSON.stringify(outer.tags) !==
        JSON.stringify([['p', role === 'self' ? owner : peer]]) ||
      !verifyEnvelope(sealWire).ok ||
      !verifyEnvelope(outerWire).ok
    )
      return false;
    const parts = { kind: 14, tags: rumor.tags, content: rumor.content };
    const decoded = messageFromWireParts(JSON.stringify(parts));
    if (
      !decoded ||
      decoded.recipients.length !== 1 ||
      decoded.recipients[0].public_key !== peer
    )
      return false;
    const canonical = messageToWireParts(JSON.stringify(decoded));
    if (!canonical || JSON.stringify(canonical) !== JSON.stringify(parts))
      return false;
    const template: UnsignedEvent = {
      pubkey: owner,
      created_at: rumor.created_at as number,
      kind: 14,
      tags: canonical.tags.map((row) => [...row]),
      content: canonical.content
    };
    return typeof rumor.id === 'string' && rumor.id === getEventHash(template);
  } catch {
    return false;
  }
}
