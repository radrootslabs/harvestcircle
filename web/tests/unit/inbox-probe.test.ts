import { afterEach, expect, it, vi } from 'vitest';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { InboxAccessController } from '../../src/lib/messaging/inbox-access-controller.ts';
import type { RelayPolicy } from '../../src/lib/config/relays.ts';
import type { InboxOperatorQualification } from '../../src/lib/config/inbox-operators.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import type { InboxProbeIdentity } from '../../src/lib/nostr/inbox-access.ts';
import type {
  InboxProbeRecipientSeal,
  PrivateRecipientSeal,
  PrivateSealOperation,
  InboxProbeSealOperation
} from '../../src/lib/nostr/seal-builder.ts';
import type {
  InboxProbeGiftwrap,
  PrivateGiftwrap
} from '../../src/lib/nostr/giftwrap-builder.ts';

afterEach(() => vi.unstubAllGlobals());
it('cold SSR import resolves the real controller/publisher/builder cycle without browser effects', async () => {
  vi.resetModules();
  expect(typeof window).toBe('undefined');
  // The repository SSR setup traps indexedDB/WebSocket/nostr/fetch accesses.
  const probe = await import('../../src/lib/nostr/inbox-access.ts');
  const seal = await import('../../src/lib/nostr/seal-builder.ts');
  const wrap = await import('../../src/lib/nostr/giftwrap-builder.ts');
  expect(probe.inboxProbeSnapshot({} as InboxProbeIdentity)).toBeUndefined();
  expect(
    await probe.reserveInboxProbeIdentity(
      {} as PrivateStorageRepository,
      {} as PrivateSession,
      {} as RelayPolicy,
      {} as InboxAccessController,
      {} as InboxOperatorQualification,
      'reviewed_inbox_probe_reservation'
    )
  ).toEqual({ status: 'invalid_scope', disposition: 'not_created' });
  expect(
    seal.captureInboxProbeSealOperation(
      {} as PrivateSession,
      {} as InboxProbeIdentity,
      'reviewed_inbox_probe_seal'
    )
  ).toBeUndefined();
  expect(
    wrap.buildInboxProbeGiftwrap(
      {} as InboxProbeRecipientSeal,
      'reviewed_inbox_probe_wrap'
    )
  ).toBeUndefined();
});

it('cold browser-shaped import and copied capabilities cannot observe a provider or storage', async () => {
  vi.resetModules();
  let observations = 0;
  vi.stubGlobal('window', {
    get nostr() {
      observations++;
      throw Error('unreviewed provider observation');
    }
  });
  vi.stubGlobal('navigator', {
    get locks() {
      observations++;
      throw Error('unreviewed owner lock');
    }
  });
  const probe = await import('../../src/lib/nostr/inbox-access.ts');
  const seal = await import('../../src/lib/nostr/seal-builder.ts');
  const wrap = await import('../../src/lib/nostr/giftwrap-builder.ts');
  const copy = Object.freeze({ owner: 'a'.repeat(64), current: () => true });
  expect(
    await probe.reserveInboxProbeIdentity(
      copy as unknown as PrivateStorageRepository,
      copy as unknown as PrivateSession,
      copy as unknown as RelayPolicy,
      copy as unknown as InboxAccessController,
      copy as unknown as InboxOperatorQualification,
      'reviewed_inbox_probe_reservation'
    )
  ).toEqual({ status: 'invalid_scope', disposition: 'not_created' });
  expect(
    await seal.buildInboxProbeSeal(copy as unknown as InboxProbeSealOperation)
  ).toEqual({ status: 'invalid' });
  expect(
    await seal.buildPrivateSeal(copy as unknown as PrivateSealOperation)
  ).toEqual({ status: 'invalid' });
  expect(
    seal.inboxProbeSealSnapshot(copy as unknown as InboxProbeRecipientSeal)
  ).toBeUndefined();
  expect(
    seal.privateSealSnapshot(copy as unknown as PrivateRecipientSeal)
  ).toBeUndefined();
  expect(
    wrap.buildInboxProbeGiftwrap(
      copy as unknown as InboxProbeRecipientSeal,
      'reviewed_inbox_probe_wrap'
    )
  ).toBeUndefined();
  expect(
    wrap.buildPrivateGiftwrap(
      copy as unknown as PrivateRecipientSeal,
      'reviewed_private_wrap'
    )
  ).toBeUndefined();
  expect(
    wrap.inboxProbeGiftwrapSnapshot(copy as unknown as InboxProbeGiftwrap)
  ).toBeUndefined();
  expect(
    wrap.privateGiftwrapSnapshot(copy as unknown as PrivateGiftwrap)
  ).toBeUndefined();
  expect(observations).toBe(0);
});
