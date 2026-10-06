import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { verifiedSymbol, verifyEvent } from 'applesauce-core/helpers';
import { PUBLIC_INGRESS_BUDGETS } from '../../src/lib/config/budgets.ts';
import {
  createPublicIngress,
  admitPublicEvent,
  publicIngressStats,
  type PublicIngress
} from '../../src/lib/nostr/ingress.ts';
import { verifiedEnvelopeSnapshot } from '../../src/lib/nostr/verified-envelope.ts';
const corpus = JSON.parse(
  readFileSync(
    new URL(
      '../../../contracts/interop/food_availability/corpus.v1.json',
      import.meta.url
    ),
    'utf8'
  )
) as { vectors: { id: string; signed_wires: Record<string, string> }[] };
const wire = corpus.vectors.find((row) => row.id.endsWith('_014'))!.signed_wires
  .event;
const candidate = () => JSON.parse(wire) as Record<string | symbol, unknown>;

describe('bounded generic public ingress', () => {
  it('isolates a bad signature and admits the following valid candidate', () => {
    const owner = createPublicIngress();
    expect(
      admitPublicEvent(owner, { ...candidate(), sig: '0'.repeat(128) }).status
    ).toBe('rejected');
    const good = admitPublicEvent(owner, wire);
    expect(good.status).toBe('accepted');
    if (good.status === 'accepted')
      expect(verifiedEnvelopeSnapshot(good.value)).toEqual(JSON.parse(wire));
    expect(publicIngressStats(owner).deliveries).toBe(2);
  });
  it('rejects a mutated object even when actual SDK verification trusts its cached symbol', () => {
    const cached = candidate();
    expect(verifyEvent(cached as Parameters<typeof verifyEvent>[0])).toBe(true);
    expect(cached[verifiedSymbol]).toBe(true);
    cached.content = 'mutation after verification';
    expect(verifyEvent(cached as Parameters<typeof verifyEvent>[0])).toBe(true);
    expect(admitPublicEvent(createPublicIngress(), cached).status).toBe(
      'rejected'
    );
  });
  it('reconstructs signed data without cache symbols or caller mutation crossing retained proof', () => {
    const input = candidate();
    input[verifiedSymbol] = true;
    input[Symbol.for('seen-relays')] = new Set(['wss://untrusted.example.org']);
    const result = admitPublicEvent(createPublicIngress(), input);
    expect(result.status).toBe('accepted');
    if (result.status !== 'accepted') return;
    input.content = 'later mutation';
    const snapshot = verifiedEnvelopeSnapshot(result.value)!;
    expect(snapshot).toEqual(JSON.parse(wire));
    expect(Object.getOwnPropertySymbols(snapshot)).toEqual([]);
    snapshot.content = 'changed detached snapshot';
    expect(verifiedEnvelopeSnapshot(result.value)).toEqual(JSON.parse(wire));
  });
  it.each([NaN, Infinity, -Infinity, -0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1])(
    'rejects unsupported decoded timestamp %s',
    (created_at) => {
      expect(
        admitPublicEvent(createPublicIngress(), { ...candidate(), created_at })
          .status
      ).toBe('rejected');
    }
  );
  it('retains numeric lexical checks when actual raw JSON is available', () => {
    const altered = wire.replace(
      /"created_at":\s*[0-9]+/u,
      '"created_at":1700000060.0'
    );
    expect(altered).not.toBe(wire);
    expect(admitPublicEvent(createPublicIngress(), altered).status).toBe(
      'rejected'
    );
  });
  it('never calls own field getters, toJSON or untrusted tag iterators', () => {
    let calls = 0;
    const input = candidate();
    Object.defineProperty(input, 'content', {
      get() {
        calls++;
        return 'untrusted';
      }
    });
    expect(admitPublicEvent(createPublicIngress(), input).status).toBe(
      'rejected'
    );
    const safe = candidate();
    safe.toJSON = () => {
      calls++;
      throw new Error('untrusted');
    };
    const tags = safe.tags as string[][];
    Object.defineProperty(tags, Symbol.iterator, {
      value() {
        calls++;
        throw new Error('untrusted');
      }
    });
    expect(admitPublicEvent(createPublicIngress(), safe).status).toBe(
      'accepted'
    );
    expect(calls).toBe(0);
  });
  it('charges repeated valid candidates before deduplication', () => {
    const owner = createPublicIngress();
    expect(admitPublicEvent(owner, wire).status).toBe('accepted');
    expect(admitPublicEvent(owner, wire).status).toBe('duplicate');
    expect(publicIngressStats(owner)).toEqual({
      deliveries: 2,
      chargedBytes: new TextEncoder().encode(wire).length * 2,
      stopped: false
    });
  });
  it('stops a rejected-input storm at the inclusive delivery limit', () => {
    const owner = createPublicIngress();
    for (let index = 0; index < PUBLIC_INGRESS_BUDGETS.deliveries; index++)
      expect(admitPublicEvent(owner, 'null').status).toBe('rejected');
    const before = publicIngressStats(owner);
    expect(before).toEqual({
      deliveries: 2000,
      chargedBytes: 8000,
      stopped: true
    });
    expect(admitPublicEvent(owner, wire).status).toBe('limit');
    expect(publicIngressStats(owner)).toEqual(before);
  });
  it('stops an oversized stream at the aggregate byte cap without admitting its payload', () => {
    const owner = createPublicIngress();
    const oversized = ' '.repeat(PUBLIC_INGRESS_BUDGETS.eventBytes + 1);
    for (let index = 0; index < 32; index++) {
      const outcome = admitPublicEvent(owner, oversized);
      expect(outcome.status).toBe(index === 31 ? 'limit' : 'rejected');
    }
    expect(publicIngressStats(owner)).toEqual({
      deliveries: 32,
      chargedBytes: 8388608,
      stopped: true
    });
    expect(admitPublicEvent(owner, wire).status).toBe('limit');
  });
  it('counts UTF8 expansion and malformed input against one shared owner', () => {
    const owner = createPublicIngress();
    const rejected = JSON.stringify({
      ...candidate(),
      content: 'é'.repeat(1000)
    });
    expect(admitPublicEvent(owner, rejected).status).toBe('rejected');
    expect(publicIngressStats(owner).chargedBytes).toBe(
      new TextEncoder().encode(rejected).length
    );
    expect(admitPublicEvent(owner, wire).status).toBe('accepted');
    expect(publicIngressStats(owner).deliveries).toBe(2);
  });
  it('refuses forged owners and provides detached immutable accounting', () => {
    expect(() => admitPublicEvent({} as PublicIngress, wire)).toThrow(
      'public_ingress_invalid'
    );
    const owner = createPublicIngress();
    const before = publicIngressStats(owner);
    expect(Reflect.set(before, 'deliveries', 99)).toBe(false);
    admitPublicEvent(owner, 'null');
    expect(before.deliveries).toBe(0);
    expect(publicIngressStats(owner).deliveries).toBe(1);
  });
  it('stops at exact aggregate byte equality before inspecting another candidate', () => {
    const owner = createPublicIngress();
    const raw = ' '.repeat(PUBLIC_INGRESS_BUDGETS.eventBytes);
    for (let index = 0; index < 32; index++)
      expect(admitPublicEvent(owner, raw).status).toBe('rejected');
    const before = publicIngressStats(owner);
    expect(before).toEqual({
      deliveries: 32,
      chargedBytes: 8388608,
      stopped: true
    });
    let inspected = 0;
    const next = new Proxy(candidate(), {
      getOwnPropertyDescriptor(target, key) {
        inspected++;
        return Reflect.getOwnPropertyDescriptor(target, key);
      }
    });
    expect(admitPublicEvent(owner, next).status).toBe('limit');
    expect(inspected).toBe(0);
    expect(publicIngressStats(owner)).toEqual(before);
  });
  it('charges and stops reentrant descriptor admission before nested reconstruction', () => {
    const owner = createPublicIngress();
    let entered = false;
    let descriptors = 0;
    let nested: ReturnType<typeof admitPublicEvent> | undefined;
    const input = new Proxy(candidate(), {
      getOwnPropertyDescriptor(target, key) {
        descriptors++;
        if (!entered) {
          entered = true;
          nested = admitPublicEvent(owner, 'null');
        }
        return Reflect.getOwnPropertyDescriptor(target, key);
      }
    });
    const outer = admitPublicEvent(owner, input);
    expect(nested?.status).toBe('limit');
    expect(outer.status).toBe('limit');
    expect(descriptors).toBe(1);
    const before = publicIngressStats(owner);
    expect(before.deliveries).toBe(2);
    expect(before.chargedBytes).toBeGreaterThan(0);
    expect(before.chargedBytes).toBeLessThanOrEqual(8388608);
    expect(before.stopped).toBe(true);
    expect(admitPublicEvent(owner, wire).status).toBe('limit');
    expect(publicIngressStats(owner)).toEqual(before);
  });

  it('allows the inclusive final valid event and then stops at exact bytes', () => {
    const owner = createPublicIngress();
    const raw = ' '.repeat(PUBLIC_INGRESS_BUDGETS.eventBytes);
    for (let index = 0; index < 31; index++)
      expect(admitPublicEvent(owner, raw).status).toBe('rejected');
    const final =
      wire +
      ' '.repeat(
        PUBLIC_INGRESS_BUDGETS.eventBytes -
          new TextEncoder().encode(wire).length
      );
    expect(admitPublicEvent(owner, final).status).toBe('accepted');
    expect(publicIngressStats(owner)).toEqual({
      deliveries: 32,
      chargedBytes: 8388608,
      stopped: true
    });
  });

  it('isolates revoked object reflection and preserves the next valid candidate', () => {
    const owner = createPublicIngress();
    const revoked = Proxy.revocable(candidate(), {});
    revoked.revoke();
    expect(admitPublicEvent(owner, revoked.proxy).status).toBe('rejected');
    expect(admitPublicEvent(owner, wire).status).toBe('accepted');
    expect(publicIngressStats(owner).deliveries).toBe(2);
  });
});
