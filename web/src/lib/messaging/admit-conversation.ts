import { canonicalPublicKey } from '../contracts/public-key.ts';
import { readIncomingPairRumor } from '../nostr/incoming-wire.ts';
import {
  receivedNestedEnvelopeSnapshot,
  receivedNestedEnvelopeOwnership,
  type ReceivedNestedEnvelope
} from '../nostr/unwrap-admission.ts';

export type ConversationRole = 'inbound' | 'self_archive';
export type ConversationData = Readonly<{
  owner: string;
  peer: string;
  sender: string;
  recipient: string;
  role: ConversationRole;
  rumorId: string;
  createdAt: number;
  content: string;
  subject: string | null;
  replyTo: string | null;
}>;
// Detached inspection grants no sender authentication or effect permission.
// Only the actual SDK nested-custody issuer below admits a current room.
export function inspectConversationData(
  raw: unknown,
  expectedOwner: unknown,
  expectedRole: unknown
): ConversationData | undefined {
  const owner = canonicalPublicKey(expectedOwner);
  if (
    !owner ||
    (expectedRole !== 'inbound' && expectedRole !== 'self_archive') ||
    typeof raw !== 'string'
  )
    return undefined;
  try {
    const incoming = readIncomingPairRumor(raw);
    if (!incoming) return undefined;
    const rumor = incoming.original,
      decoded = incoming.message,
      sender = rumor.pubkey;
    const recipient = decoded.recipients[0].public_key;
    if (sender === recipient || (owner !== sender && owner !== recipient))
      return undefined;
    const role = sender === owner ? 'self_archive' : 'inbound';
    if (role !== expectedRole) return undefined;
    return {
      owner,
      peer: sender === owner ? recipient : sender,
      sender,
      recipient,
      role,
      rumorId: rumor.id,
      createdAt: rumor.created_at,
      content: decoded.content,
      subject: decoded.subject,
      // Private parent hash only; no hint, URL or public lookup is authorized.
      replyTo: decoded.reply_to?.id ?? null
    };
  } catch {
    return undefined;
  }
}

declare const conversationBrand: unique symbol;
export type AdmittedConversation = Readonly<{ [conversationBrand]: true }>;
const rooms = new WeakMap<
  AdmittedConversation,
  { nested: ReceivedNestedEnvelope; role: ConversationRole }
>();
// Outer disposable pubkey, body contacts and relay hints are never participants.
// The original inner recipient remains unchanged for self archives. No cache,
// persistent projection, send/readiness authority or reply action is minted.
export function admitReceivedConversation(
  envelope: ReceivedNestedEnvelope,
  expectedRole: unknown
): AdmittedConversation | undefined {
  const nested = receivedNestedEnvelopeSnapshot(envelope);
  if (!nested) return undefined;
  const data = inspectConversationData(
    nested.rumorWire,
    nested.owner,
    expectedRole
  );
  if (!data || !receivedNestedEnvelopeSnapshot(envelope)) return undefined;
  const token = Object.freeze({}) as AdmittedConversation;
  rooms.set(token, { nested: envelope, role: data.role });
  return token;
}
export function conversationSnapshot(
  token: AdmittedConversation
): ConversationData | undefined {
  const saved = rooms.get(token);
  const nested = saved && receivedNestedEnvelopeSnapshot(saved.nested);
  return nested && saved
    ? inspectConversationData(nested.rumorWire, nested.owner, saved.role)
    : undefined;
}

// Original immutable identity generation is observed only from the actual
// admitted nested proof; same-owner strings do not import another session.
export function conversationOwnership(token: AdmittedConversation) {
  const saved = rooms.get(token);
  return saved && receivedNestedEnvelopeOwnership(saved.nested);
}

// HCP094 ciphertext identity observation from the original authenticated room.
// Detached strings never replace genuine room custody or grant persistence.
export function conversationEnvelopeIdentity(token: AdmittedConversation) {
  const saved = rooms.get(token),
    nested = saved && receivedNestedEnvelopeSnapshot(saved.nested);
  return nested && conversationSnapshot(token)
    ? {
        owner: nested.owner,
        outerId: nested.outerId,
        outerWire: nested.outerWire
      }
    : undefined;
}
