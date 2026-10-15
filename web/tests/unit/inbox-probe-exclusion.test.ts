import { afterEach, expect, it, vi } from 'vitest';
import {
  generateSecretKey,
  getPublicKey,
  finalizeEvent
} from 'applesauce-core/helpers';
import { nip44 } from 'applesauce-core/helpers/encryption';
afterEach(() => vi.unstubAllGlobals());
for (const browser of [false, true]) {
  it(`cold ${browser ? 'browser-shaped' : 'SSR'} envelope/probe forgeries cannot acquire exclusion or native effects`, async () => {
    vi.resetModules();
    let effects = 0;
    const trap = () => {
      effects++;
      throw Error('unadmitted effect');
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
    const envelopes =
      await import('../../src/lib/persistence/inbox-envelope-repository.ts');
    const probes = await import('../../src/lib/persistence/inbox-probes.ts');
    type Args = Parameters<typeof envelopes.retainInboxEnvelope>;
    const repository = {} as Args[0],
      session = {} as Args[1];
    for (const value of [
      undefined,
      null,
      false,
      0,
      '',
      Symbol(),
      {},
      {
        family: 'private_access_probe',
        status: 'probe_excluded',
        id: 'a'.repeat(64),
        current: () => true
      }
    ]) {
      const envelope = value as Args[2];
      expect(envelopes.inboxEnvelopeOuterMetadata(envelope)).toBeUndefined();
      expect(
        (await envelopes.retainInboxEnvelope(repository, session, envelope))
          .status
      ).toBe('invalid');
      expect(
        await probes.observeInboxProbeDelivery(
          repository,
          session,
          'a'.repeat(64),
          '{}'
        )
      ).toBe('ordinary');
      expect(
        probes.consumeInboxProbeDelivery(
          value as Parameters<typeof probes.consumeInboxProbeDelivery>[0],
          repository,
          session,
          'a'.repeat(64),
          '{}'
        )
      ).toBe(false);
    }
    expect(effects).toBe(0);
  });
}
it('only the genuine admitted envelope owns detached signed outer cursor metadata', async () => {
  const envelopes =
    await import('../../src/lib/persistence/inbox-envelope-repository.ts');
  const secret = generateSecretKey(),
    ownerSecret = generateSecretKey();
  try {
    const owner = getPublicKey(ownerSecret),
      key = nip44.v2.utils.getConversationKey(secret, owner);
    try {
      const event = finalizeEvent(
        {
          kind: 1059,
          created_at: 1700000000,
          tags: [['p', owner]],
          content: nip44.v2.encrypt('ciphertext fixture only', key)
        },
        secret
      );
      const token = envelopes.admitInboxEnvelope(
        JSON.stringify(event),
        owner,
        'wss://inbox.example.org',
        100
      );
      expect(token).toBeDefined();
      const metadata = envelopes.inboxEnvelopeOuterMetadata(token!)! as {
        id: string;
        createdAt: number;
      };
      expect(metadata).toEqual({ id: event.id, createdAt: event.created_at });
      metadata.id = '0'.repeat(64);
      expect(envelopes.inboxEnvelopeOuterMetadata(token!)?.id).toBe(event.id);
      expect(
        envelopes.inboxEnvelopeOuterMetadata({ ...token! })
      ).toBeUndefined();
    } finally {
      key.fill(0);
    }
  } finally {
    secret.fill(0);
    ownerSecret.fill(0);
  }
});
