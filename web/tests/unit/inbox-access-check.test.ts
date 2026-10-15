import { afterEach, expect, it, vi } from 'vitest';
afterEach(() => vi.unstubAllGlobals());
for (const browser of [false, true]) {
  it(`cold ${browser ? 'browser-shaped' : 'SSR'} check and review forgeries have no effects`, async () => {
    vi.resetModules();
    let effects = 0;
    const trap = () => {
      effects++;
      throw Error('unadmitted check effect');
    };
    if (browser) {
      vi.stubGlobal('window', {
        get nostr() {
          return trap();
        }
      });
      vi.stubGlobal('navigator', {
        get locks() {
          return trap();
        }
      });
      vi.stubGlobal('indexedDB', { open: trap });
      vi.stubGlobal('WebSocket', trap);
    }
    const api = await import('../../src/lib/messaging/inbox-access-check.ts');
    const access =
      await import('../../src/lib/messaging/inbox-access-controller.ts');
    type Args = Parameters<typeof api.createInboxAccessCheck>;
    const args = [{}, {}, {}, {}, {}, {}] as unknown as Args;
    expect(api.createInboxAccessCheck(...args)).toBeUndefined();
    for (const value of [
      undefined,
      null,
      false,
      0,
      '',
      Symbol(),
      {},
      {
        current: () => true,
        state: 'qualified',
        sourceOnly: false,
        mode: 'original',
        disclosures: ['reviewed'],
        command: 'a'.repeat(64)
      }
    ]) {
      const token = value as Parameters<typeof api.inboxAccessCheckSnapshot>[0];
      const review = value as Parameters<
        typeof api.inboxAccessCheckReviewSnapshot
      >[0];
      expect(api.inboxAccessCheckSnapshot(token)).toBeUndefined();
      expect(api.inboxAccessCheckQualification(token)).toBeUndefined();
      expect(api.inboxAccessCheckReviewSnapshot(review)).toBeUndefined();
      expect(
        api.reviewInboxAccessCheck(token, 'original', 'wss://inbox.example.org')
      ).toBeUndefined();
      expect(api.reviewInboxAccessCheckCleanup(token)).toBeUndefined();
      expect(api.reviewInboxAccessCheckReconciliation(token)).toBeUndefined();
      expect(
        (await api.runInboxAccessCheck(review, 'reviewed_inbox_access_check'))
          .status
      ).toBe('invalid');
      expect(
        (
          await api.cleanupInboxAccessCheck(
            review,
            'reviewed_delete_local_inbox_probe'
          )
        ).status
      ).toBe('invalid');
      expect(api.stopInboxAccessCheck(token)).toBe(false);
      expect(
        (
          await api.reconcileInboxAccessCheck(
            review,
            'reviewed_inbox_access_check_reconciliation'
          )
        ).status
      ).toBe('invalid');
      expect(
        api.retryInboxAccessCheckCleanup(
          token,
          'reviewed_inbox_access_check_cleanup_retry'
        )
      ).toBe(false);
      expect(
        access.inboxAccessControllerSourceSnapshot(
          value as Parameters<
            typeof access.inboxAccessControllerSourceSnapshot
          >[0]
        )
      ).toBeUndefined();
    }
    expect(effects).toBe(0);
  });
}
