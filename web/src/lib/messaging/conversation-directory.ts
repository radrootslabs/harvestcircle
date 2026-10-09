import { canonicalLocalId } from '../private-handles.ts';
import {
  privateSessionOwnership,
  subscribePrivateSessionClose,
  type PrivateSession
} from '../runtime/private-session.ts';
import {
  messageMetadataOwnership,
  messageMetadataSnapshot,
  rememberMessageConversation,
  type MessageMetadata
} from '../persistence/message-metadata.ts';
import type { AdmittedConversation } from './admit-conversation.ts';
declare const directoryBrand: unique symbol;
export type ConversationDirectory = Readonly<{ [directoryBrand]: true }>;
export const CONVERSATION_DIRECTORY_CONTEXT =
  'harvestcircle_conversation_directory';
type Resolution =
  | Readonly<{ status: 'unavailable' }>
  | Readonly<{
      status: 'resolved';
      owner: string;
      peer: string;
      conversationId: string;
      href: string;
    }>;
type Remembered =
  | Awaited<ReturnType<typeof rememberMessageConversation>>
  | Readonly<{ status: 'unavailable' }>;
type Controller = {
  resolve(id: unknown): Promise<Resolution>;
  remember(room: AdmittedConversation, review: unknown): Promise<Remembered>;
  subscribe(listener: () => void): () => void;
  close(): void;
};
const directories = new WeakMap<ConversationDirectory, Controller>();
const unavailable = (): Resolution => ({ status: 'unavailable' });
// Pure syntax projection, never authorization. Owner/peer/product/body do not
// occur in the path; IDs from another browser may have no local mapping.
export function canonicalConversationHref(id: unknown): string | undefined {
  const valid = canonicalLocalId(id);
  return valid ? '/messages/' + valid : undefined;
}
export function captureConversationDirectory(
  session: PrivateSession,
  metadata: MessageMetadata,
  review: unknown
): ConversationDirectory | undefined {
  if (
    typeof window === 'undefined' ||
    review !== 'reviewed_conversation_directory'
  )
    return;
  const privateOwner = privateSessionOwnership(session),
    localOwner = messageMetadataOwnership(metadata);
  if (
    !privateOwner?.current() ||
    !localOwner?.current() ||
    privateOwner.owner !== localOwner.owner ||
    privateOwner.session !== localOwner.session
  )
    return;
  const original = privateOwner,
    local = localOwner;
  let closed = false,
    off = () => {};
  const listeners = new Map<symbol, () => void>();
  const token = Object.freeze({}) as ConversationDirectory;
  function close() {
    if (closed) return;
    closed = true;
    off();
    directories.set(token, {
      resolve: () => Promise.resolve(unavailable()),
      remember: () => Promise.resolve({ status: 'unavailable' }),
      subscribe: () => () => {},
      close: () => {}
    });
    for (const listener of listeners.values()) {
      try {
        listener();
      } catch {
        /* Presentation observer cannot retain private custody. */
      }
    }
    listeners.clear();
  }
  function current() {
    if (closed || !original.current() || !local.current()) {
      close();
      return false;
    }
    return true;
  }
  directories.set(token, {
    async resolve(id) {
      const canonical = canonicalLocalId(id),
        href = canonicalConversationHref(id);
      if (!canonical || !href || !current()) return unavailable();
      try {
        const snapshot = await messageMetadataSnapshot(metadata);
        if (
          !current() ||
          snapshot.status !== 'ready' ||
          snapshot.owner !== original.owner
        )
          return unavailable();
        const pair = snapshot.pairs.find((row) => row.id === canonical);
        return pair
          ? {
              status: 'resolved',
              owner: original.owner,
              peer: pair.peer,
              conversationId: canonical,
              href
            }
          : unavailable();
      } catch {
        return unavailable();
      }
    },
    async remember(room, reviewed) {
      if (!current()) return { status: 'unavailable' };
      const result = await rememberMessageConversation(
        metadata,
        room,
        reviewed
      );
      // A settled native write on an expired owner cannot disclose a room.
      return current() ? result : { status: 'unavailable' };
    },
    subscribe(listener) {
      if (!current()) return () => {};
      const key = Symbol();
      listeners.set(key, listener);
      return () => {
        listeners.delete(key);
      };
    },
    close
  });
  off = subscribePrivateSessionClose(session, close);
  return current() ? token : undefined;
}
// Read-only lookup of genuine owner metadata. Returned peer is a local
// projection, never cryptographic room/reply permission or navigation effects.
export function resolveLocalConversation(
  directory: ConversationDirectory,
  id: unknown
): Promise<Resolution> {
  return (
    directories.get(directory)?.resolve(id) ?? Promise.resolve(unavailable())
  );
}
// Explicit admitted cached exchange only; copied snapshots/unknown GET cannot
// create a mapping. Product compose admission is wired by its later slice.
export function rememberAdmittedConversation(
  directory: ConversationDirectory,
  room: AdmittedConversation,
  review: unknown
): Promise<Remembered> {
  return (
    directories.get(directory)?.remember(room, review) ??
    Promise.resolve({ status: 'unavailable' })
  );
}
export function subscribeConversationDirectory(
  directory: ConversationDirectory,
  listener: () => void
): () => void {
  return directories.get(directory)?.subscribe(listener) ?? (() => {});
}
export function closeConversationDirectory(
  directory: ConversationDirectory
): void {
  directories.get(directory)?.close();
}
