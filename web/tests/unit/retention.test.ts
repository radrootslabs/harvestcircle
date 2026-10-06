import { afterAll, describe, expect, it } from 'vitest';
import { finalizeEvent } from 'applesauce-core/helpers';
import { verifyEnvelope } from '../../src/lib/nostr/verified-envelope.ts';
import {
  createPublicHeadCandidate,
  publicHeadSnapshot
} from '../../src/lib/catalog/heads.ts';
import { evaluatePublicHeadDeletion } from '../../src/lib/catalog/deletions.ts';
import {
  createPublicRetention,
  retainPublicEnvelope,
  publicRetentionSnapshot,
  publicRetentionEnvelope,
  publicRetentionKnown,
  closePublicRetention
} from '../../src/lib/catalog/retention.ts';

const keys: Uint8Array[] = [];
afterAll(() => {
  for (const key of keys) key.fill(0);
});
function fixture() {
  const key = crypto.getRandomValues(new Uint8Array(32));
  keys.push(key);
  const proof = (
    time: number,
    tags: string[][] = [['d', 'item']],
    content = '',
    kind = 30402
  ) => {
    const raw = JSON.stringify(
      finalizeEvent({ kind, created_at: time, tags, content }, key)
    );
    const p = verifyEnvelope(raw);
    if (!p.ok) throw Error('signed retention fixture');
    return {
      proof: p.value,
      raw,
      event: JSON.parse(raw) as { id: string; pubkey: string }
    };
  };
  const head = (p: ReturnType<typeof proof>) => {
    const h = createPublicHeadCandidate(p.proof);
    if (!h) throw Error('head fixture');
    return h;
  };
  return { proof, head, owner: createPublicRetention() };
}
describe('coherent public working-set retention', () => {
  it('counts actual UTF8 serialized event payload once per verified ID and exposes detached metrics', () => {
    const f = fixture(),
      p = f.proof(100, [['d', '菜']], 'multiline\n菜');
    expect(retainPublicEnvelope(f.owner, p.proof)).toBe('accepted');
    const bytes = new TextEncoder().encode(p.raw).length;
    expect(publicRetentionSnapshot(f.owner)).toMatchObject({
      payloadBytes: bytes,
      events: 1,
      stopped: false,
      closed: false
    });
    expect(retainPublicEnvelope(f.owner, p.proof)).toBe('duplicate');
    const row = publicRetentionSnapshot(f.owner);
    (row as unknown as { payloadBytes: number }).payloadBytes = 0;
    expect(publicRetentionSnapshot(f.owner).payloadBytes).toBe(bytes);
    expect(publicRetentionEnvelope(f.owner, p.event.id)).toBe(p.proof);
  });
  it('retains the generic newest incompatible coordinate winner ahead of focused selection', () => {
    const f = fixture(),
      old = f.proof(100),
      next = f.proof(101, [
        ['d', 'item'],
        ['status', 'unknown']
      ]);
    retainPublicEnvelope(f.owner, old.proof);
    retainPublicEnvelope(f.owner, next.proof);
    const known = publicRetentionKnown(f.owner, f.head(old));
    expect(publicHeadSnapshot(known.head).id).toBe(next.event.id);
    expect(publicRetentionEnvelope(f.owner, old.event.id)).toBe(old.proof);
  });
  it('retains authorized tombstones before their target and across later lookups without redelivery', () => {
    const f = fixture(),
      target = f.proof(100),
      request = f.proof(1, [['e', target.event.id]], '', 5);
    retainPublicEnvelope(f.owner, request.proof);
    retainPublicEnvelope(f.owner, target.proof);
    const known = publicRetentionKnown(f.owner, f.head(target));
    expect(known.requests).toHaveLength(1);
    expect(evaluatePublicHeadDeletion(known.head, known.requests).outcome).toBe(
      'suppressed'
    );
    expect(retainPublicEnvelope(f.owner, target.proof)).toBe('duplicate');
    expect(
      evaluatePublicHeadDeletion(
        known.head,
        publicRetentionKnown(f.owner, f.head(target)).requests
      ).outcome
    ).toBe('suppressed');
  });
  it('retains greatest inclusive address cutoff and canonical exact-ID authority without imposing k or time on e', () => {
    const f = fixture(),
      target = f.proof(100),
      coordinate = `30402:${target.event.pubkey}:item`;
    const old = f.proof(99, [['a', coordinate]], '', 5),
      current = f.proof(
        100,
        [
          ['a', coordinate],
          ['k', '30402']
        ],
        '',
        5
      ),
      exact = f.proof(1, [['e', target.event.id]], '', 5);
    for (const p of [target, old, current, exact])
      retainPublicEnvelope(f.owner, p.proof);
    const known = publicRetentionKnown(f.owner, f.head(target));
    const decision = evaluatePublicHeadDeletion(known.head, known.requests);
    expect(decision.outcome).toBe('suppressed');
    expect(decision.addressReference?.inclusiveCutoff).toBe(100);
    expect(decision.eventReference?.requestId).toBe(exact.event.id);
  });
  it('a missing first d cannot acquire address deletion authority from generic empty head identity', () => {
    const f = fixture(),
      target = f.proof(100, []),
      request = f.proof(100, [['a', `30402:${target.event.pubkey}:`]], '', 5);
    retainPublicEnvelope(f.owner, target.proof);
    retainPublicEnvelope(f.owner, request.proof);
    const known = publicRetentionKnown(f.owner, f.head(target));
    expect(evaluatePublicHeadDeletion(known.head, known.requests).outcome).toBe(
      'visible'
    );
  });
  it('denies forged and non-public proofs before installation or public accounting', () => {
    const f = fixture();
    let installs = 0;
    const install = () => {
      installs++;
      return true;
    };
    expect(
      retainPublicEnvelope(
        f.owner,
        {} as ReturnType<typeof f.proof>['proof'],
        install
      )
    ).toBe('rejected');
    expect(
      retainPublicEnvelope(
        f.owner,
        f.proof(100, [], 'opaque test envelope', 1059).proof,
        install
      )
    ).toBe('not_public');
    expect(installs).toBe(0);
    expect(publicRetentionSnapshot(f.owner)).toMatchObject({
      events: 0,
      payloadBytes: 0,
      stopped: false
    });
  });
  it('accepts the inclusive real32MiB boundary, then stops before installation and retains all lifecycle evidence', () => {
    const f = fixture(),
      target = f.proof(100),
      request = f.proof(1, [['e', target.event.id]], '', 5);
    for (const p of [target, request]) retainPublicEnvelope(f.owner, p.proof);
    const cap = 33554432;
    let time = 1000;
    for (;;) {
      const empty = f.proof(time, [['d', `bulk-${time}`]]),
        overhead = new TextEncoder().encode(empty.raw).length,
        remaining = cap - publicRetentionSnapshot(f.owner).payloadBytes;
      const length = Math.min(131000, remaining - overhead);
      expect(length).toBeGreaterThanOrEqual(0);
      const p = f.proof(time, [['d', `bulk-${time}`]], 'a'.repeat(length));
      expect(retainPublicEnvelope(f.owner, p.proof)).toBe('accepted');
      time++;
      if (publicRetentionSnapshot(f.owner).payloadBytes === cap) break;
    }
    expect(publicRetentionSnapshot(f.owner)).toMatchObject({
      payloadBytes: cap,
      stopped: false
    });
    expect(retainPublicEnvelope(f.owner, target.proof)).toBe('duplicate');
    let installs = 0;
    const tooMany = f.proof(time);
    expect(
      retainPublicEnvelope(f.owner, tooMany.proof, () => {
        installs++;
        return true;
      })
    ).toBe('limit');
    expect(installs).toBe(0);
    expect(publicRetentionSnapshot(f.owner)).toMatchObject({
      payloadBytes: cap,
      stopped: true,
      reason: 'payload_limit'
    });
    expect(retainPublicEnvelope(f.owner, target.proof)).toBe('limit');
    const known = publicRetentionKnown(f.owner, f.head(target));
    expect(evaluatePublicHeadDeletion(known.head, known.requests).outcome).toBe(
      'suppressed'
    );
    expect(publicRetentionEnvelope(f.owner, request.event.id)).toBe(
      request.proof
    );
  }, 30000);
  it('reserves coherent evidence before installer reentry and stops on installation failure', () => {
    const f = fixture(),
      old = f.proof(100),
      newer = f.proof(101);
    expect(
      retainPublicEnvelope(f.owner, old.proof, () => {
        expect(publicRetentionEnvelope(f.owner, old.event.id)).toBe(old.proof);
        expect(retainPublicEnvelope(f.owner, newer.proof)).toBe('accepted');
        return true;
      })
    ).toBe('accepted');
    expect(
      publicHeadSnapshot(publicRetentionKnown(f.owner, f.head(old)).head).id
    ).toBe(newer.event.id);
    const failed = f.proof(102);
    expect(retainPublicEnvelope(f.owner, failed.proof, () => false)).toBe(
      'rejected'
    );
    expect(publicRetentionSnapshot(f.owner)).toMatchObject({
      stopped: true,
      reason: 'installation_unavailable'
    });
    expect(retainPublicEnvelope(f.owner, old.proof)).toBe('limit');
  });
  it('terminal disposal cannot reset public admission or touch a separately owned private envelope', () => {
    const f = fixture(),
      publicValue = f.proof(100),
      privateValue = f.proof(100, [], 'opaque private test payload', 1059);
    retainPublicEnvelope(f.owner, publicValue.proof);
    const original = privateValue.raw;
    closePublicRetention(f.owner);
    expect(publicRetentionSnapshot(f.owner)).toMatchObject({
      closed: true,
      events: 0,
      payloadBytes: 0
    });
    expect(retainPublicEnvelope(f.owner, publicValue.proof)).toBe('closed');
    expect(
      publicRetentionEnvelope(f.owner, publicValue.event.id)
    ).toBeUndefined();
    expect(privateValue.raw).toBe(original);
    expect(verifyEnvelope(privateValue.raw).ok).toBe(true);
  });
});

import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openPublicRequest,
  closePublicScheduler
} from '../../src/lib/nostr/request-scope.ts';
import {
  createHeadResolver,
  resolveHeads,
  headResolutionSnapshot
} from '../../src/lib/catalog/resolve-head.ts';
function resolverFixture() {
  const f = fixture();
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [
        {
          origin: 'wss://one.example.org',
          read: true,
          write: false,
          nip50: false
        }
      ],
      inbox: [],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  )!;
  const scheduler = createPublicScheduler({
      now: () => 0,
      schedule: () => () => {}
    }),
    run = createPublicRun(scheduler, policy);
  return { ...f, scheduler, run };
}
it('retries failed projection cleanup without repeating a successful request stop', () => {
  const f = resolverFixture();
  let available = true,
    headStops = 0,
    deletionStops = 0;
  const resolver = createHeadResolver(
    f.run,
    (kind, filters, next) =>
      openPublicRequest(
        f.run,
        kind,
        () => () => {
          if (kind === 'head') {
            headStops++;
            if (headStops === 1) throw Error('one cleanup failure');
          } else deletionStops++;
        },
        next
      ),
    { nowSeconds: () => 200 },
    {
      retain: () => true,
      read: (head) => ({ head, requests: [] }),
      available: () => available
    }
  );
  try {
    resolveHeads(resolver, [f.head(f.proof(100))]);
    available = false;
    expect(() => headResolutionSnapshot(resolver)).toThrow(
      'head_resolver_close_failed'
    );
    expect(headStops).toBe(1);
    expect(deletionStops).toBe(1);
    expect(headResolutionSnapshot(resolver)).toEqual([]);
    expect(headStops).toBe(2);
    expect(deletionStops).toBe(1);
  } finally {
    closePublicScheduler(f.scheduler);
  }
});
it('a snapshot clock cannot publish rows after reentrant retention invalidation', () => {
  const f = resolverFixture();
  let available = true,
    armed = false;
  const old = f.proof(100),
    newer = f.proof(101);
  retainPublicEnvelope(f.owner, old.proof);
  const resolver = createHeadResolver(
    f.run,
    (kind, filters, next) =>
      openPublicRequest(f.run, kind, () => () => {}, next),
    {
      nowSeconds: () => {
        if (armed) available = false;
        return 200;
      }
    },
    {
      retain: () => true,
      read: (head) => publicRetentionKnown(f.owner, head),
      available: () => available
    }
  );
  try {
    resolveHeads(resolver, [f.head(old)]);
    retainPublicEnvelope(f.owner, newer.proof);
    armed = true;
    expect(headResolutionSnapshot(resolver)).toEqual([]);
    expect(headResolutionSnapshot(resolver)).toEqual([]);
  } finally {
    closePublicScheduler(f.scheduler);
  }
});
