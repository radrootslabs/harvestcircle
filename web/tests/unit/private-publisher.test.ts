import { describe, it, expect } from 'vitest';
import {
  capturePrivatePublication,
  privatePublicationSnapshot,
  takePrivatePublication,
  type PrivatePublication
} from '../../src/lib/nostr/private-publisher.ts';
import {
  publishPrivateGiftWrapAttempt,
  type PrivatePool
} from '../../src/lib/nostr/private-pool.ts';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type { PrivateStorageRepository } from '../../src/lib/persistence/private-storage.ts';
import type { PairedDeliveryAcknowledgement } from '../../src/lib/persistence/private-sends.ts';
import type { PairedDeliveryContext } from '../../src/lib/messaging/prepare-send.ts';
describe('private publication has no arbitrary event sink', () => {
  it('SSR refuses forged local acknowledgement and publication without effects', async () => {
    const session = {} as PrivateSession;
    expect(
      capturePrivatePublication(
        {} as PrivateStorageRepository,
        session,
        {} as PairedDeliveryAcknowledgement,
        {} as PairedDeliveryContext,
        'peer',
        'wss://peer.example.org',
        'reviewed_private_delivery'
      )
    ).toBeUndefined();
    expect(
      privatePublicationSnapshot({} as PrivatePublication)
    ).toBeUndefined();
    expect(
      await takePrivatePublication(
        {} as PrivatePublication,
        session,
        {} as PairedDeliveryContext['policy'],
        'wss://peer.example.org'
      )
    ).toBeUndefined();
    expect(
      await publishPrivateGiftWrapAttempt(
        {} as PrivatePool,
        {} as PrivatePublication,
        new AbortController().signal
      )
    ).toEqual({ status: 'stopped' });
  });
  it.each([14, 13, 1059])(
    'a caller event of kind %i cannot act as publication permission',
    async (kind) => {
      const raw = {
        kind,
        content: 'PRIVATE_TEXT',
        tags: [],
        id: '0'.repeat(64)
      } as unknown as PrivatePublication;
      expect(privatePublicationSnapshot(raw)).toBeUndefined();
      expect(
        await publishPrivateGiftWrapAttempt(
          {} as PrivatePool,
          raw,
          new AbortController().signal
        )
      ).toEqual({ status: 'stopped' });
    }
  );
  it('review objects and forged capabilities are not coerced', () => {
    let calls = 0;
    const review = {
      toString() {
        calls++;
        return 'reviewed_private_delivery';
      }
    };
    expect(
      capturePrivatePublication(
        {} as PrivateStorageRepository,
        {} as PrivateSession,
        {} as PairedDeliveryAcknowledgement,
        {} as PairedDeliveryContext,
        'peer',
        'wss://peer.example.org',
        review
      )
    ).toBeUndefined();
    expect(calls).toBe(0);
  });
});
