import { it, expect } from 'vitest';
import {
  makeSetupFixture,
  fixturePolicy,
  setupInput,
  inbox,
  discovery,
  reviewInboxSetup,
  inboxSetupPreview,
  captureInboxSetup,
  publicRecordSnapshot
} from '../e2e/harness/inbox-setup.ts';
import { createIdentitySession } from '../../src/lib/runtime/identity-session.ts';
import type { InboxSetupReview } from '../../src/lib/messaging/inbox-setup.ts';
async function reviewed(
  f: Awaited<ReturnType<typeof makeSetupFixture>>,
  input = setupInput()
) {
  const r = await reviewInboxSetup(
    f.identity,
    f.own.resolver,
    fixturePolicy(),
    input,
    () => 101
  );
  if (r.status !== 'review') throw new Error(r.reason);
  return r.review;
}
it('Connect alone creates no preference; exact preview captures preserved duplicate and unknown entries', async () => {
  const f = await makeSetupFixture();
  try {
    const before = f.counts(),
      review = await reviewed(f),
      view = inboxSetupPreview(review)!;
    expect(view.author).toBe(f.owner);
    expect(view.destinations).toEqual([discovery]);
    expect(view.selectedInboxes).toEqual([inbox]);
    expect((JSON.parse(view.wire) as { tags: string[][] }).tags).toEqual([
      ...f.event.tags,
      ['relay', inbox]
    ]);
    expect((JSON.parse(view.wire) as { content: string }).content).toBe(
      f.event.content
    );
    expect(view.globalEffect).toContain('other clients');
    expect(f.counts()).toEqual(before);
    const absent = captureInboxSetup(
      review,
      f.own.resolver,
      crypto.randomUUID(),
      'connect'
    );
    expect(absent.status).toBe('invalid');
    const id = crypto.randomUUID(),
      capture = captureInboxSetup(
        review,
        f.own.resolver,
        id,
        'reviewed_global_inbox_replacement'
      );
    expect(capture.status).toBe('captured');
    if (capture.status !== 'captured') return;
    const row = publicRecordSnapshot(capture.record, f.owner, id)!;
    expect(row.family).toBe('preference_operation');
    if (row.family !== 'preference_operation') return;
    expect(row.source.wire).toBe(view.originalWire);
    expect(row.capture.wire).toBe(view.wire);
    expect(row.capture.hash).toBe(view.hash);
    expect(row.capture.targets).toEqual(view.destinations);
    expect(row.consent).toBe('explicit_review');
    expect(row.artifact).toBeNull();
    expect(row.receipts).toEqual([]);
    expect(f.counts()).toEqual(before);
  } finally {
    f.close();
  }
});
it('more than three existing advertisements are never silently truncated', async () => {
  const tags = [1, 2, 3, 4, 5].map((n) => [
    'relay',
    'wss://old' + n + '.example.org'
  ]);
  const f = await makeSetupFixture(tags);
  try {
    const view = inboxSetupPreview(await reviewed(f))!;
    expect((JSON.parse(view.wire) as { tags: string[][] }).tags).toEqual([
      ...tags,
      ['relay', inbox]
    ]);
  } finally {
    f.close();
  }
});
it('unknown root fields block canonical replacement unless owner explicitly removes each field', async () => {
  const f = await makeSetupFixture(undefined, {
    unknown: { nested: ['keep'] },
    another: 'keep'
  });
  try {
    expect(
      (
        await reviewInboxSetup(
          f.identity,
          f.own.resolver,
          fixturePolicy(),
          setupInput(),
          () => 101
        )
      ).status
    ).toBe('blocked');
    expect(
      (
        await reviewInboxSetup(
          f.identity,
          f.own.resolver,
          fixturePolicy(),
          setupInput({ removeExtraFields: ['unknown'] }),
          () => 101
        )
      ).status
    ).toBe('blocked');
    const view = inboxSetupPreview(
      await reviewed(
        f,
        setupInput({ removeExtraFields: ['unknown', 'another'] })
      )
    )!;
    expect(
      (JSON.parse(view.originalWire!) as Record<string, unknown>).unknown
    ).toEqual({
      nested: ['keep']
    });
    expect(view.removedExtraFields).toEqual(['unknown', 'another']);
    expect(JSON.parse(view.wire)).not.toHaveProperty('unknown');
  } finally {
    f.close();
  }
});
it('malformed relay entries require explicit removal and retain every unrelated unknown tag', async () => {
  const tags = [
    ['relay', 'https://bad.example.org'],
    ['unknown', 'keep'],
    ['relay', 'wss://old.example.org']
  ];
  const f = await makeSetupFixture(tags);
  try {
    expect(
      (
        await reviewInboxSetup(
          f.identity,
          f.own.resolver,
          fixturePolicy(),
          setupInput(),
          () => 101
        )
      ).status
    ).toBe('blocked');
    const view = inboxSetupPreview(
      await reviewed(f, setupInput({ removeTagIndices: [0] }))
    )!;
    expect(view.removedTagIndices).toEqual([0]);
    expect((JSON.parse(view.wire) as { tags: string[][] }).tags).toEqual([
      tags[1],
      tags[2],
      ['relay', inbox]
    ]);
  } finally {
    f.close();
  }
});
it('known-head change conflicts rather than authorizing stale global replacement', async () => {
  const f = await makeSetupFixture();
  try {
    const review = await reviewed(f),
      fresh = f.resolve(f.newerEvent);
    expect(
      captureInboxSetup(
        review,
        fresh.resolver,
        crypto.randomUUID(),
        'reviewed_global_inbox_replacement'
      ).status
    ).toBe('conflict');
  } finally {
    f.close();
  }
});
it('missing bounded observation can capture null base but partial discovery cannot', async () => {
  const f = await makeSetupFixture(undefined, {}, true);
  try {
    const review = await reviewed(f),
      id = crypto.randomUUID(),
      r = captureInboxSetup(
        review,
        f.own.resolver,
        id,
        'reviewed_global_inbox_replacement'
      );
    expect(r.status).toBe('captured');
    if (r.status === 'captured') {
      const row = publicRecordSnapshot(r.record, f.owner, id)!;
      if (row.family === 'preference_operation')
        expect(row.source.wire).toBeNull();
    }
    const incomplete = f.resolve(null, false);
    expect(
      (
        await reviewInboxSetup(
          f.identity,
          incomplete.resolver,
          fixturePolicy(),
          setupInput(),
          () => 101
        )
      ).status
    ).toBe('blocked');
  } finally {
    f.close();
  }
});
it('detached preview cannot retarget frozen capture and forged reviews cannot capture', async () => {
  const f = await makeSetupFixture();
  try {
    const review = await reviewed(f),
      view = inboxSetupPreview(review)!;
    view.destinations.push('wss://hint.example.org');
    view.selectedInboxes.length = 0;
    view.removedExtraFields.push('not_selected');
    const id = crypto.randomUUID(),
      r = captureInboxSetup(
        review,
        f.own.resolver,
        id,
        'reviewed_global_inbox_replacement'
      );
    expect(r.status).toBe('captured');
    if (r.status === 'captured') {
      const row = publicRecordSnapshot(r.record, f.owner, id)!;
      if (row.family === 'preference_operation')
        expect(row.capture.targets).toEqual([discovery]);
    }
    expect(
      captureInboxSetup(
        {} as InboxSetupReview,
        f.own.resolver,
        id,
        'reviewed_global_inbox_replacement'
      ).status
    ).toBe('invalid');
  } finally {
    f.close();
  }
});
it('stale owner or resolver, unsupported clock and unapproved target cannot capture', async () => {
  const f = await makeSetupFixture();
  try {
    for (const changes of [
      { createdAt: 100 },
      { createdAt: Number.MAX_SAFE_INTEGER + 1 },
      { selectedInboxes: ['wss://hint.example.org'] },
      { removeTagIndices: [999] },
      { removeExtraFields: ['absent'] }
    ])
      expect(
        (
          await reviewInboxSetup(
            f.identity,
            f.own.resolver,
            fixturePolicy(),
            setupInput(changes),
            () => 101
          )
        ).status
      ).toBe('blocked');
    expect(
      (
        await reviewInboxSetup(
          createIdentitySession(),
          f.own.resolver,
          fixturePolicy(),
          setupInput(),
          () => 101
        )
      ).status
    ).toBe('blocked');
    const review = await reviewed(f);
    f.own.stale();
    expect(
      captureInboxSetup(
        review,
        f.own.resolver,
        crypto.randomUUID(),
        'reviewed_global_inbox_replacement'
      ).status
    ).toBe('unavailable');
  } finally {
    f.close();
  }
});
