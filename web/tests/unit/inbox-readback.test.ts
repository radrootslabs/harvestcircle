import { it, expect } from 'vitest';
import { makeFixture } from '../e2e/harness/approved-signing.ts';
import { fixturePolicy, discovery } from '../e2e/harness/inbox-setup.ts';
import {
  createPublicScheduler,
  createPublicRun,
  openInboxRequest,
  openPublicRequest,
  closePublicScheduler
} from '../../src/lib/nostr/request-scope.ts';
it('forwards the actual canonical admitted inbox source with the verified proof', () => {
  const f = makeFixture(10050),
    scheduler = createPublicScheduler({
      now: () => 0,
      schedule: () => () => {}
    });
  const seen: (string | undefined)[] = [];
  try {
    const run = createPublicRun(scheduler, fixturePolicy());
    openInboxRequest(
      run,
      f.owner,
      (sink) => {
        sink({
          type: 'EVENT',
          from: discovery + '/',
          id: 'owned',
          event: f.sign(f.template)
        });
        sink({ type: 'EOSE', from: discovery + '/', id: 'owned' });
        return () => {};
      },
      (_proof, source?: string) => {
        seen.push(source);
      }
    );
    expect(seen).toEqual([discovery]);
  } finally {
    closePublicScheduler(scheduler);
    f.close();
  }
});
it('unknown inbox sources emit no proof and ordinary public callbacks retain their original arity', () => {
  const f = makeFixture(10050),
    publicFixture = makeFixture(30402),
    scheduler = createPublicScheduler({
      now: () => 0,
      schedule: () => () => {}
    });
  const inbox: unknown[] = [],
    publicArguments: number[] = [];
  try {
    openInboxRequest(
      createPublicRun(scheduler, fixturePolicy()),
      f.owner,
      (sink) => {
        sink({
          type: 'EVENT',
          from: 'wss://foreign.example.org',
          id: 'owned',
          event: f.sign(f.template)
        });
        return () => {};
      },
      (...args) => {
        inbox.push(args);
      }
    );
    openPublicRequest(
      createPublicRun(scheduler, fixturePolicy()),
      'head',
      (sink) => {
        sink({
          type: 'EVENT',
          from: discovery,
          id: 'owned',
          event: publicFixture.sign(publicFixture.template)
        });
        return () => {};
      },
      (...args) => {
        publicArguments.push(args.length);
      }
    );
    expect(inbox).toEqual([]);
    expect(publicArguments).toEqual([1]);
  } finally {
    closePublicScheduler(scheduler);
    f.close();
    publicFixture.close();
  }
});

import {
  decodePublicRecord,
  publicRecordSnapshot,
  publicRecordWire
} from '../../src/lib/persistence/records.ts';
import {
  preparePublicReceiptTransition,
  publicTransitionSnapshot
} from '../../src/lib/persistence/artifact-records.ts';
function receiptFixture(kind: 10050 | 30402 = 10050) {
  const f = makeFixture(kind);
  const row = publicRecordSnapshot(f.record, f.owner, f.id);
  if (!row || row.family === 'public_draft')
    throw new Error('missing operation');
  const event = f.sign(f.template),
    wire = JSON.stringify(event);
  const fact = {
    actionId: crypto.randomUUID(),
    origin: row.capture.targets[0],
    role: kind === 10050 ? 'preference' : 'publication',
    attempt: 1,
    eventId: event.id,
    status: 'accepted',
    observedAtMilliseconds: 102,
    readbackWire: wire
  };
  const record = {
    ...row,
    revision: 2,
    artifact: { eventId: event.id, wire },
    receipts: []
  };
  return { ...f, record, fact, wire };
}
it('preference readback names its actual discovery origin without changing the acknowledged write target', () => {
  const f = receiptFixture();
  try {
    const row = {
      ...f.record,
      receipts: [{ ...f.fact, readbackOrigin: 'wss://discovery.example.org' }]
    };
    const raw = JSON.stringify(row),
      decoded = decodePublicRecord(raw, f.owner, f.id);
    expect(decoded.ok).toBe(true);
    if (!decoded.ok) throw new Error(decoded.reason);
    expect(publicRecordWire(decoded.value, f.owner, f.id)).toBe(raw);
  } finally {
    f.close();
  }
});
it('legacy ACK and legacy readback receipts roundtrip unchanged without an invented discovery source', () => {
  const f = receiptFixture();
  try {
    for (const readbackWire of [null, f.wire]) {
      const raw = JSON.stringify({
        ...f.record,
        receipts: [{ ...f.fact, readbackWire }]
      });
      const decoded = decodePublicRecord(raw, f.owner, f.id);
      expect(decoded.ok).toBe(true);
      if (!decoded.ok) throw new Error(decoded.reason);
      expect(publicRecordWire(decoded.value, f.owner, f.id)).toBe(raw);
    }
  } finally {
    f.close();
  }
});
it('readback source rejects null wire, noncanonical origins, arbitrary fields, invalid signatures and ordinary publication rows', () => {
  const f = receiptFixture(),
    publicFixture = receiptFixture(30402);
  try {
    for (const fact of [
      {
        ...f.fact,
        readbackOrigin: 'wss://discovery.example.org',
        readbackWire: null
      },
      { ...f.fact, readbackOrigin: 'wss://discovery.example.org/' },
      { ...f.fact, readbackOrigin: 'ws://127.0.0.1' },
      { ...f.fact, readbackOrigin: 'wss://discovery.example.org', extra: true },
      {
        ...f.fact,
        readbackOrigin: 'wss://discovery.example.org',
        readbackWire: JSON.stringify({
          ...JSON.parse(f.wire),
          content: 'forged'
        })
      }
    ])
      expect(
        decodePublicRecord(
          JSON.stringify({ ...f.record, receipts: [fact] }),
          f.owner,
          f.id
        ).ok
      ).toBe(false);
    expect(
      decodePublicRecord(
        JSON.stringify({
          ...publicFixture.record,
          receipts: [
            {
              ...publicFixture.fact,
              readbackOrigin: 'wss://discovery.example.org'
            }
          ]
        }),
        publicFixture.owner,
        publicFixture.id
      ).ok
    ).toBe(false);
  } finally {
    f.close();
    publicFixture.close();
  }
});
it('distinct discovery sources remain separate typed receipt facts within the same readback action', () => {
  const f = receiptFixture();
  try {
    const decoded = decodePublicRecord(JSON.stringify(f.record), f.owner, f.id);
    if (!decoded.ok) throw new Error(decoded.reason);
    const first = preparePublicReceiptTransition(
      decoded.value,
      f.owner,
      f.id,
      JSON.stringify({
        ...f.fact,
        readbackOrigin: 'wss://discovery.example.org'
      })
    );
    expect(first.ok).toBe(true);
    if (!first.ok) throw new Error(first.reason);
    const transition = publicTransitionSnapshot(first.value, f.owner, f.id);
    const next = decodePublicRecord(transition?.nextWire, f.owner, f.id);
    if (!next.ok) throw new Error(next.reason);
    const second = preparePublicReceiptTransition(
      next.value,
      f.owner,
      f.id,
      JSON.stringify({ ...f.fact, readbackOrigin: 'wss://second.example.org' })
    );
    expect(second.ok).toBe(true);
    if (!second.ok) throw new Error(second.reason);
    const settled = publicTransitionSnapshot(second.value, f.owner, f.id);
    const both = decodePublicRecord(settled?.nextWire, f.owner, f.id);
    if (!both.ok) throw new Error(both.reason);
    const row = publicRecordSnapshot(both.value, f.owner, f.id);
    expect(row && row.family !== 'public_draft' && row.receipts).toHaveLength(
      2
    );
  } finally {
    f.close();
  }
});
