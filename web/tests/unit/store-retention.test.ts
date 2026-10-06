import { beforeEach, afterEach, describe, it, expect, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { finalizeEvent } from 'applesauce-core/helpers';

let store: typeof import('../../src/lib/nostr/public-store.ts');
let envelope: typeof import('../../src/lib/nostr/verified-envelope.ts');
let heads: typeof import('../../src/lib/catalog/heads.ts');
let deletions: typeof import('../../src/lib/catalog/deletions.ts');
beforeEach(async () => {
  vi.resetModules();
  store = await import('../../src/lib/nostr/public-store.ts');
  envelope = await import('../../src/lib/nostr/verified-envelope.ts');
  heads = await import('../../src/lib/catalog/heads.ts');
  deletions = await import('../../src/lib/catalog/deletions.ts');
  vi.stubGlobal('window', {});
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
const corpus = JSON.parse(
  readFileSync(
    new URL(
      '../../../contracts/interop/food_availability/corpus.v1.json',
      import.meta.url
    ),
    'utf8'
  )
) as { vectors: { id: string; signed_wires: Record<string, string> }[] };
const base = JSON.parse(
  corpus.vectors.find((v) => v.id.endsWith('_032'))!.signed_wires.previous
) as { tags: string[][]; content: string; created_at: number };
function wire(
  time: number,
  tags = base.tags,
  content = base.content,
  kind = 30402,
  key?: Uint8Array
) {
  const secret = key ?? crypto.getRandomValues(new Uint8Array(32));
  try {
    return JSON.stringify(
      finalizeEvent({ kind, created_at: time, tags, content }, secret)
    );
  } finally {
    if (!key) secret.fill(0);
  }
}
function proof(raw: string) {
  const p = envelope.verifyEnvelope(raw);
  if (!p.ok) throw Error('store retention fixture');
  return p.value;
}
describe('actual SDK public working-set ownership', () => {
  it('indexes retained generic newer heads and same-author deletion without SDK erasure', () => {
    const owner = store.getPublicStore()!,
      key = crypto.getRandomValues(new Uint8Array(32));
    try {
      const old = proof(
          wire(base.created_at, base.tags, base.content, 30402, key)
        ),
        next = proof(
          wire(
            base.created_at + 1,
            base.tags.map((t) =>
              t[0] === 'status' ? ['status', 'unknown'] : [...t]
            ),
            base.content,
            30402,
            key
          )
        );
      const nextId = envelope.verifiedEnvelopeSnapshot(next)!.id;
      const request = proof(
        wire(base.created_at - 1, [['e', nextId]], '', 5, key)
      );
      for (const p of [old, request, next])
        expect(store.insertPublicEnvelope(owner, p)).toBe('accepted');
      const h = heads.createPublicHeadCandidate(old)!;
      const known = store.publicStoreKnownEvidence(owner, h);
      expect(heads.publicHeadSnapshot(known.head).id).toBe(nextId);
      expect(
        deletions.evaluatePublicHeadDeletion(known.head, known.requests).outcome
      ).toBe('suppressed');
      expect(store.publicStoreEnvelope(owner, nextId)).toBeDefined();
      expect(store.publicStoreRetention(owner).events).toBe(3);
    } finally {
      key.fill(0);
      store.closePublicStore(owner);
    }
  });
  it('rejects signed private kinds without changing public retention or exposing their envelope', () => {
    const owner = store.getPublicStore()!;
    try {
      const p = proof(
        wire(base.created_at, [], 'opaque private test envelope', 1059)
      );
      expect(store.insertPublicEnvelope(owner, p)).toBe('not_public');
      expect(store.publicStoreRetention(owner)).toMatchObject({
        payloadBytes: 0,
        events: 0,
        stopped: false
      });
      expect(
        store.publicStoreEnvelope(
          owner,
          envelope.verifiedEnvelopeSnapshot(p)!.id
        )
      ).toBeUndefined();
    } finally {
      store.closePublicStore(owner);
    }
  });
});

for (const evidenceKind of ['newer', 'deletion'] as const)
  it(`normal shared runtime close settles unconsumed ${evidenceKind} evidence before SDK disposal`, async () => {
    const runtime = await import('../../src/lib/runtime/public-runtime.ts'),
      resolver = await import('../../src/lib/catalog/resolve-head.ts');
    const context = runtime.createPublicRuntimeContext(),
      mounted = runtime.mountPublicRuntime(context)!,
      view = runtime.createPublicView(mounted),
      run = runtime.beginPublicViewRun(view);
    const key = crypto.getRandomValues(new Uint8Array(32));
    try {
      const old = proof(
          wire(base.created_at, base.tags, base.content, 30402, key)
        ),
        h = heads.createPublicHeadCandidate(old)!,
        model = resolver.createPublicViewHeadResolver(view, run, {
          nowSeconds: () => base.created_at + 10
        });
      resolver.resolveHeads(model, [h]);
      expect(
        resolver.headResolutionSnapshot(model)[0].state.food
      ).toBeDefined();
      const next = proof(
        wire(
          base.created_at + 1,
          base.tags.map((t) =>
            t[0] === 'status' ? ['status', 'unknown'] : [...t]
          ),
          base.content,
          30402,
          key
        )
      );
      const admitted =
        evidenceKind === 'newer'
          ? next
          : proof(
              wire(
                base.created_at - 1,
                [['e', envelope.verifiedEnvelopeSnapshot(old)!.id]],
                '',
                5,
                key
              )
            );
      expect(
        store.insertPublicEnvelope(store.getPublicStore()!, admitted)
      ).toBe('accepted');
      runtime.closePublicRuntime(context);
      const row = resolver.headResolutionSnapshot(model)[0];
      expect(row.lastKnown).toBe(true);
      expect(row.state.food).toBeUndefined();
      if (evidenceKind === 'newer')
        expect(row.state.head.id).toBe(
          envelope.verifiedEnvelopeSnapshot(next)!.id
        );
      else expect(row.deletion.outcome).toBe('suppressed');
    } finally {
      key.fill(0);
      runtime.closePublicRuntime(context);
    }
  });
it('a settlement clock reentry cannot overwrite a newer view run', async () => {
  const runtime = await import('../../src/lib/runtime/public-runtime.ts'),
    resolver = await import('../../src/lib/catalog/resolve-head.ts');
  const context = runtime.createPublicRuntimeContext(),
    mounted = runtime.mountPublicRuntime(context)!,
    view = runtime.createPublicView(mounted),
    run = runtime.beginPublicViewRun(view);
  const key = crypto.getRandomValues(new Uint8Array(32));
  let armed = false,
    nested: ReturnType<typeof runtime.beginPublicViewRun> | undefined;
  try {
    const old = proof(
        wire(base.created_at, base.tags, base.content, 30402, key)
      ),
      next = proof(
        wire(base.created_at + 1, base.tags, base.content, 30402, key)
      ),
      model = resolver.createPublicViewHeadResolver(view, run, {
        nowSeconds: () => {
          if (armed) {
            armed = false;
            nested = runtime.beginPublicViewRun(view);
          }
          return base.created_at + 10;
        }
      });
    resolver.resolveHeads(model, [heads.createPublicHeadCandidate(old)!]);
    store.insertPublicEnvelope(store.getPublicStore()!, next);
    armed = true;
    expect(() => runtime.beginPublicViewRun(view)).toThrow(
      'public_view_run_superseded'
    );
    expect(nested).toBeDefined();
    expect(runtime.publicViewRunCurrent(view, nested!)).toBe(true);
    expect(resolver.headResolutionSnapshot(model)[0].state.head.id).toBe(
      envelope.verifiedEnvelopeSnapshot(next)!.id
    );
  } finally {
    key.fill(0);
    runtime.closePublicRuntime(context);
  }
});
it('terminal public invalidation retries failed view settlement independently', async () => {
  const runtime = await import('../../src/lib/runtime/public-runtime.ts');
  const context = runtime.createPublicRuntimeContext(),
    mounted = runtime.mountPublicRuntime(context)!,
    a = runtime.createPublicView(mounted),
    b = runtime.createPublicView(mounted);
  const arun = runtime.beginPublicViewRun(a),
    brun = runtime.beginPublicViewRun(b);
  let acalls = 0,
    bcalls = 0;
  runtime.settlePublicViewProjection(a, () => {
    acalls++;
    if (acalls === 1) throw Error('one settlement failure');
  });
  runtime.settlePublicViewProjection(b, () => {
    bcalls++;
  });
  try {
    store.closePublicStore(store.getPublicStore()!);
    expect(() => runtime.publicViewProjectionAvailable(a)).toThrow(
      'public_projection_close_failed'
    );
    expect(acalls).toBe(1);
    expect(bcalls).toBe(1);
    expect(runtime.publicViewProjectionAvailable(a)).toBe(false);
    expect(acalls).toBe(2);
    expect(bcalls).toBe(1);
    expect(runtime.publicViewRunCurrent(a, arun)).toBe(false);
    expect(runtime.publicViewRunCurrent(b, brun)).toBe(false);
  } finally {
    runtime.closePublicRuntime(context);
  }
});
it('nested close during settlement cannot discard newly admitted shared evidence beneath returned rows', async () => {
  const runtime = await import('../../src/lib/runtime/public-runtime.ts'),
    resolver = await import('../../src/lib/catalog/resolve-head.ts');
  const context = runtime.createPublicRuntimeContext(),
    mounted = runtime.mountPublicRuntime(context)!,
    view = runtime.createPublicView(mounted),
    run = runtime.beginPublicViewRun(view),
    key = crypto.getRandomValues(new Uint8Array(32));
  let armed = false;
  try {
    const old = proof(
        wire(base.created_at, base.tags, base.content, 30402, key)
      ),
      next = proof(
        wire(base.created_at + 1, base.tags, base.content, 30402, key)
      ),
      unknown = proof(
        wire(
          base.created_at + 2,
          base.tags.map((t) =>
            t[0] === 'status' ? ['status', 'unknown'] : [...t]
          ),
          base.content,
          30402,
          key
        )
      );
    const model = resolver.createPublicViewHeadResolver(view, run, {
      nowSeconds: () => {
        if (armed) {
          armed = false;
          expect(
            store.insertPublicEnvelope(store.getPublicStore()!, unknown)
          ).toBe('accepted');
          runtime.closePublicRuntime(context);
        }
        return base.created_at + 10;
      }
    });
    resolver.resolveHeads(model, [heads.createPublicHeadCandidate(old)!]);
    store.insertPublicEnvelope(store.getPublicStore()!, next);
    armed = true;
    runtime.closePublicRuntime(context);
    expect(resolver.headResolutionSnapshot(model)).toEqual([]);
    expect(runtime.publicViewProjectionAvailable(view)).toBe(false);
  } finally {
    key.fill(0);
    runtime.closePublicRuntime(context);
  }
});
