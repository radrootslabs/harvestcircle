import { getEventHash, type NostrEvent } from 'applesauce-core/helpers';
import { PUBLIC_INGRESS_BUDGETS } from '../config/budgets.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { boundedEnvelopeNumbers } from './envelope-bounds.ts';
import { boundedEnvelopeTags } from './verified-envelope.ts';
export type PublicSignedFields = NostrEvent;
export type LocalUnsignedTemplate = Readonly<{
  pubkey: string;
  kind: number;
  created_at: number;
  tags: readonly (readonly string[])[];
  content: string;
  hash: string;
}>;
// Hashing uses the qualified SDK. Admission conveys no signer/transport/store
// capability, supported Food profile, current-head or publishing authorization.
export function inspectLocalTemplate(
  raw: unknown,
  expectedOwner: unknown,
  expectedKind: unknown
): LocalUnsignedTemplate | undefined {
  const owner = canonicalPublicKey(expectedOwner);
  if (
    !owner ||
    ![30402, 5, 10050].some((kind) => kind === expectedKind) ||
    typeof raw !== 'string' ||
    !boundedUtf8(raw, PUBLIC_INGRESS_BUDGETS.eventBytes)
  )
    return undefined;
  try {
    const value: unknown = JSON.parse(raw);
    if (
      !exactLocalFields(value, [
        'pubkey',
        'kind',
        'created_at',
        'tags',
        'content'
      ]) ||
      JSON.stringify(value) !== raw ||
      value.pubkey !== owner ||
      value.kind !== expectedKind ||
      !boundedEnvelopeTags(value.tags) ||
      typeof value.content !== 'string' ||
      !boundedUtf8(value.content, PUBLIC_INGRESS_BUDGETS.eventBytes)
    )
      return undefined;
    const numbers = boundedEnvelopeNumbers(value.kind, value.created_at);
    if (!numbers) return undefined;
    const template = {
      pubkey: owner,
      kind: numbers.kind,
      created_at: numbers.created_at,
      tags: value.tags,
      content: value.content
    };
    return { ...template, hash: getEventHash(template) };
  } catch {
    return undefined;
  }
}
