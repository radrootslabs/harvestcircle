import { describe, expect, it } from 'vitest';
import {
  createReplyController,
  replySnapshot,
  replyText,
  updateReplyText,
  sendReply,
  requestReplyNavigation,
  keepReplyEditing,
  discardReplyNavigation,
  closeReplyController,
  type ReplyController
} from '../../src/lib/messaging/reply-controller.ts';
import { captureAuthenticatedReplyRumor } from '../../src/lib/messaging/rumor-plan.ts';
import type { InboxView } from '../../src/lib/messaging/inbox-view.ts';
import type { IdentitySession } from '../../src/lib/runtime/identity-session.ts';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { UnlockedSession } from '../../src/lib/messaging/unlocked-session.ts';
import type { AdmittedConversation } from '../../src/lib/messaging/admit-conversation.ts';

const id = '12345678-1234-4234-8234-123456789abc';
describe('reply entry points reject detached custody and SSR effects', () => {
  it('SSR cannot allocate an effect-capable reply controller or run discovery', () => {
    let lookups = 0;
    expect(
      createReplyController({} as InboxView, id, 'a'.repeat(64), {
        lookupPeer: () => {
          lookups++;
          throw Error('must not query');
        }
      })
    ).toBeUndefined();
    expect(lookups).toBe(0);
  });
  it('plain copied reply tokens expose no private body or lifecycle state', () => {
    const fake = Object.freeze({}) as ReplyController;
    expect(replySnapshot(fake)).toBeUndefined();
    expect(replyText(fake)).toBeUndefined();
    expect(
      updateReplyText(fake, 'Private sentinel', 'reviewed_private_text')
    ).toBe(false);
    closeReplyController(fake);
    expect(replyText(fake)).toBeUndefined();
  });
  it('fake Send cannot manufacture native reservation or remote success', async () => {
    expect(
      await sendReply({} as ReplyController, 'reviewed_reply_send')
    ).toEqual({ status: 'invalid' });
    expect(await sendReply({} as ReplyController, undefined)).toEqual({
      status: 'invalid'
    });
  });
  it('fake dirty-navigation tokens cannot discard a genuine draft', () => {
    const fake = {} as ReplyController;
    expect(requestReplyNavigation(fake)).toBe('invalid');
    expect(keepReplyEditing(fake)).toBe(false);
    expect(discardReplyNavigation(fake, 'reviewed_discard_private_text')).toBe(
      'invalid'
    );
  });
  it('same-owner strings and copied rooms cannot issue an authenticated reply rumor', () => {
    expect(
      captureAuthenticatedReplyRumor(
        {} as IdentitySession,
        {} as PrivateSession,
        {} as UnlockedSession,
        {
          owner: 'a'.repeat(64),
          peer: 'b'.repeat(64),
          rumorId: 'c'.repeat(64)
        } as unknown as AdmittedConversation,
        'Reply must not redirect to a body contact',
        1700000000
      )
    ).toBeUndefined();
  });
});
