import {
  messageMetadataSnapshot,
  messageMetadataMessages,
  type MessageMetadata
} from '../persistence/message-metadata.ts';
import type { ConversationData } from './admit-conversation.ts';
// Pure detached local display calculation. No authentication/effect permission;
// no signed timestamp or outer fetch receipt can imply a local read.
export function localUnreadRumors(
  messages: readonly ConversationData[],
  owner: string,
  read: readonly string[]
): readonly string[] {
  const seen = new Set(read);
  return Array.from(
    new Set(
      messages
        .filter(
          (x) =>
            x.owner === owner && x.role === 'inbound' && !seen.has(x.rumorId)
        )
        .map((x) => x.rumorId)
    )
  );
}
export async function readLocalUnread(scope: MessageMetadata) {
  const metadata = await messageMetadataSnapshot(scope);
  if (metadata.status !== 'ready') return { status: metadata.status };
  const view = messageMetadataMessages(scope);
  if (view.status !== 'ready') return { status: view.status };
  const ids = localUnreadRumors(
    view.messages,
    metadata.owner,
    metadata.readRumors
  );
  return { status: 'ready' as const, count: ids.length, rumorIds: ids };
}
