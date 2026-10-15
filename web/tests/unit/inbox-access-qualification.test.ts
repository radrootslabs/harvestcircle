import { afterEach, expect, it, vi } from 'vitest';
afterEach(() => vi.unstubAllGlobals());
for (const browser of [false, true])
  it(`cold ${browser ? 'browser-shaped' : 'SSR'} access qualification rejects forged provenance before all effects`, async () => {
    vi.resetModules();
    let effects = 0;
    const trap = () => {
      effects++;
      throw Error('unadmitted qualification effect');
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
    const q =
        await import('../../src/lib/messaging/inbox-access-qualification.ts'),
      p = await import('../../src/lib/nostr/private-publisher.ts'),
      a = await import('../../src/lib/nostr/inbox-access.ts');
    type Args = Parameters<typeof q.qualifyInboxAccess>;
    const repository = {} as Args[0],
      session = {} as Args[1],
      policy = {} as Args[2],
      access = {} as Args[3],
      operator = {} as Args[4];
    const context = {
      get owner(): string {
        return trap();
      },
      get session(): symbol {
        return trap();
      },
      get preferenceId(): string {
        return trap();
      },
      get readTargets(): string[] {
        return trap();
      },
      get archiveTargets(): string[] {
        return trap();
      }
    };
    for (const value of [
      undefined,
      null,
      false,
      0,
      '',
      Symbol(),
      {},
      { status: 'qualified', current: () => true, authority: 'operator' },
      { status: 'removed', qualification: {}, generation: 1 }
    ]) {
      expect(
        (
          await q.qualifyInboxAccess(
            repository,
            session,
            policy,
            access,
            operator,
            value as Args[5],
            [],
            'reviewed_qualify_inbox_access'
          )
        ).qualification
      ).toBeUndefined();
      expect(
        q.qualifyRemovedInboxAccess(
          repository,
          session,
          policy,
          access,
          operator,
          value as Parameters<typeof q.qualifyRemovedInboxAccess>[5],
          'reviewed_qualify_inbox_access'
        ).qualification
      ).toBeUndefined();
      const token = value as Parameters<
        typeof q.observeQualifiedInboxAccess
      >[0];
      expect(q.observeQualifiedInboxAccess(token)(context)).toBeUndefined();
      expect(q.inboxAccessQualificationSnapshot(token)).toBeUndefined();
      q.stopInboxAccessQualification(token);
      expect(
        p.captureInboxProbeQualificationFence(
          repository,
          session,
          policy,
          value as Parameters<typeof p.captureInboxProbeQualificationFence>[3]
        )
      ).toBeUndefined();
      expect(
        p.inboxProbeQualificationFenceCurrent(
          value as Parameters<typeof p.inboxProbeQualificationFenceCurrent>[0]
        )
      ).toBe(false);
      expect(
        p.inboxProbeQualifiedRemovalBinding(
          value as Parameters<typeof p.inboxProbeQualifiedRemovalBinding>[0],
          repository,
          session,
          policy
        )
      ).toBeUndefined();
      expect(
        a.inboxProbeOperatorBinding(
          value as Parameters<typeof a.inboxProbeOperatorBinding>[0],
          repository,
          session,
          policy
        )
      ).toBeUndefined();
    }
    expect(effects).toBe(0);
  });
