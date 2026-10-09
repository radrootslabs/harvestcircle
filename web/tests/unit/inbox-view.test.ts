import { describe, expect, it } from 'vitest';
import {
  createInboxView,
  inboxViewSnapshot,
  unlockInboxView,
  checkInboxView,
  readNewInboxView,
  inboxViewSetup,
  closeInboxView,
  type InboxView
} from '../../src/lib/messaging/inbox-view.ts';
import { createIdentitySession } from '../../src/lib/runtime/identity-session.ts';
import type { InboxSetupView } from '../../src/lib/messaging/inbox-setup-view.ts';
describe('inbox page requires original browser owners', () => {
  it('SSR capture cannot create effects from a copied setup', () => {
    expect(
      createInboxView({
        identity: createIdentitySession(),
        setup: {} as InboxSetupView
      })
    ).toBeUndefined();
  });
  it('forged scalar tokens never return an owner, checked time or empty inbox', () => {
    const value = inboxViewSnapshot({} as InboxView);
    expect(value.state).toBe('unavailable');
    expect(value.lastCheckedAt).toBeNull();
    expect(value.owner).toBeUndefined();
    expect(value.count).toBe(0);
  });
  it('copied observations are not an inbox controller', () => {
    const value = inboxViewSnapshot(undefined);
    expect(inboxViewSnapshot({ ...value } as unknown as InboxView).state).toBe(
      'unavailable'
    );
    expect(
      inboxViewSetup({ ...value } as unknown as InboxView)
    ).toBeUndefined();
  });
  it('a forged unlock cannot acquire extension or native database effects', async () => {
    expect(
      await unlockInboxView({} as InboxView, 'reviewed_messages_unlock')
    ).toBe(false);
  });
  it('a forged check cannot issue a relay request or checked timestamp', async () => {
    expect(
      await checkInboxView({} as InboxView, 'reviewed_foreground_inbox')
    ).toBe(false);
    expect(inboxViewSnapshot({} as InboxView).lastCheckedAt).toBeNull();
  });
  it('forged continuation cannot approve any decrypt batch', async () => {
    expect(
      await readNewInboxView({} as InboxView, 'reviewed_decrypt_batch')
    ).toBe(false);
  });
  it('copied or absent owners cannot expose the shared setup token', () => {
    expect(inboxViewSetup(undefined)).toBeUndefined();
    expect(inboxViewSetup({} as InboxView)).toBeUndefined();
  });
  it('closing an unknown owner is inert and cannot turn it into a ready inbox', () => {
    closeInboxView({} as InboxView);
    expect(inboxViewSnapshot({} as InboxView).state).toBe('unavailable');
  });
});
