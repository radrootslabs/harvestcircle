import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { finalizeEvent, verifiedSymbol } from 'applesauce-core/helpers';

const corpus = JSON.parse(
  readFileSync(
    new URL(
      '../../../contracts/interop/food_availability/corpus.v1.json',
      import.meta.url
    ),
    'utf8'
  )
) as { vectors: { id: string; signed_wires: Record<string, string> }[] };
const wires = (suffix: string) =>
  corpus.vectors.find((row) => row.id.endsWith(suffix))!.signed_wires;
let store: typeof import('../../src/lib/nostr/public-store.ts');
let envelope: typeof import('../../src/lib/nostr/verified-envelope.ts');
beforeEach(async () => {
  vi.resetModules();
  store = await import('../../src/lib/nostr/public-store.ts');
  envelope = await import('../../src/lib/nostr/verified-envelope.ts');
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
function proof(raw: string) {
  const result = envelope.verifyEnvelope(raw);
  expect(result.ok).toBe(true);
  if (!result.ok) throw new Error('Expected qualified signed test event');
  return result.value;
}
function browserStore() {
  vi.stubGlobal('window', {});
  return store.getPublicStore()!;
}
// Ephemeral test-only signing material never becomes a fixture, production
// provider, persisted input, output or user identity.
function signed(kind: number, tags: string[][] = [], content = '') {
  const secret = crypto.getRandomValues(new Uint8Array(32));
  return JSON.stringify(
    finalizeEvent({ kind, tags, content, created_at: 1700000060 }, secret)
  );
}
describe('public store contract', () => {
  it('does no browser work during SSR and owns one anonymous browser lifetime', () => {
    vi.stubGlobal('window', undefined);
    expect(store.getPublicStore()).toBeUndefined();
    const owner = browserStore();
    expect(store.getPublicStore()).toBe(owner);
    expect(Object.keys(owner)).toEqual([]);
    expect(Object.isFrozen(owner)).toBe(true);
    store.closePublicStore(owner);
    expect(() => store.getPublicStore()).toThrow('public_store_closed');
  });
  it('has an explicit callable verifier and fixed evidence-preserving SDK options', async () => {
    const { EventStore } = await import('applesauce-core');
    // eslint-disable-next-line @typescript-eslint/unbound-method -- Explicit receiver supplied by original.call(this, event) below.
    const original = EventStore.prototype.add;
    const checks: boolean[] = [];
    vi.spyOn(EventStore.prototype, 'add').mockImplementation(function (
      this: InstanceType<typeof EventStore>,
      event
    ) {
      checks.push(
        typeof this.verifyEvent === 'function' &&
          this.keepOldVersions &&
          this.keepDeleted &&
          this.keepExpired
      );
      expect(this.verifyEvent!(event)).toBe(true);
      expect(this.verifyEvent!({ ...event, sig: '0'.repeat(128) })).toBe(false);
      return original.call(this, event);
    });
    const owner = browserStore();
    expect(store.insertPublicEnvelope(owner, proof(wires('_014').event))).toBe(
      'accepted'
    );
    expect(checks).toEqual([true]);
    expect('verifyEvent' in owner).toBe(false);
    // @ts-expect-error The production acquisition API accepts no store options.
    expect(store.getPublicStore({ verifyEvent: undefined })).toBe(owner);
    store.closePublicStore(owner);
  });
  it('rejects forged and mutated cached input before it can poison a verified winner', () => {
    const owner = browserStore();
    const old = proof(wires('_032').previous);
    expect(store.insertPublicEnvelope(owner, old)).toBe('accepted');
    const invalid = JSON.parse(wires('_032').current) as Record<
      string | symbol,
      unknown
    >;
    invalid.content = 'mutated';
    invalid[verifiedSymbol] = true;
    expect(
      store.insertPublicEnvelope(
        owner,
        invalid as unknown as ReturnType<typeof proof>
      )
    ).toBe('rejected');
    expect(
      store.insertPublicEnvelope(
        owner,
        Object.freeze({}) as ReturnType<typeof proof>
      )
    ).toBe('rejected');
    const event = envelope.verifiedEnvelopeSnapshot(old)!;
    expect(
      store
        .publicStoreVersions(owner, event.kind, event.pubkey, 'nantes-carrots')
        .map((token) => envelope.verifiedEnvelopeSnapshot(token))
    ).toEqual([event]);
    expect(envelope.verifyEnvelope(JSON.stringify(invalid)).ok).toBe(false);
    store.closePublicStore(owner);
  });
  it('preserves both valid versions before focused filtering and exposes detached proof lists', () => {
    const owner = browserStore();
    const newest = proof(wires('_032').current);
    const older = proof(wires('_032').previous);
    expect(store.insertPublicEnvelope(owner, newest)).toBe('accepted');
    expect(store.insertPublicEnvelope(owner, older)).toBe('accepted');
    const event = envelope.verifiedEnvelopeSnapshot(newest)!;
    const versions = store.publicStoreVersions(
      owner,
      event.kind,
      event.pubkey,
      'nantes-carrots'
    );
    expect(
      new Set(
        versions.map((token) => envelope.verifiedEnvelopeSnapshot(token)!.id)
      )
    ).toEqual(
      new Set([
        envelope.verifiedEnvelopeSnapshot(newest)!.id,
        envelope.verifiedEnvelopeSnapshot(older)!.id
      ])
    );
    (versions as unknown as unknown[]).pop();
    expect(
      store.publicStoreVersions(
        owner,
        event.kind,
        event.pubkey,
        'nantes-carrots'
      )
    ).toHaveLength(2);
    expect(
      envelope.verifiedEnvelopeSnapshot(
        store.publicStoreEnvelope(owner, event.id)!
      )
    ).toEqual(event);
    const detached = envelope.verifiedEnvelopeSnapshot(newest)!;
    detached.content = 'changed';
    expect(
      envelope.verifiedEnvelopeSnapshot(
        store.publicStoreEnvelope(owner, event.id)!
      )!.content
    ).toBe(event.content);
    store.closePublicStore(owner);
  });
  it('retains verified incompatible food without applying product admission', () => {
    const owner = browserStore();
    const incompatible = proof(wires('_020').event);
    expect(store.insertPublicEnvelope(owner, incompatible)).toBe('accepted');
    expect(
      envelope.verifiedEnvelopeSnapshot(
        store.publicStoreEnvelope(
          owner,
          envelope.verifiedEnvelopeSnapshot(incompatible)!.id
        )!
      )
    ).toEqual(envelope.verifiedEnvelopeSnapshot(incompatible));
    store.closePublicStore(owner);
  });
  it('retains deletion evidence without giving SDK deletion filtering lifecycle authority', () => {
    const owner = browserStore();
    const food = proof(wires('_014').event);
    const event = envelope.verifiedEnvelopeSnapshot(food)!;
    const deletion = proof(signed(5, [['e', event.id]]));
    expect(store.insertPublicEnvelope(owner, deletion)).toBe('accepted');
    expect(
      envelope.verifiedEnvelopeSnapshot(
        store.publicStoreEnvelope(
          owner,
          envelope.verifiedEnvelopeSnapshot(deletion)!.id
        )!
      )
    ).toEqual(envelope.verifiedEnvelopeSnapshot(deletion));
    expect(store.insertPublicEnvelope(owner, food)).toBe('accepted');
    expect(
      envelope.verifiedEnvelopeSnapshot(
        store.publicStoreEnvelope(owner, event.id)!
      )
    ).toEqual(event);
    store.closePublicStore(owner);
  });
  it.each([4, 13, 14, 1059, 22242])(
    'refuses authentic private or connection event kind %s',
    (kind) => {
      const owner = browserStore();
      const token = proof(signed(kind));
      expect(store.insertPublicEnvelope(owner, token)).toBe('not_public');
      expect(
        store.publicStoreEnvelope(
          owner,
          envelope.verifiedEnvelopeSnapshot(token)!.id
        )
      ).toBeUndefined();
      store.closePublicStore(owner);
    }
  );
  it('strips unprocessed decrypted metadata and SDK cache symbols from stored instances', async () => {
    const { EventStore } = await import('applesauce-core');
    // eslint-disable-next-line @typescript-eslint/unbound-method -- Explicit receiver supplied by original.call(this, event) below.
    const original = EventStore.prototype.add;
    const observed: unknown[] = [];
    vi.spyOn(EventStore.prototype, 'add').mockImplementation(function (
      this: InstanceType<typeof EventStore>,
      event
    ) {
      observed.push(Object.keys(event).sort());
      expect(Object.getOwnPropertySymbols(event)).toEqual([]);
      expect('decrypted' in event).toBe(false);
      return original.call(this, event);
    });
    const owner = browserStore();
    const raw = JSON.stringify({
      ...JSON.parse(wires('_014').event),
      decrypted: 'HC_TEST_ONLY_PRIVATE_METADATA'
    });
    expect(store.insertPublicEnvelope(owner, proof(raw))).toBe('accepted');
    expect(observed).toEqual([
      ['content', 'created_at', 'id', 'kind', 'pubkey', 'sig', 'tags']
    ]);
    const clean = envelope.verifiedEnvelopeSnapshot(
      store.publicStoreEnvelope(owner, (JSON.parse(raw) as { id: string }).id)!
    )!;
    expect('decrypted' in clean).toBe(false);
    expect(Object.keys(clean).sort()).toEqual([
      'content',
      'created_at',
      'id',
      'kind',
      'pubkey',
      'sig',
      'tags'
    ]);
    store.closePublicStore(owner);
  });
  it('deduplicates exact proof without importing duplicate symbols or mutable metadata', () => {
    const owner = browserStore();
    const first = proof(wires('_014').event);
    const second = proof(wires('_014').event);
    expect(store.insertPublicEnvelope(owner, first)).toBe('accepted');
    expect(store.insertPublicEnvelope(owner, second)).toBe('duplicate');
    expect(
      envelope.verifiedEnvelopeSnapshot(
        store.publicStoreEnvelope(
          owner,
          envelope.verifiedEnvelopeSnapshot(first)!.id
        )!
      )
    ).toEqual(envelope.verifiedEnvelopeSnapshot(first));
    store.closePublicStore(owner);
  });
  it('retains signed expired data as evidence before product display policy', () => {
    const owner = browserStore();
    const token = proof(
      signed(30402, [
        ['d', 'expired'],
        ['expiration', '1']
      ])
    );
    expect(store.insertPublicEnvelope(owner, token)).toBe('accepted');
    expect(
      envelope.verifiedEnvelopeSnapshot(
        store.publicStoreEnvelope(
          owner,
          envelope.verifiedEnvelopeSnapshot(token)!.id
        )!
      )
    ).toEqual(envelope.verifiedEnvelopeSnapshot(token));
    store.closePublicStore(owner);
  });
  it('rejects forged owners and terminally releases public proof on disposal', async () => {
    const { EventStore } = await import('applesauce-core');
    const disposal = vi.spyOn(EventStore.prototype, 'dispose');
    expect(() =>
      store.closePublicStore({} as Parameters<typeof store.closePublicStore>[0])
    ).toThrow('public_store_invalid');
    const owner = browserStore();
    const token = proof(wires('_014').event);
    store.insertPublicEnvelope(owner, token);
    store.closePublicStore(owner);
    store.closePublicStore(owner);
    expect(disposal).toHaveBeenCalledTimes(1);
    expect(
      store.publicStoreEnvelope(
        owner,
        envelope.verifiedEnvelopeSnapshot(token)!.id
      )
    ).toBeUndefined();
    expect(store.insertPublicEnvelope(owner, token)).toBe('closed');
    expect(store.publicStoreVersions(owner, 30402, '0'.repeat(64), '')).toEqual(
      []
    );
  });
});
