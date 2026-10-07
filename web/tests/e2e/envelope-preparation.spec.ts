import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Pair from './harness/envelope-preparation.ts';
declare global {
  interface Window {
    hcp078: typeof Pair;
    hcp078Fixture: Awaited<ReturnType<typeof Pair.makeFixture>>;
    hcp078Pair: NonNullable<ReturnType<typeof Pair.captureEnvelopePreparation>>;
    hcp078Job: ReturnType<typeof Pair.prepareEnvelopeRole>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/envelope-preparation.ts', import.meta.url)
        ),
        name: 'hcp078',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('missing bundle');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => server.close());
async function load(page: Page) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    const s = window.hcp078,
      f = await s.makeFixture();
    window.hcp078Fixture = f;
    const pair = s.captureEnvelopePreparation(
      f.identity,
      f.reserved,
      'reviewed_envelope_pair'
    );
    if (!pair) throw Error('missing pair');
    window.hcp078Pair = pair;
  });
}
test('self first and peer second decrypt to the same original, wrong routes and repeats spend no prompts', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const s = window.hcp078,
      f = window.hcp078Fixture,
      p = window.hcp078Pair;
    try {
      const before = f.counts();
      const early = await s.prepareEnvelopeRole(
          p,
          'peer',
          'reviewed_pair_role'
        ),
        wrong = await s.prepareEnvelopeRole(p, 'third', 'reviewed_pair_role'),
        review = await s.prepareEnvelopeRole(p, 'self', 'wrong');
      const blockedUnchanged =
        JSON.stringify(before) === JSON.stringify(f.counts());
      const first = await s.prepareEnvelopeRole(
          p,
          'self',
          'reviewed_pair_role'
        ),
        prefix = s.envelopePreparationSnapshot(p);
      if (!prefix?.self) throw Error('missing prefix');
      const afterSelf = f.counts();
      const second = await s.prepareEnvelopeRole(
          p,
          'peer',
          'reviewed_pair_role'
        ),
        complete = s.envelopePreparationSnapshot(p);
      if (!complete?.self || !complete.peer) throw Error('missing pair');
      const after = f.counts();
      const repeatedSelf = await s.prepareEnvelopeRole(
          p,
          'self',
          'reviewed_pair_role'
        ),
        repeatedPeer = await s.prepareEnvelopeRole(
          p,
          'peer',
          'reviewed_pair_role'
        );
      const selfSeal = f.decryptWrap(complete.self.wire, 'self'),
        peerSeal = f.decryptWrap(complete.peer.wire, 'peer');
      const selfOuter = JSON.parse(complete.self.wire) as {
          id: string;
          tags: string[][];
        },
        peerOuter = JSON.parse(complete.peer.wire) as {
          id: string;
          tags: string[][];
        };
      return {
        early: early.status,
        wrong: wrong.status,
        review: review.status,
        blockedUnchanged,
        first: first.status,
        phase: prefix.phase,
        selfSigns: afterSelf.signs - before.signs,
        selfEncrypts: afterSelf.encrypts - before.encrypts,
        second: second.status,
        complete: complete.phase,
        samePrefix: prefix.self.wire === complete.self.wire,
        sameRumor: complete.self.rumorHash === complete.peer.rumorHash,
        selfEqual: f.decrypt(selfSeal, 'self').equal,
        peerEqual: f.decrypt(peerSeal, 'peer').equal,
        selfTags: selfOuter.tags,
        peerTags: peerOuter.tags,
        owner: f.owner,
        peer: f.peer,
        differentOuter: selfOuter.id !== peerOuter.id,
        repeatedSelf: repeatedSelf.status,
        repeatedPeer: repeatedPeer.status,
        repeatedUnchanged: JSON.stringify(after) === JSON.stringify(f.counts())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toMatchObject({
    early: 'out_of_order',
    wrong: 'invalid',
    review: 'invalid',
    blockedUnchanged: true,
    first: 'prepared',
    phase: 'peer',
    selfSigns: 1,
    selfEncrypts: 1,
    second: 'complete',
    complete: 'complete',
    samePrefix: true,
    sameRumor: true,
    selfEqual: true,
    peerEqual: true,
    differentOuter: true,
    repeatedSelf: 'complete',
    repeatedPeer: 'complete',
    repeatedUnchanged: true
  });
  expect(result.selfTags).toEqual([['p', result.owner]]);
  expect(result.peerTags).toEqual([['p', result.peer]]);
});
test('peer refusal preserves the exact self proof and only explicit retry resumes the original pair', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const s = window.hcp078,
      f = window.hcp078Fixture,
      p = window.hcp078Pair;
    try {
      await s.prepareEnvelopeRole(p, 'self', 'reviewed_pair_role');
      const prefix = s.envelopePreparationSnapshot(p),
        proof = s.preparedEnvelopeProof(p, 'self');
      f.mode('declined');
      const failed = await s.prepareEnvelopeRole(
          p,
          'peer',
          'reviewed_pair_role'
        ),
        paused = s.envelopePreparationSnapshot(p),
        counts = f.counts();
      f.mode('normal');
      const retried = await s.prepareEnvelopeRole(
          p,
          'peer',
          'reviewed_pair_role'
        ),
        done = s.envelopePreparationSnapshot(p);
      if (!done?.self || !done.peer) throw Error('missing completed pair');
      return {
        failed: failed.status,
        phase: paused?.phase,
        busy: paused?.busy,
        unchanged:
          prefix?.self?.wire === paused?.self?.wire &&
          paused?.self?.wire === done.self.wire,
        sameProof: proof === s.preparedEnvelopeProof(p, 'self'),
        retry: retried.status,
        retrySigns: f.counts().signs - counts.signs,
        retryEncrypts: f.counts().encrypts - counts.encrypts,
        selfEqual: f.decrypt(f.decryptWrap(done.self.wire, 'self'), 'self')
          .equal,
        peerEqual: f.decrypt(f.decryptWrap(done.peer.wire, 'peer'), 'peer')
          .equal
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    failed: 'refused',
    phase: 'peer',
    busy: false,
    unchanged: true,
    sameProof: true,
    retry: 'complete',
    retrySigns: 1,
    retryEncrypts: 1,
    selfEqual: true,
    peerEqual: true
  });
});
for (const action of ['stop', 'expire'] as const)
  test(`${action} pending peer keeps admission and real lock until settlement and preserves completed self`, async ({
    page
  }) => {
    await load(page);
    const prefix = await page.evaluate(async () => {
      const s = window.hcp078,
        f = window.hcp078Fixture,
        p = window.hcp078Pair;
      await s.prepareEnvelopeRole(p, 'self', 'reviewed_pair_role');
      const wire = s.envelopePreparationSnapshot(p)?.self?.wire;
      f.mode('hold_decrypt');
      window.hcp078Job = s.prepareEnvelopeRole(p, 'peer', 'reviewed_pair_role');
      return wire;
    });
    await expect
      .poll(() => page.evaluate(() => window.hcp078Fixture.slot()))
      .toMatchObject({ state: 'active', pending: 'decrypt' });
    const pending = await page.evaluate(async (action) => {
      const s = window.hcp078,
        f = window.hcp078Fixture,
        p = window.hcp078Pair;
      if (action === 'expire') s.expireEnvelopePreparationWait(p);
      else s.stopEnvelopePreparation(p);
      const contender = f.operation();
      if (!contender) throw Error('missing contender');
      return {
        busy: s.envelopePreparationSnapshot(p)?.busy,
        pairBusy: (await s.prepareEnvelopeRole(p, 'peer', 'reviewed_pair_role'))
          .status,
        lockBusy: (await s.buildPrivateSeal(contender)).status,
        slot: f.slot(),
        wire: s.envelopePreparationSnapshot(p)?.self?.wire
      };
    }, action);
    expect(pending).toMatchObject({
      busy: true,
      pairBusy: 'busy',
      lockBusy: 'busy',
      slot: { state: 'active', pending: 'decrypt' },
      wire: prefix
    });
    const settled = await page.evaluate(async () => {
      const s = window.hcp078,
        f = window.hcp078Fixture,
        p = window.hcp078Pair,
        before = f.counts();
      try {
        f.settle();
        const stopped = await window.hcp078Job,
          paused = s.envelopePreparationSnapshot(p),
          noLate = JSON.stringify(before) === JSON.stringify(f.counts());
        f.mode('normal');
        const result = await s.prepareEnvelopeRole(
            p,
            'peer',
            'reviewed_pair_role'
          ),
          done = s.envelopePreparationSnapshot(p);
        return {
          status: stopped.status,
          phase: paused?.phase,
          busy: paused?.busy,
          wire: paused?.self?.wire,
          noLate,
          slot: f.slot(),
          resumed: result.status,
          sameSelf: done?.self?.wire === paused?.self?.wire
        };
      } finally {
        f.close();
      }
    });
    expect(settled).toEqual({
      status: 'stopped',
      phase: 'peer',
      busy: false,
      wire: prefix,
      noLate: true,
      slot: { state: 'idle' },
      resumed: 'complete',
      sameSelf: true
    });
  });
for (const action of ['close', 'disconnect'] as const)
  test(`${action} retires prefix and pending peer permanently without releasing unsettled work early`, async ({
    page
  }) => {
    await load(page);
    await page.evaluate(async () => {
      const s = window.hcp078,
        f = window.hcp078Fixture,
        p = window.hcp078Pair;
      await s.prepareEnvelopeRole(p, 'self', 'reviewed_pair_role');
      f.mode('hold_decrypt');
      window.hcp078Job = s.prepareEnvelopeRole(p, 'peer', 'reviewed_pair_role');
    });
    await expect
      .poll(() => page.evaluate(() => window.hcp078Fixture.slot()))
      .toMatchObject({ state: 'active', pending: 'decrypt' });
    const pending = await page.evaluate((action) => {
      const s = window.hcp078,
        f = window.hcp078Fixture,
        p = window.hcp078Pair;
      if (action === 'close') {
        s.closeEnvelopePreparation(p);
        s.closeEnvelopePreparation(p);
      } else f.disconnect();
      return {
        retired: !s.envelopePreparationSnapshot(p),
        proofGone: !s.preparedEnvelopeProof(p, 'self'),
        slot: f.slot()
      };
    }, action);
    expect(pending).toEqual({
      retired: true,
      proofGone: true,
      slot: { state: 'active', pending: 'decrypt', phase: 'stopped' }
    });
    const settled = await page.evaluate(async (action) => {
      const s = window.hcp078,
        f = window.hcp078Fixture,
        p = window.hcp078Pair,
        before = f.counts();
      try {
        f.settle();
        const result = await window.hcp078Job,
          noLate = JSON.stringify(before) === JSON.stringify(f.counts());
        f.mode('normal');
        if (action === 'disconnect') {
          await s.connectIdentity(f.identity);
          await s.probeIdentityMessaging(f.identity, 'reviewed_self_copy');
        }
        return {
          status: result.status,
          noLate,
          slot: f.slot(),
          oldRetired: !s.envelopePreparationSnapshot(p),
          retry: (await s.prepareEnvelopeRole(p, 'peer', 'reviewed_pair_role'))
            .status
        };
      } finally {
        f.close();
      }
    }, action);
    expect(settled).toEqual({
      status: 'stopped',
      noLate: true,
      slot: { state: 'idle' },
      oldRetired: true,
      retry: 'stopped'
    });
  });
test('detached snapshots and cast reservations cannot become another preparation controller', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const s = window.hcp078,
      f = window.hcp078Fixture,
      p = window.hcp078Pair;
    try {
      await s.prepareEnvelopeRole(p, 'self', 'reviewed_pair_role');
      const observed = s.envelopePreparationSnapshot(p);
      if (!observed) throw Error('missing snapshot');
      const cast = observed as unknown as typeof p,
        second = await f.reserveAgain();
      const other = s.captureEnvelopePreparation(
        f.identity,
        second,
        'reviewed_envelope_pair'
      );
      return {
        cast: (await s.prepareEnvelopeRole(cast, 'peer', 'reviewed_pair_role'))
          .status,
        detached: !s.captureEnvelopePreparation(
          f.identity,
          {} as typeof f.reserved,
          'reviewed_envelope_pair'
        ),
        sameReservation: second === f.reserved,
        newPair: !!other,
        newPairHasNoPrefix: !!other && !s.preparedEnvelopeProof(other, 'self'),
        originalPrefix: !!s.preparedEnvelopeProof(p, 'self')
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    cast: 'invalid',
    detached: true,
    sameReservation: false,
    newPair: true,
    newPairHasNoPrefix: true,
    originalPrefix: true
  });
});
