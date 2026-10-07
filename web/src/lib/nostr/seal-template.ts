import { getEventHash, type UnsignedEvent } from 'applesauce-core/helpers';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { safeUnsignedInteger } from './envelope-bounds.ts';
import { verifyEnvelope } from './verified-envelope.ts';
// Encoded version/size preflight only. Authentication/decryption is owned by
// the SDK and later nested validation, never inferred from base64 structure.
export function sealCiphertextV2(value: unknown): value is string {
  if (
    typeof value !== 'string' ||
    value.length < 132 ||
    !boundedUtf8(value, 16384) ||
    !/^[A-Za-z0-9+/]+={0,2}$/.test(value)
  )
    return false;
  try {
    const decoded = atob(value);
    return (
      decoded.length >= 99 &&
      decoded.charCodeAt(0) === 2 &&
      btoa(decoded) === value
    );
  } catch {
    return false;
  }
}
// Detached pure data, not an operation/signing/publication capability.
export function captureSealTemplate(
  author: unknown,
  ciphertext: unknown,
  timestamp: unknown
): string | undefined {
  const owner = canonicalPublicKey(author),
    time = safeUnsignedInteger(timestamp);
  if (!owner || time === undefined || !sealCiphertextV2(ciphertext))
    return undefined;
  const wire = JSON.stringify({
    pubkey: owner,
    kind: 13,
    created_at: time,
    tags: [],
    content: ciphertext
  });
  return boundedUtf8(wire, 16384) ? wire : undefined;
}
function own(value: object, name: string): unknown {
  const property = Object.getOwnPropertyDescriptor(value, name);
  return property && 'value' in property ? property.value : undefined;
}
// Reconstruct only fixed own data fields. No raw spread, getters, toJSON,
// iterators, extra fields or SDK cache symbols enter the retained private wire.
export function bindSealResponse(
  captured: unknown,
  response: unknown
): string | undefined {
  try {
    if (
      typeof captured !== 'string' ||
      !boundedUtf8(captured, 16384) ||
      typeof response !== 'object' ||
      response === null ||
      Array.isArray(response)
    )
      return undefined;
    const template = JSON.parse(captured) as UnsignedEvent;
    if (
      captureSealTemplate(
        template.pubkey,
        template.content,
        template.created_at
      ) !== captured
    )
      return undefined;
    for (const name of ['pubkey', 'kind', 'created_at', 'content'] as const)
      if (own(response, name) !== template[name]) return undefined;
    const tags = own(response, 'tags'),
      id = own(response, 'id'),
      sig = own(response, 'sig');
    if (
      !Array.isArray(tags) ||
      own(tags, 'length') !== 0 ||
      typeof id !== 'string' ||
      !/^[0-9a-f]{64}$/.test(id) ||
      typeof sig !== 'string' ||
      !/^[0-9a-f]{128}$/.test(sig) ||
      id !== getEventHash(template)
    )
      return undefined;
    const wire = JSON.stringify({
      pubkey: template.pubkey,
      kind: 13,
      created_at: template.created_at,
      tags: [],
      content: template.content,
      id,
      sig
    });
    return boundedUtf8(wire, 16384) && verifyEnvelope(wire).ok
      ? wire
      : undefined;
  } catch {
    return undefined;
  }
}
