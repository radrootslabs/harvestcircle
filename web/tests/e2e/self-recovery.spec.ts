import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Recovery from './harness/self-recovery.ts';
declare global {
  interface Window {
    hcp081: typeof Recovery;
    hcp081Fixture: Awaited<ReturnType<typeof Recovery.makeFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const built = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/self-recovery.ts', import.meta.url)
        ),
        name: 'hcp081',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(built) ? built[0] : built;
  if (!('output' in output)) throw Error('missing recovery bundle');
  const chunks = output.output.filter((entry) => entry.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP081_REAL_SELF_IDB_ACKNOWLEDGEMENT',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'Source-only genuine SDK/IDB/WebLocks; synthetic fault boundaries, no actual operator Q'
    })
  );
});
test.afterAll(async () => server.close());
async function load(page: Page) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp081Fixture = await window.hcp081.makeFixture();
  });
}
test('self ciphertext is acknowledged before workflow peer permission, with no plaintext in either store', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp081Fixture,
      s = window.hcp081;
    try {
      const before = s.selfRecoveryPreparationSnapshot(f.preparation),
        base = f.counts();
      const early = await s.selfRecoveryPeerPreparation(
        f.preparation,
        {} as Parameters<typeof s.selfRecoveryPeerPreparation>[1]
      );
      const saved = await s.prepareSelfRecovery(
        f.preparation,
        'reviewed_self_recovery'
      );
      const receipt = s.preparedSelfRecovery(f.preparation),
        ack = receipt && s.selfRecoveryAcknowledgementSnapshot(receipt),
        rows = await f.stored(),
        publicRows = await f.publicDraftRows();
      const storedRows = JSON.parse(rows) as { wire: string }[];
      return {
        before: before?.state,
        early: Boolean(early),
        status: saved.status,
        acknowledged: Boolean(ack),
        copy: ack?.copy,
        peerReady: Boolean(
          receipt &&
          (await s.selfRecoveryPeerPreparation(f.preparation, receipt))
        ),
        plaintext: rows.includes(f.marker) || publicRows.includes(f.marker),
        selfOnly: storedRows[0].wire.includes('"peerArtifact":null'),
        encrypts: f.counts().encrypts - base.encrypts,
        signs: f.counts().signs - base.signs
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    before: 'unsaved',
    early: false,
    status: 'saved',
    acknowledged: true,
    copy: 'Saved encrypted in this browser',
    peerReady: true,
    plaintext: false,
    selfOnly: true,
    encrypts: 1,
    signs: 1
  });
});
test('same-command retry reuses the exact self ciphertext without any additional SDK work', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp081Fixture,
      s = window.hcp081;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      const first = await f.stored(),
        before = f.counts(),
        receipt = s.preparedSelfRecovery(f.preparation);
      const retry = await s.prepareSelfRecovery(
        f.preparation,
        'reviewed_self_recovery'
      );
      return {
        status: retry.status,
        equal: first === (await f.stored()),
        sameReceipt: receipt === s.preparedSelfRecovery(f.preparation),
        counts: JSON.stringify(before) === JSON.stringify(f.counts())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'saved',
    equal: true,
    sameReceipt: true,
    counts: true
  });
});
test('closed storage refuses acknowledgement and peer permission without a plaintext fallback', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp081Fixture,
      s = window.hcp081;
    try {
      f.closeStorage();
      const before = f.counts(),
        result = await s.prepareSelfRecovery(
          f.preparation,
          'reviewed_self_recovery'
        );
      return {
        status: result.status,
        receipt: Boolean(s.preparedSelfRecovery(f.preparation)),
        counts: JSON.stringify(before) === JSON.stringify(f.counts()),
        plaintext: (await f.stored()).includes(f.marker)
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'unavailable',
    receipt: false,
    counts: true,
    plaintext: false
  });
});
test('actual aborted self write never grants local acknowledgement or peer permission', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp081Fixture,
      s = window.hcp081;
    try {
      const prior = await f.stored(),
        hits = f.fault('abort'),
        result = await s.prepareSelfRecovery(
          f.preparation,
          'reviewed_self_recovery'
        );
      return {
        status: result.status,
        hits: hits(),
        receipt: Boolean(s.preparedSelfRecovery(f.preparation)),
        unchanged: prior === (await f.stored())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'aborted',
    hits: 1,
    receipt: false,
    unchanged: true
  });
});
test('unknown write completion is reconciled by exact original ciphertext readback, never by regenerated crypto', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp081Fixture,
      s = window.hcp081;
    try {
      const before = f.counts(),
        hits = f.fault('unknown'),
        result = await s.prepareSelfRecovery(
          f.preparation,
          'reviewed_self_recovery'
        );
      const receipt = s.preparedSelfRecovery(f.preparation),
        ack = receipt && s.selfRecoveryAcknowledgementSnapshot(receipt);
      const rows = JSON.parse(await f.stored()) as { wire: string }[],
        stored = JSON.parse(rows[0].wire) as { self: { wire: string } };
      return {
        status: result.status,
        hits: hits(),
        ack: Boolean(ack),
        exact: Boolean(ack && stored.self.wire === ack.self.wire),
        encrypts: f.counts().encrypts - before.encrypts,
        signs: f.counts().signs - before.signs
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'reconciled',
    hits: 1,
    ack: true,
    exact: true,
    encrypts: 1,
    signs: 1
  });
});
test('late encryption after actual Disconnect grants no receipt and cannot queue peer work', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp081Fixture,
      s = window.hcp081;
    try {
      f.mode('hold_encrypt');
      const pending = s.prepareSelfRecovery(
        f.preparation,
        'reviewed_self_recovery'
      );
      for (let i = 0; i < 1800 && !f.pendingEncryption(); i++)
        await new Promise((resolve) => setTimeout(resolve, 50));
      if (!f.pendingEncryption()) throw Error('encryption did not enter');
      const repeated = await s.prepareSelfRecovery(
        f.preparation,
        'reviewed_self_recovery'
      );
      f.disconnect();
      f.settle();
      const late = await pending;
      const rows = JSON.parse(await f.stored()) as { wire: string }[],
        stored = JSON.parse(rows[0].wire) as { family: string };
      return {
        repeated: repeated.status,
        late: late.status,
        receipt: Boolean(s.preparedSelfRecovery(f.preparation)),
        plaintext: (await f.stored()).includes(f.marker),
        family: stored.family
      };
    } finally {
      f.settle();
      f.close();
    }
  });
  expect(result).toEqual({
    repeated: 'busy',
    late: 'stopped',
    receipt: false,
    plaintext: false,
    family: 'private_send_reservation'
  });
});
test('a competing owner lock refuses preparation without prompts and permits only explicit later retry', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp081Fixture,
      s = window.hcp081;
    let release: (() => Promise<void>) | undefined;
    try {
      release = await f.holdOwner();
      const before = f.counts(),
        refused = await s.prepareSelfRecovery(
          f.preparation,
          'reviewed_self_recovery'
        ),
        unchanged = JSON.stringify(before) === JSON.stringify(f.counts());
      await release();
      release = undefined;
      const later = await s.prepareSelfRecovery(
        f.preparation,
        'reviewed_self_recovery'
      );
      return { refused: refused.status, unchanged, later: later.status };
    } finally {
      await release?.();
      f.close();
    }
  });
  expect(result).toEqual({ refused: 'busy', unchanged: true, later: 'saved' });
});
test('a new controller with existing self evidence requires explicit recovery instead of regenerating a wrapper', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp081Fixture,
      s = window.hcp081;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      const second = s.captureSelfRecoveryPreparation(
        f.repository,
        f.identity,
        f.reserved,
        'reviewed_self_recovery'
      );
      if (!second) throw Error('missing second controller');
      const before = f.counts(),
        bytes = await f.stored(),
        result = await s.prepareSelfRecovery(second, 'reviewed_self_recovery');
      s.stopSelfRecoveryPreparation(second);
      return {
        status: result.status,
        unchanged: JSON.stringify(before) === JSON.stringify(f.counts()),
        bytes: bytes === (await f.stored())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'recovery_required',
    unchanged: true,
    bytes: true
  });
});
test('a genuine peer envelope or foreign namespace cannot acknowledge a self recovery record', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp081Fixture,
      s = window.hcp081;
    try {
      const peer = await f.peerProof(),
        prior = await f.stored();
      const denied = await s.commitSelfRecovery(
        f.repository,
        f.identity,
        f.reserved,
        peer,
        'reviewed_self_commit'
      );
      const self = await f.selfProof();
      const foreign = await s.commitSelfRecovery(
        f.foreignRepository,
        f.identity,
        f.reserved,
        self,
        'reviewed_self_commit'
      );
      return {
        denied: denied.status,
        foreignAcknowledged: 'receipt' in foreign,
        unchanged: prior === (await f.stored()),
        ack: Boolean(s.preparedSelfRecovery(f.preparation))
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    denied: 'invalid',
    foreignAcknowledged: false,
    unchanged: true,
    ack: false
  });
});
test('unrecognized stored evidence is preserved and refuses crypto, reset and local acknowledgement', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp081Fixture,
      s = window.hcp081;
    try {
      await f.corruptReservation();
      const prior = await f.stored(),
        before = f.counts(),
        result = await s.prepareSelfRecovery(
          f.preparation,
          'reviewed_self_recovery'
        );
      return {
        status: result.status,
        unchanged: prior === (await f.stored()),
        counts: JSON.stringify(before) === JSON.stringify(f.counts()),
        receipt: Boolean(s.preparedSelfRecovery(f.preparation))
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'corrupt_record',
    unchanged: true,
    counts: true,
    receipt: false
  });
});
test('ending crypto preparation preserves the acknowledged local fact and revokes workflow peer permission', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp081Fixture,
      s = window.hcp081;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      const receipt = s.preparedSelfRecovery(f.preparation);
      if (!receipt) throw Error('missing actual receipt');
      const pair = await s.selfRecoveryPeerPreparation(f.preparation, receipt);
      if (!pair) throw Error('missing acknowledged permission');
      const before = await f.stored();
      f.closePeerPreparation(pair);
      return {
        savedFact: Boolean(s.selfRecoveryAcknowledgementSnapshot(receipt)),
        workflow: Boolean(s.selfRecoveryPreparationSnapshot(f.preparation)),
        peer: Boolean(
          await s.selfRecoveryPeerPreparation(f.preparation, receipt)
        ),
        unchanged: before === (await f.stored())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    savedFact: true,
    workflow: false,
    peer: false,
    unchanged: true
  });
});
