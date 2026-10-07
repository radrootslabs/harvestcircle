import {
  getEventHash,
  verifyEvent,
  type NostrEvent
} from 'applesauce-core/helpers';
import { PUBLIC_INGRESS_BUDGETS } from '../config/budgets.ts';
import { boundedEnvelopeNumbers } from './envelope-bounds.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';

declare const verified: unique symbol;
export type VerifiedEnvelope = Readonly<{ readonly [verified]: true }>;
export type EnvelopeFailure = Readonly<{
  code: 'envelope_invalid' | 'event_id_mismatch' | 'signature_invalid';
  message: string;
}>;
// Weak ownership bounds lifetime to callers. Retained bytes cannot be mutated
// through detached snapshots, nor forged by a type assertion or SDK cache flag.
const snapshots = new WeakMap<VerifiedEnvelope, string>();
const invalid: EnvelopeFailure = Object.freeze({
  code: 'envelope_invalid',
  message: 'invalid or unsupported bounded NIP-01 envelope'
});
export function boundedEnvelopeTags(tags: unknown): tags is string[][] {
  if (!Array.isArray(tags) || tags.length > 1024) return false;
  let elements = 0;
  let bytes = 0;
  const encoder = new TextEncoder();
  for (const tag of tags) {
    if (
      !Array.isArray(tag) ||
      tag.length === 0 ||
      typeof tag[0] !== 'string' ||
      tag[0].length === 0 ||
      /\p{Cc}/u.test(tag[0])
    )
      return false;
    elements += tag.length;
    if (elements > 4096) return false;
    for (const element of tag) {
      if (typeof element !== 'string' || !boundedUtf8(element, 4096))
        return false;
      bytes += encoder.encode(element).length;
      if (bytes > 131072) return false;
    }
  }
  return true;
}
export function verifyEnvelope(
  raw: unknown
):
  | Readonly<{ ok: true; value: VerifiedEnvelope }>
  | Readonly<{ ok: false; error: EnvelopeFailure }> {
  if (
    typeof raw !== 'string' ||
    !boundedUtf8(raw, PUBLIC_INGRESS_BUDGETS.eventBytes)
  )
    return { ok: false, error: invalid };
  try {
    // Parse a fresh bounded JSON object: caller getters and cached SDK symbols
    // never cross verification. Generic SDK helpers own hashing and crypto.
    const unsupportedNumbers = new WeakMap<object, boolean>();
    const event: unknown = JSON.parse(
      raw,
      function (
        this: object,
        key: string,
        value: unknown,
        context?: { source?: string }
      ) {
        if (
          (key === 'kind' || key === 'created_at') &&
          typeof value === 'number' &&
          (context?.source === undefined || !/^[0-9]+$/u.test(context.source))
        )
          unsupportedNumbers.set(this, true);
        if (
          !key.isWellFormed() ||
          (typeof value === 'string' && !value.isWellFormed())
        )
          throw new Error('Invalid Unicode in bounded envelope');
        if (typeof value === 'number' && !Number.isFinite(value))
          throw new Error('Nonfinite number in bounded envelope');
        return value;
      }
    );
    if (
      typeof event !== 'object' ||
      event === null ||
      Array.isArray(event) ||
      !('id' in event) ||
      !('pubkey' in event) ||
      !('sig' in event) ||
      !('kind' in event) ||
      !('created_at' in event) ||
      !('tags' in event) ||
      !('content' in event)
    )
      return { ok: false, error: invalid };
    if (
      typeof event.id !== 'string' ||
      !/^[0-9a-f]{64}$/u.test(event.id) ||
      typeof event.pubkey !== 'string' ||
      !/^[0-9a-f]{64}$/u.test(event.pubkey) ||
      typeof event.sig !== 'string' ||
      !/^[0-9a-f]{128}$/u.test(event.sig) ||
      !boundedEnvelopeNumbers(event.kind, event.created_at) ||
      !boundedEnvelopeTags(event.tags) ||
      typeof event.content !== 'string' ||
      !boundedUtf8(event.content, 131072)
    )
      return { ok: false, error: invalid };
    if (unsupportedNumbers.has(event)) return { ok: false, error: invalid };
    const fields = event as Record<string, unknown>;
    let extraFields = 0;
    let extraBytes = 0;
    const encoder = new TextEncoder();
    for (const name in fields) {
      if (
        [
          'id',
          'pubkey',
          'sig',
          'kind',
          'created_at',
          'tags',
          'content'
        ].includes(name)
      )
        continue;
      extraFields++;
      const encoded = JSON.stringify(fields[name]);
      if (!name.isWellFormed() || encoded === undefined)
        return { ok: false, error: invalid };
      extraBytes +=
        encoder.encode(JSON.stringify(name)).length +
        1 +
        encoder.encode(encoded).length;
      if (extraFields > 64 || extraBytes > 65536)
        return { ok: false, error: invalid };
    }
    const candidate = event as NostrEvent;
    if (getEventHash(candidate) !== candidate.id)
      return {
        ok: false,
        error: {
          code: 'event_id_mismatch',
          message: 'NIP-01 event id does not match canonical content'
        }
      };
    if (!verifyEvent(candidate))
      return {
        ok: false,
        error: {
          code: 'signature_invalid',
          message: 'invalid NIP-01 event signature'
        }
      };
    const token = Object.freeze({}) as VerifiedEnvelope;
    snapshots.set(token, raw);
    return { ok: true, value: token };
  } catch {
    return { ok: false, error: invalid };
  }
}
export function verifiedEnvelopeSnapshot(
  token: VerifiedEnvelope
): NostrEvent | undefined {
  const raw = snapshots.get(token);
  return raw === undefined ? undefined : (JSON.parse(raw) as NostrEvent);
}

// Genuine proof-owned input only. SDK object ingress records reconstructed
// decoded JSON; callers must not mislabel that as original transport bytes.
export function verifiedEnvelopeWire(
  token: VerifiedEnvelope
): string | undefined {
  return snapshots.get(token);
}
