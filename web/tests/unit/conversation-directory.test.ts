import { test, expect } from 'vitest';
import {
  canonicalConversationHref,
  captureConversationDirectory,
  resolveLocalConversation,
  rememberAdmittedConversation,
  type ConversationDirectory
} from '../../src/lib/messaging/conversation-directory.ts';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { MessageMetadata } from '../../src/lib/persistence/message-metadata.ts';
import type { AdmittedConversation } from '../../src/lib/messaging/admit-conversation.ts';
const id = '12345678-1234-4234-8234-123456789abc';
test('only the canonical random UUID occurs in the private URL', () =>
  expect(canonicalConversationHref(id)).toBe('/messages/' + id));
test('uppercase UUID is not normalized into a private lookup', () =>
  expect(canonicalConversationHref(id.toUpperCase())).toBeUndefined());
test('a nonrandom UUID does not become a conversation handle', () =>
  expect(
    canonicalConversationHref(id.replace('-4234-', '-1234-'))
  ).toBeUndefined());
test('peer keys and product coordinates never form conversation paths', () => {
  for (const value of ['a'.repeat(64), 'naddr1product', '/messages/' + id])
    expect(canonicalConversationHref(value)).toBeUndefined();
});
test('query fragments and malformed tails are rejected in their entirety', () => {
  for (const value of [
    id + '?peer=' + 'b'.repeat(64),
    id + '#context',
    id + '/',
    null,
    {}
  ])
    expect(canonicalConversationHref(value)).toBeUndefined();
});
test('SSR and scalar/copy ownership cannot construct a private directory', () => {
  expect(
    captureConversationDirectory(
      {} as PrivateSession,
      {} as MessageMetadata,
      'reviewed_conversation_directory'
    )
  ).toBeUndefined();
});
test('copied directory lookup is safely unavailable without inferred metadata', async () =>
  expect(
    await resolveLocalConversation({} as ConversationDirectory, id)
  ).toEqual({ status: 'unavailable' }));
test('unknown token and copied room cannot create a substitute conversation', async () =>
  expect(
    await rememberAdmittedConversation(
      {} as ConversationDirectory,
      {} as AdmittedConversation,
      'reviewed_admitted_conversation_navigation'
    )
  ).toEqual({ status: 'unavailable' }));
