import { canonicalPublicKey } from '../contracts/public-key.ts';
import { boundedUtf8 } from '../contracts/food-availability-v1/text.ts';
import { LOCAL_PERSISTENCE_BUDGETS } from '../config/budgets.ts';
import { localUnreadRumors } from './unread.ts';
import type { ConversationData } from './admit-conversation.ts';
export type InboxListRow = Readonly<{
  peer: string;
  count: number;
  unread: number;
  latest: ConversationData;
}>;
export type InboxListSnapshot = Readonly<{
  status: 'ready' | 'locked' | 'unavailable';
  requests: readonly InboxListRow[];
  conversations: readonly InboxListRow[];
}>;
const unavailable = (): InboxListSnapshot => ({
  status: 'unavailable',
  requests: [],
  conversations: []
});
function arrayValue(value: unknown): boolean {
  return Array.isArray(value);
}
// Pure detached presentation. These values never authenticate a sender or grant
// storage, navigation, decryption, query or reply permission.
export function groupInboxMessages(
  owner: unknown,
  messages: readonly ConversationData[],
  read: readonly string[]
): InboxListSnapshot {
  if (
    !canonicalPublicKey(owner) ||
    !arrayValue(messages) ||
    !arrayValue(read) ||
    messages.length > LOCAL_PERSISTENCE_BUDGETS.receivedEnvelopes
  )
    return unavailable();
  const rows = new Map<string, ConversationData>(),
    wires = new Map<string, string>();
  for (const row of messages) {
    if (
      !row ||
      row.owner !== owner ||
      !canonicalPublicKey(row.peer) ||
      row.peer === owner ||
      !['inbound', 'self_archive'].includes(row.role) ||
      row.sender !== (row.role === 'inbound' ? row.peer : owner) ||
      row.recipient !== (row.role === 'inbound' ? owner : row.peer) ||
      typeof row.rumorId !== 'string' ||
      !/^[0-9a-f]{64}$/.test(row.rumorId) ||
      !Number.isSafeInteger(row.createdAt) ||
      row.createdAt < 0 ||
      typeof row.content !== 'string' ||
      !boundedUtf8(row.content, 4096) ||
      (row.subject !== null && typeof row.subject !== 'string') ||
      (row.replyTo !== null &&
        (typeof row.replyTo !== 'string' ||
          !/^[0-9a-f]{64}$/.test(row.replyTo)))
    )
      return unavailable();
    const wire = JSON.stringify([
        row.owner,
        row.peer,
        row.sender,
        row.recipient,
        row.role,
        row.rumorId,
        row.createdAt,
        row.content,
        row.subject,
        row.replyTo
      ]),
      old = wires.get(row.rumorId);
    if (old !== undefined && old !== wire) return unavailable();
    wires.set(row.rumorId, wire);
    rows.set(row.rumorId, { ...row });
  }
  const grouped = new Map<string, ConversationData[]>();
  for (const row of rows.values())
    grouped.set(row.peer, (grouped.get(row.peer) ?? []).concat(row));
  let requests = Array.from<InboxListRow>([]),
    conversations = Array.from<InboxListRow>([]);
  for (const [peer, group] of grouped) {
    const ordered = group
        .slice()
        .sort(
          (a, b) =>
            b.createdAt - a.createdAt || b.rumorId.localeCompare(a.rumorId)
        ),
      row = {
        peer,
        count: group.length,
        unread: localUnreadRumors(group, owner as string, read).length,
        latest: { ...ordered[0] }
      };
    if (group.some((value) => value.role === 'self_archive'))
      conversations = conversations.concat(row);
    else requests = requests.concat(row);
  }
  const order = (a: InboxListRow, b: InboxListRow) =>
    b.latest.createdAt - a.latest.createdAt ||
    b.latest.rumorId.localeCompare(a.latest.rumorId) ||
    a.peer.localeCompare(b.peer);
  return {
    status: 'ready',
    requests: requests.sort(order),
    conversations: conversations.sort(order)
  };
}
