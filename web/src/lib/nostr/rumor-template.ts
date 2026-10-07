import { getEventHash, type UnsignedEvent } from 'applesauce-core/helpers';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { messageToWireParts } from '../contracts/message-v1/index.ts';
import { safeUnsignedInteger } from './envelope-bounds.ts';
export type CanonicalPairRumor = Readonly<UnsignedEvent & { id: string }>;
// Fresh detached data only, never authenticated inbound/room, signer, store or
// Send authority. The caller supplies the observed time;074 owns clock/CAS.
export function buildCanonicalPairRumor(
  rawMessage: unknown,
  author: unknown,
  observedTime: unknown
): CanonicalPairRumor | undefined {
  const owner = canonicalPublicKey(author),
    createdAt = safeUnsignedInteger(observedTime),
    parts = messageToWireParts(rawMessage);
  if (!owner || createdAt === undefined || !parts) return undefined;
  const recipients = parts.tags.filter((row) => row[0] === 'p');
  if (recipients.length !== 1 || recipients[0]?.[1] === owner) return undefined;
  const template: UnsignedEvent = {
    pubkey: owner,
    created_at: createdAt,
    kind: 14,
    tags: parts.tags.map((row) => [...row]),
    content: parts.content
  };
  try {
    const rumor = { id: getEventHash(template), ...template };
    return boundedUtf8(JSON.stringify(rumor), 8192) ? rumor : undefined;
  } catch {
    return undefined;
  }
}
