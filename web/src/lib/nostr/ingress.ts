import { PUBLIC_INGRESS_BUDGETS } from '../config/budgets.ts';
import { boundedEnvelopeNumbers } from './envelope-bounds.ts';
import { decodedInboxWire } from './decoded-inbox-wire.ts';
import {
  boundedEnvelopeTags,
  verifyEnvelope,
  verifiedEnvelopeSnapshot,
  type VerifiedEnvelope
} from './verified-envelope.ts';

declare const ingressOwner: unique symbol;
export type PublicIngress = Readonly<{ readonly [ingressOwner]: true }>;
export type IngressResult =
  | Readonly<{ status: 'accepted' | 'duplicate'; value: VerifiedEnvelope }>
  | Readonly<{ status: 'rejected' | 'limit' }>;
export type IngressStats = Readonly<{
  deliveries: number;
  chargedBytes: number;
  stopped: boolean;
}>;
interface Owner {
  readonly admit: (input: unknown, inbox?: boolean) => IngressResult;
  readonly stats: () => IngressStats;
}
const owners = new WeakMap<PublicIngress, Owner>();
const encoder = new TextEncoder();

// Read only own data properties. SDK cache symbols, getters, inherited fields,
// toJSON and iterator overrides cannot cross into a verified snapshot.
function ownData(
  input: object,
  name: string,
  assertActive: () => void
): unknown {
  assertActive();
  const descriptor = Object.getOwnPropertyDescriptor(input, name);
  assertActive();
  if (!descriptor || !('value' in descriptor))
    throw new Error('candidate_invalid');
  return descriptor.value;
}
function freshTags(
  input: unknown,
  assertActive: () => void
): string[][] | undefined {
  if (!Array.isArray(input)) return undefined;
  const length = ownData(input, 'length', assertActive);
  if (
    typeof length !== 'number' ||
    !Number.isSafeInteger(length) ||
    length < 0 ||
    length > 1024
  )
    return undefined;
  let elements = 0;
  const tags = Array.from({ length }, (_, index) => {
    const row = ownData(input, String(index), assertActive);
    if (!Array.isArray(row)) throw new Error('candidate_invalid');
    const width = ownData(row, 'length', assertActive);
    if (typeof width !== 'number' || !Number.isSafeInteger(width) || width < 1)
      throw new Error('candidate_invalid');
    elements += width;
    if (elements > 4096) throw new Error('candidate_invalid');
    return Array.from({ length: width }, (_, column) => {
      const element = ownData(row, String(column), assertActive);
      if (typeof element !== 'string' || element.length > 4096)
        throw new Error('candidate_invalid');
      return element;
    });
  });
  return boundedEnvelopeTags(tags) ? tags : undefined;
}
function reconstructedWire(
  input: unknown,
  assertActive: () => void
): string | undefined {
  try {
    assertActive();
    if (!input || typeof input !== 'object' || Array.isArray(input))
      return undefined;
    const id = ownData(input, 'id', assertActive);
    const pubkey = ownData(input, 'pubkey', assertActive);
    const sig = ownData(input, 'sig', assertActive);
    const kind = ownData(input, 'kind', assertActive);
    const createdAt = ownData(input, 'created_at', assertActive);
    const content = ownData(input, 'content', assertActive);
    if (
      typeof id !== 'string' ||
      !/^[0-9a-f]{64}$/u.test(id) ||
      typeof pubkey !== 'string' ||
      !/^[0-9a-f]{64}$/u.test(pubkey) ||
      typeof sig !== 'string' ||
      !/^[0-9a-f]{128}$/u.test(sig) ||
      !boundedEnvelopeNumbers(kind, createdAt) ||
      typeof content !== 'string' ||
      content.length > 131072
    )
      return undefined;
    const tags = freshTags(ownData(input, 'tags', assertActive), assertActive);
    if (!tags) return undefined;
    return JSON.stringify({
      id,
      pubkey,
      sig,
      kind,
      created_at: createdAt,
      tags,
      content
    });
  } catch {
    return undefined;
  }
}

// One owner is shared by a public run's primary and auxiliary candidates.
// These are post-parse logical application bounds. The selected SDK exposes
// parsed events, not a qualified raw-frame hook or preallocation guarantee.
export function createPublicIngress(): PublicIngress {
  const token = Object.freeze({}) as PublicIngress;
  const seen = new Map<string, true>();
  let deliveries = 0;
  let chargedBytes = 0;
  let stopped = false;
  let busy = false;
  const assertActive = () => {
    if (stopped) throw new Error('candidate_invalid');
  };
  const charge = (bytes: number): boolean => {
    if (bytes > PUBLIC_INGRESS_BUDGETS.processedBytes - chargedBytes) {
      chargedBytes = PUBLIC_INGRESS_BUDGETS.processedBytes;
      stopped = true;
      return false;
    }
    chargedBytes += bytes;
    if (chargedBytes === PUBLIC_INGRESS_BUDGETS.processedBytes) stopped = true;
    return true;
  };
  owners.set(token, {
    stats: () =>
      Object.freeze({
        deliveries: deliveries,
        chargedBytes: chargedBytes,
        stopped: stopped
      }),
    admit(input, inbox = false) {
      if (stopped || deliveries >= PUBLIC_INGRESS_BUDGETS.deliveries) {
        stopped = true;
        return { status: 'limit' };
      }
      deliveries++;
      // Reserve entry before descriptor traps can reenter this owner. Refuse
      // nested admission without reflecting again; retain bounded charges.
      if (busy) {
        charge(PUBLIC_INGRESS_BUDGETS.eventBytes + 1);
        stopped = true;
        return { status: 'limit' };
      }
      busy = true;
      try {
        const rawAvailable = typeof input === 'string';
        const wire = rawAvailable
          ? input
          : inbox
            ? decodedInboxWire(input, assertActive)
            : reconstructedWire(input, assertActive);
        if (stopped) {
          charge(PUBLIC_INGRESS_BUDGETS.eventBytes + 1);
          return { status: 'limit' };
        }
        // Oversized raw input and unmeasurable rejected objects reserve the
        // event ceiling plus one. Decoded fields charge actual reconstructed
        // UTF8 JSON including expansion; original SDK frame bytes are unknown.
        const bytes =
          wire === undefined ||
          (rawAvailable && wire.length > PUBLIC_INGRESS_BUDGETS.eventBytes)
            ? PUBLIC_INGRESS_BUDGETS.eventBytes + 1
            : encoder.encode(wire).length;
        if (!charge(bytes)) return { status: 'limit' };
        if (deliveries === PUBLIC_INGRESS_BUDGETS.deliveries) stopped = true;
        // The inclusive last reserved candidate may finish. Subsequent entry
        // is already blocked by either cap; reentrant interruption aborts above.
        if (wire === undefined || bytes > PUBLIC_INGRESS_BUDGETS.eventBytes)
          return { status: 'rejected' };
        const result = verifyEnvelope(wire);
        if (!result.ok) return { status: 'rejected' };
        const event = verifiedEnvelopeSnapshot(result.value);
        if (!event) return { status: 'rejected' };
        // Dedup does not erase freshly verified per-source observation proof.
        if (seen.has(event.id))
          return { status: 'duplicate', value: result.value };
        seen.set(event.id, true);
        return { status: 'accepted', value: result.value };
      } finally {
        busy = false;
      }
    }
  });
  return token;
}
function ownerOf(token: PublicIngress): Owner {
  const owner = owners.get(token);
  if (!owner) throw new Error('public_ingress_invalid');
  return owner;
}
export function admitPublicEvent(
  token: PublicIngress,
  input: unknown
): IngressResult {
  return ownerOf(token).admit(input);
}
export function publicIngressStats(token: PublicIngress): IngressStats {
  return ownerOf(token).stats();
}

export function admitInboxEvent(
  token: PublicIngress,
  input: unknown
): IngressResult {
  return ownerOf(token).admit(input, true);
}
