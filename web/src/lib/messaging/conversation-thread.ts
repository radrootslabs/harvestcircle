import { groupInboxMessages } from './inbox-list.ts';
import type { ConversationData } from './admit-conversation.ts';
import { canonicalPublicKey } from '../contracts/public-key.ts';
import {
  decodeProductReference,
  type ProductReference
} from '../nostr/references.ts';
import {
  privateRecordIdentity,
  privateRecordSnapshot,
  type PrivateRecordHandle
} from '../persistence/private-records.ts';
import { privateSendStatus } from './send-status.ts';
export type ThreadMessage = Readonly<{
  data: ConversationData;
  citations: readonly ProductReference[];
  parent: 'present' | 'missing' | null;
  delivery: readonly string[] | null;
}>;
export type ThreadSnapshot = Readonly<{
  status: 'ready' | 'unavailable';
  messages: readonly ThreadMessage[];
}>;
export function unavailableThread(): ThreadSnapshot {
  return { status: 'unavailable', messages: [] };
}
// Detached presentation only; authentication and display custody stay with the
// current original InboxView room proofs, never these copied data or receipts.
export function projectConversationThread(
  owner: unknown,
  peer: unknown,
  messages: readonly ConversationData[],
  records: readonly PrivateRecordHandle[]
): ThreadSnapshot {
  if (
    !canonicalPublicKey(peer) ||
    peer === owner ||
    groupInboxMessages(owner, messages, []).status !== 'ready' ||
    messages.some((row) => row.peer !== peer)
  )
    return unavailableThread();
  const rows = new Map<string, ConversationData>();
  for (const row of messages) rows.set(row.rumorId, { ...row });
  const receipts = new Map<string, readonly string[]>();
  for (const token of records) {
    const identity = privateRecordIdentity(token),
      record = identity && privateRecordSnapshot(token, owner, identity.id);
    if (record?.family !== 'private_send_operation' || record.peer !== peer)
      continue;
    const status = privateSendStatus(token, owner, record.id);
    if (status) receipts.set(record.rumorHash, status.labels.slice());
  }
  return {
    status: 'ready',
    messages: Array.from(rows.values())
      .sort(
        (a, b) =>
          a.createdAt - b.createdAt || a.rumorId.localeCompare(b.rumorId)
      )
      .map((data) => {
        const citations = new Map<string, ProductReference>();
        for (const match of data.content.matchAll(
          /\bnostr:(naddr[0-9a-zA-Z]+)/g
        )) {
          const reference = decodeProductReference(match[1]);
          if (reference) citations.set(reference.naddr, reference);
        }
        return {
          data,
          citations: Array.from(citations.values()),
          parent:
            data.replyTo === null
              ? null
              : rows.has(data.replyTo)
                ? 'present'
                : 'missing',
          delivery:
            data.role === 'inbound'
              ? null
              : (receipts.get(data.rumorId) ?? ['Recipient delivery unknown'])
        };
      })
  };
}
