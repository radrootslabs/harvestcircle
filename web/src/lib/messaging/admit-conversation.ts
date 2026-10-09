import { buildCanonicalPairRumor } from '../nostr/rumor-template.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import { exactLocalFields } from '../contracts/local-records.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import {
  messageFromWireParts,
  messageToWireParts
} from '../contracts/message-v1/index.ts';
import { safeUnsignedInteger } from '../nostr/envelope-bounds.ts';
import {
  receivedNestedEnvelopeSnapshot,
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
    typeof raw !== 'string' ||
    !boundedUtf8(raw, 8192)
  )
    return undefined;
  try {
    const rumor: unknown = JSON.parse(raw);
    if (
      !exactLocalFields(rumor, [
        'id',
        'pubkey',
        'created_at',
        'kind',
        'tags',
        'content'
      ]) ||
      rumor.kind !== 14 ||
      typeof rumor.id !== 'string' ||
      !/^[0-9a-f]{64}$/u.test(rumor.id) ||
      safeUnsignedInteger(rumor.created_at) === undefined
    )
      return undefined;
    const sender = canonicalPublicKey(rumor.pubkey);
    if (!sender) return undefined;
    const parts = { kind: 14, tags: rumor.tags, content: rumor.content };
    const decoded = messageFromWireParts(JSON.stringify(parts));
    if (!decoded || decoded.recipients.length !== 1) return undefined;
    const recipient = decoded.recipients[0].public_key;
    if (sender === recipient || (owner !== sender && owner !== recipient))
      return undefined;
    const canonical = messageToWireParts(JSON.stringify(decoded));
    if (!canonical || JSON.stringify(canonical) !== JSON.stringify(parts))
      return undefined;
    const template = buildCanonicalPairRumor(
      JSON.stringify(decoded),
      sender,
      rumor.created_at
    );
    if (!template || template.id !== rumor.id) return undefined;
    const role = sender === owner ? 'self_archive' : 'inbound';
    if (role !== expectedRole) return undefined;
    return {
      owner,
      peer: sender === owner ? recipient : sender,
      sender,
      recipient,
      role,
      rumorId: rumor.id,
      createdAt: template.created_at,
      content: canonical.content,
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
