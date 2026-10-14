import { afterEach, expect, it, vi } from 'vitest';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import type { InboxProbeGiftwrap } from '../../src/lib/nostr/giftwrap-builder.ts';
import type { InboxProbeAcknowledgement } from '../../src/lib/persistence/inbox-probes.ts';
import { pairedDeliveryAcknowledgementSnapshot } from '../../src/lib/persistence/private-sends.ts';

afterEach(() => vi.unstubAllGlobals());
it('cold SSR probe persistence import and forged acknowledgements have no browser effects', async () => {
  vi.resetModules();
  const api = await import('../../src/lib/persistence/inbox-probes.ts');
  expect(
    await api.commitInboxProbe(
      {} as PrivateStorageRepository,
      {} as PrivateSession,
      {} as InboxProbeGiftwrap,
      'reviewed_inbox_probe_commit'
    )
  ).toEqual({ status: 'invalid', disposition: 'original_protected' });
  expect(
    api.inboxProbeAcknowledgementSnapshot({} as InboxProbeAcknowledgement)
  ).toBeUndefined();
  expect(
    await api.verifyInboxProbeAcknowledgement(
      {} as PrivateStorageRepository,
      {} as PrivateSession,
      {} as InboxProbeAcknowledgement
    )
  ).toBe(false);
});

it('browser-shaped forged inputs and consent coercion cannot access storage or a provider', async () => {
  vi.resetModules();
  let observations = 0;
  const trap = () => {
    observations++;
    throw Error('unreviewed browser work');
  };
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
  const api = await import('../../src/lib/persistence/inbox-probes.ts');
  const copy = Object.freeze({ owner: 'a'.repeat(64), current: () => true });
  for (const primitive of [null, undefined, false, 0, 'wrap', Symbol()]) {
    expect(
      await api.commitInboxProbe(
        copy as unknown as PrivateStorageRepository,
        copy as unknown as PrivateSession,
        primitive as unknown as InboxProbeGiftwrap,
        'reviewed_inbox_probe_commit'
      )
    ).toEqual({ status: 'invalid', disposition: 'original_protected' });
  }
  for (const review of ['reviewed_inbox_probe_commit', { toString: trap }]) {
    expect(
      await api.commitInboxProbe(
        copy as unknown as PrivateStorageRepository,
        copy as unknown as PrivateSession,
        copy as unknown as InboxProbeGiftwrap,
        review
      )
    ).toEqual({ status: 'invalid', disposition: 'original_protected' });
  }
  expect(
    api.inboxProbeAcknowledgementSnapshot(
      copy as unknown as InboxProbeAcknowledgement
    )
  ).toBeUndefined();
  expect(
    pairedDeliveryAcknowledgementSnapshot(
      copy as unknown as Parameters<
        typeof pairedDeliveryAcknowledgementSnapshot
      >[0]
    )
  ).toBeUndefined();
  expect(observations).toBe(0);
});
