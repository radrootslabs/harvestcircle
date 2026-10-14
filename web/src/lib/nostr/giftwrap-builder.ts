import {
  generateSecretKey,
  getPublicKey,
  finalizeEvent,
  getEventHash,
  type UnsignedEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { safeUnsignedInteger } from './envelope-bounds.ts';
import { verifyEnvelope } from './verified-envelope.ts';
import {
  privateSealSnapshot,
  inboxProbeSealSnapshot,
  type InboxProbeRecipientSeal,
  type PrivateRecipientSeal
} from './seal-builder.ts';

declare const probeWrapBrand: unique symbol;
export type InboxProbeGiftwrap = Readonly<{ [probeWrapBrand]: true }>;
declare const giftwrapBrand: unique symbol;
export type PrivateGiftwrap = Readonly<{ [giftwrapBrand]: true }>;
type Saved = Readonly<{
  owner: string;
  peer: string;
  destination: string;
  role: 'peer' | 'self';
  command: string;
  rumorHash: string;
  kind: 1059;
  wire: string;
  seal: PrivateRecipientSeal;
}>;
const wraps = new WeakMap<PrivateGiftwrap, Saved>();
function own(value: object, name: string): unknown {
  const descriptor = Object.getOwnPropertyDescriptor(value, name);
  return descriptor && 'value' in descriptor ? descriptor.value : undefined;
}
type SavedProbeWrap = Readonly<{
  owner: string;
  destination: string;
  command: string;
  rumorHash: string;
  createdAt: number;
  kind: 1059;
  wire: string;
  seal: InboxProbeRecipientSeal;
}>;
const probeWraps = new WeakMap<InboxProbeGiftwrap, SavedProbeWrap>();
const originalProbeWraps = new WeakMap<
  InboxProbeRecipientSeal,
  InboxProbeGiftwrap | null
>();
// Module-private stock SDK wrapper construction; only genuine ordinary/probe
// seal factories below can supply the binding. No exported callback/key port.
function constructGiftwrap(
  record: Readonly<{ destination: string; wire: string }>,
  current: () => boolean
): string | undefined {
  let secret: Uint8Array | undefined, conversation: Uint8Array | undefined;
  try {
    const milliseconds = safeUnsignedInteger(Date.now());
    if (milliseconds === undefined || !current()) return undefined;
    const random = crypto.getRandomValues(new Uint32Array(1))[0],
      time = Math.max(0, Math.floor(milliseconds / 1000) - (random % 3600));
    if (!current()) return undefined;
    secret = generateSecretKey();
    if (!current()) return undefined;
    const author = getPublicKey(secret);
    conversation = nip44.v2.utils.getConversationKey(
      secret,
      record.destination
    );
    if (!current()) return undefined;
    const content = nip44.v2.encrypt(record.wire, conversation);
    if (!current()) return undefined;
    if (nip44.v2.decrypt(content, conversation) !== record.wire || !current())
      return undefined;
    const captured = JSON.stringify({
      pubkey: author,
      kind: 1059,
      created_at: time,
      tags: [['p', record.destination]],
      content
    });
    const response = finalizeEvent(
        JSON.parse(captured) as UnsignedEvent,
        secret
      ),
      id = own(response, 'id'),
      sig = own(response, 'sig'),
      tags = own(response, 'tags');
    if (
      own(response, 'pubkey') !== author ||
      own(response, 'kind') !== 1059 ||
      own(response, 'created_at') !== time ||
      own(response, 'content') !== content ||
      !Array.isArray(tags) ||
      own(tags, 'length') !== 1
    )
      return undefined;
    const target = own(tags, '0');
    if (
      !Array.isArray(target) ||
      own(target, 'length') !== 2 ||
      own(target, '0') !== 'p' ||
      own(target, '1') !== record.destination ||
      typeof id !== 'string' ||
      typeof sig !== 'string' ||
      id !== getEventHash(JSON.parse(captured) as UnsignedEvent)
    )
      return undefined;
    // Explicit fields discard SDK verification/cache symbols and extra data.
    // Parse and verify a fresh wire, independently of the SDK result object.
    const wire = JSON.stringify({
      pubkey: author,
      kind: 1059,
      created_at: time,
      tags: [['p', record.destination]],
      content,
      id,
      sig
    });
    if (!boundedUtf8(wire, 32768) || !verifyEnvelope(wire).ok || !current())
      return undefined;
    return wire;
  } catch {
    return undefined;
  } finally {
    // Directly owned byte buffers only. The SDK/engine may allocate other
    // buffers; this does not promise complete browser-memory zeroization.
    conversation?.fill(0);
    secret?.fill(0);
  }
}
// Genuine ordinary sender seals only; probe casts grant no ordinary token.
export function buildPrivateGiftwrap(
  seal: PrivateRecipientSeal,
  review: unknown
): PrivateGiftwrap | undefined {
  if (typeof window === 'undefined' || review !== 'reviewed_private_wrap')
    return undefined;
  const record = privateSealSnapshot(seal);
  if (
    !record ||
    !boundedUtf8(record.wire, 16384) ||
    record.destination !== (record.role === 'self' ? record.owner : record.peer)
  )
    return undefined;
  const wire = constructGiftwrap(record, () => !!privateSealSnapshot(seal));
  if (!wire || !privateSealSnapshot(seal)) return undefined;
  const token = Object.freeze({}) as PrivateGiftwrap;
  wraps.set(token, { ...record, kind: 1059, wire, seal });
  return token;
}
// One self-only immutable crypto artifact per genuine diagnostic seal. Failed
// construction cannot silently generate a replacement artifact in this action.
export function buildInboxProbeGiftwrap(
  seal: InboxProbeRecipientSeal,
  review: unknown
): InboxProbeGiftwrap | undefined {
  if (typeof window === 'undefined' || review !== 'reviewed_inbox_probe_wrap')
    return undefined;
  const record = inboxProbeSealSnapshot(seal);
  if (
    !record ||
    record.destination !== record.owner ||
    !boundedUtf8(record.wire, 16384)
  )
    return undefined;
  if (originalProbeWraps.has(seal))
    return originalProbeWraps.get(seal) ?? undefined;
  originalProbeWraps.set(seal, null);
  const wire = constructGiftwrap(record, () => !!inboxProbeSealSnapshot(seal));
  if (!wire || !inboxProbeSealSnapshot(seal)) return undefined;
  const token = Object.freeze({}) as InboxProbeGiftwrap;
  probeWraps.set(token, { ...record, kind: 1059, wire, seal });
  originalProbeWraps.set(seal, token);
  return token;
}
export function inboxProbeGiftwrapSeal(
  token: InboxProbeGiftwrap
): InboxProbeRecipientSeal | undefined {
  const saved = probeWraps.get(token);
  return saved && inboxProbeSealSnapshot(saved.seal) ? saved.seal : undefined;
}
export function inboxProbeGiftwrapSnapshot(
  token: InboxProbeGiftwrap
): Readonly<Omit<SavedProbeWrap, 'seal'>> | undefined {
  const saved = probeWraps.get(token);
  return saved && inboxProbeSealSnapshot(saved.seal)
    ? {
        owner: saved.owner,
        destination: saved.destination,
        command: saved.command,
        rumorHash: saved.rumorHash,
        createdAt: saved.createdAt,
        kind: 1059,
        wire: saved.wire
      }
    : undefined;
}
// Returns only the genuine original capability, never disposable key material.
export function privateGiftwrapSeal(
  token: PrivateGiftwrap
): PrivateRecipientSeal | undefined {
  const saved = wraps.get(token);
  return saved && privateSealSnapshot(saved.seal) ? saved.seal : undefined;
}
export function privateGiftwrapSnapshot(
  token: PrivateGiftwrap
): Readonly<Omit<Saved, 'seal'>> | undefined {
  const saved = wraps.get(token);
  return saved && privateSealSnapshot(saved.seal)
    ? {
        owner: saved.owner,
        peer: saved.peer,
        destination: saved.destination,
        role: saved.role,
        command: saved.command,
        rumorHash: saved.rumorHash,
        kind: 1059,
        wire: saved.wire
      }
    : undefined;
}
