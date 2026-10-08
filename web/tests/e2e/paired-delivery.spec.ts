import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Pair from './harness/paired-delivery.ts';
declare global {
  interface Window {
    hcp082: typeof Pair;
    hcp082Fixture: Awaited<ReturnType<typeof Pair.makeFixture>>;
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
          new URL('./harness/paired-delivery.ts', import.meta.url)
        ),
        name: 'hcp082',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(built) ? built[0] : built;
  if (!('output' in output)) throw Error('missing pair bundle');
  const chunks = output.output.filter((entry) => entry.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP082_REAL_PAIRED_IDB_PLAN',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'Source-only actual SDK/IDB/WebLocks; controlled public preference observations, no operator Q'
    })
  );
});
test.afterAll(async () => server.close());
async function load(page: Page) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp082Fixture = await window.hcp082.makeFixture();
  });
}

test('no paired permission before self acknowledgement; complete artifacts and frozen routes commit before local readiness', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp082Fixture,
      s = window.hcp082;
    try {
      const before = f.counts(),
        refused = await s.preparePairedDelivery(
          f.preparation,
          f.context,
          'reviewed_pair_preparation'
        ),
        inert = JSON.stringify(before) === JSON.stringify(f.counts());
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      const self = await f.stored(),
        prepared = await s.preparePairedDelivery(
          f.preparation,
          f.context,
          'reviewed_pair_preparation'
        ),
        token = s.preparedPairedDelivery(f.preparation),
        row = token && s.pairedDeliveryAcknowledgementSnapshot(token),
        raw = await f.stored();
      return {
        refused: refused.status,
        inert,
        prepared: prepared.status,
        ack: !!row,
        state: row?.deliveryPlan.state,
        roles: row && [
          row.deliveryPlan.routes.peer.role,
          row.deliveryPlan.routes.archive.role
        ],
        sameRumor:
          row?.rumorHash ===
          s.selfRecoveryAcknowledgementSnapshot(
            s.preparedSelfRecovery(f.preparation)!
          )?.rumorHash,
        twoArtifacts: !!row?.self.wire && !!row.peerArtifact.wire,
        changed: raw !== self,
        plaintext:
          raw.includes(f.marker) ||
          (await f.publicDraftRows()).includes(f.marker)
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    refused: 'self_required',
    inert: true,
    prepared: 'prepared',
    ack: true,
    state: 'prepared',
    roles: ['peer', 'self_archive'],
    sameRumor: true,
    twoArtifacts: true,
    changed: true,
    plaintext: false
  });
});
test('same paired command retry keeps both exact wires and routes without more SDK work', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp082Fixture,
      s = window.hcp082;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      await s.preparePairedDelivery(
        f.preparation,
        f.context,
        'reviewed_pair_preparation'
      );
      const token = s.preparedPairedDelivery(f.preparation),
        before = f.counts(),
        bytes = await f.stored(),
        again = await s.preparePairedDelivery(
          f.preparation,
          f.context,
          'reviewed_pair_preparation'
        );
      return {
        status: again.status,
        same: token === s.preparedPairedDelivery(f.preparation),
        bytes: bytes === (await f.stored()),
        counts: JSON.stringify(before) === JSON.stringify(f.counts())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'prepared',
    same: true,
    bytes: true,
    counts: true
  });
});
test('genuine paired acknowledgement cannot reconcile arbitrary structurally valid stored advancement', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp082Fixture,
      s = window.hcp082;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      await s.preparePairedDelivery(
        f.preparation,
        f.context,
        'reviewed_pair_preparation'
      );
      await f.changePairRevision();
      const changed = await f.stored(),
        before = f.counts(),
        self = await s.prepareSelfRecovery(
          f.preparation,
          'reviewed_self_recovery'
        ),
        pair = await s.preparePairedDelivery(
          f.preparation,
          f.context,
          'reviewed_pair_preparation'
        );
      return {
        self: self.status,
        pair: pair.status,
        selfAck: !!s.preparedSelfRecovery(f.preparation),
        pairAck: !!s.preparedPairedDelivery(f.preparation),
        preserved: changed === (await f.stored()),
        counts: JSON.stringify(before) === JSON.stringify(f.counts())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    self: 'conflict',
    pair: 'self_required',
    selfAck: false,
    pairAck: false,
    preserved: true,
    counts: true
  });
});
for (const fault of ['abort', 'unknown'] as const)
  test(
    'actual ' + fault + ' pair write preserves truthful atomic acknowledgement',
    async ({ page }) => {
      await load(page);
      const result = await page.evaluate(async (mode) => {
        const f = window.hcp082Fixture,
          s = window.hcp082;
        try {
          await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
          const old = await f.stored(),
            hits = f.fault(mode),
            result = await s.preparePairedDelivery(
              f.preparation,
              f.context,
              'reviewed_pair_preparation'
            ),
            receipt = s.preparedPairedDelivery(f.preparation),
            stored = await f.stored();
          return {
            status: result.status,
            hits: hits(),
            ack: !!receipt,
            unchanged: old === stored,
            plaintext: stored.includes(f.marker)
          };
        } finally {
          f.close();
        }
      }, fault);
      expect(result).toEqual(
        fault === 'abort'
          ? {
              status: 'aborted',
              hits: 1,
              ack: false,
              unchanged: true,
              plaintext: false
            }
          : {
              status: 'reconciled',
              hits: 1,
              ack: true,
              unchanged: false,
              plaintext: false
            }
      );
    }
  );
test('peer interruption after actual encryption entry leaves only acknowledged self recovery ciphertext', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp082Fixture,
      s = window.hcp082;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      const bytes = await f.stored();
      f.mode('hold_encrypt');
      const pending = s.preparePairedDelivery(
        f.preparation,
        f.context,
        'reviewed_pair_preparation'
      );
      let entered = false;
      for (let i = 0; i < 1800; i++) {
        if (f.pendingEncryption()) {
          entered = true;
          break;
        }
        await new Promise((resolve) => setTimeout(resolve, 50));
      }
      if (!entered) throw Error('peer encryption did not enter');
      f.disconnect();
      f.settle();
      const result = await pending;
      return {
        status: result.status,
        ack: !!s.preparedPairedDelivery(f.preparation),
        unchanged: bytes === (await f.stored())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({ status: 'stopped', ack: false, unchanged: true });
});
test('changed route ownership after actual peer crypto blocks pair commit and preserves self', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp082Fixture,
      s = window.hcp082;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      const bytes = await f.stored();
      f.mode('hold_encrypt');
      const pending = s.preparePairedDelivery(
        f.preparation,
        f.context,
        'reviewed_pair_preparation'
      );
      let entered = false;
      for (let i = 0; i < 1800; i++) {
        if (f.pendingEncryption()) {
          entered = true;
          break;
        }
        await new Promise((resolve) => setTimeout(resolve, 50));
      }
      if (!entered) throw Error('peer encryption did not enter');
      f.staleRoutes();
      f.settle();
      const result = await pending;
      return {
        status: result.status,
        ack: !!s.preparedPairedDelivery(f.preparation),
        unchanged: bytes === (await f.stored())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'review_required',
    ack: false,
    unchanged: true
  });
});
test('different genuine peer ciphertext for the same committed command conflicts without overwriting either artifact', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp082Fixture,
      s = window.hcp082;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      const self = s.preparedSelfRecovery(f.preparation)!;
      await s.preparePairedDelivery(
        f.preparation,
        f.context,
        'reviewed_pair_preparation'
      );
      const old = await f.stored(),
        peer = await f.peerProof();
      if (!peer) throw Error('missing competing genuine peer proof');
      const changed = await s.commitPairedDelivery(
        f.repository,
        f.identity,
        f.reserved,
        self,
        peer,
        f.context.plan,
        'reviewed_pair_commit'
      );
      return { status: changed.status, unchanged: old === (await f.stored()) };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({ status: 'conflict', unchanged: true });
});
test('stored route metadata rejects unknown plaintext and missing paired artifacts instead of fabricating readiness', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp082Fixture,
      s = window.hcp082;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      await s.preparePairedDelivery(
        f.preparation,
        f.context,
        'reviewed_pair_preparation'
      );
      const token = s.preparedPairedDelivery(f.preparation)!,
        row = s.pairedDeliveryAcknowledgementSnapshot(token)!;
      const raw = {
        schema: 1,
        family: 'private_send_operation',
        owner: row.owner,
        id: row.id,
        revision: row.revision,
        peer: row.peer,
        rumorHash: row.rumorHash,
        createdAt: row.createdAt,
        self: row.self,
        peerArtifact: row.peerArtifact,
        deliveryPlan: row.deliveryPlan
      };
      return {
        valid: s.decodePrivateRecord(JSON.stringify(raw), row.owner, row.id).ok,
        unknown: s.decodePrivateRecord(
          JSON.stringify({
            ...raw,
            deliveryPlan: { ...raw.deliveryPlan, body: f.marker }
          }),
          row.owner,
          row.id
        ).ok,
        missing: s.decodePrivateRecord(
          JSON.stringify({ ...raw, peerArtifact: null }),
          row.owner,
          row.id
        ).ok
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({ valid: true, unknown: false, missing: false });
});
test('distinct intentional identical text is not collapsed into the prior paired command', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp082Fixture,
      s = window.hcp082;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      const self = s.preparedSelfRecovery(f.preparation)!;
      await s.preparePairedDelivery(
        f.preparation,
        f.context,
        'reviewed_pair_preparation'
      );
      const peer = await f.peerProof(),
        intent = await f.distinctIntent();
      if (!peer) throw Error('missing genuine prior peer');
      const before = await f.stored();
      const refused = intent.identity
        ? await s.commitPairedDelivery(
            f.repository,
            f.identity,
            intent.identity,
            self,
            peer,
            f.context.plan,
            'reviewed_pair_commit'
          )
        : undefined;
      return {
        safe:
          intent.status === 'clock_conflict' ||
          (intent.status === 'reserved' &&
            intent.id !== s.selfRecoveryAcknowledgementSnapshot(self)?.id),
        refused: !intent.identity || refused?.status === 'invalid',
        unchanged: before === (await f.stored())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({ safe: true, refused: true, unchanged: true });
});
test('concurrent genuine paired commits from the same self-only base produce one exact durable winner and one conflict', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp082Fixture,
      s = window.hcp082;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      const self = s.preparedSelfRecovery(f.preparation)!,
        left = await f.peerProof(),
        right = await f.peerProof();
      if (!left || !right) throw Error('missing genuine competing peer proofs');
      const a = s.verifiedOutboundSnapshot(left),
        b = s.verifiedOutboundSnapshot(right);
      if (!a || !b || a.wire === b.wire)
        throw Error('competing wrappers were not distinct');
      const outcomes = await Promise.all([
        s.commitPairedDelivery(
          f.repository,
          f.identity,
          f.reserved,
          self,
          left,
          f.context.plan,
          'reviewed_pair_commit'
        ),
        s.commitPairedDelivery(
          f.repository,
          f.identity,
          f.reserved,
          self,
          right,
          f.context.plan,
          'reviewed_pair_commit'
        )
      ]);
      const winner = outcomes.find((value) => 'receipt' in value);
      if (!winner || !('receipt' in winner))
        throw Error('missing actual acknowledged winner');
      const expected = winner === outcomes[0] ? a : b,
        ack = s.pairedDeliveryAcknowledgementSnapshot(winner.receipt),
        stored = await f.storedPair(),
        original = s.selfRecoveryAcknowledgementSnapshot(self),
        wire = await f.stored();
      return {
        statuses: outcomes.map((value) => value.status).sort(),
        oneReceipt: outcomes.filter((value) => 'receipt' in value).length === 1,
        peer:
          stored?.peerArtifact?.wire === expected.wire &&
          ack?.peerArtifact.wire === expected.wire,
        self: stored?.self.wire === original?.self.wire,
        routes:
          JSON.stringify(stored?.deliveryPlan?.routes) ===
          JSON.stringify(s.inboxRoutePlanSnapshot(f.context.plan)),
        revision: stored?.revision === ack?.revision,
        plaintext: wire.includes(f.marker)
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    statuses: ['conflict', 'prepared'],
    oneReceipt: true,
    peer: true,
    self: true,
    routes: true,
    revision: true,
    plaintext: false
  });
});
test('explicit self then pair retry after pair commit retains genuine successor evidence and exact ciphertext', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp082Fixture,
      s = window.hcp082;
    try {
      await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
      const self = s.preparedSelfRecovery(f.preparation);
      await s.preparePairedDelivery(
        f.preparation,
        f.context,
        'reviewed_pair_preparation'
      );
      const token = s.preparedPairedDelivery(f.preparation),
        bytes = await f.stored(),
        before = f.counts(),
        selfAgain = await s.prepareSelfRecovery(
          f.preparation,
          'reviewed_self_recovery'
        ),
        pairAgain = await s.preparePairedDelivery(
          f.preparation,
          f.context,
          'reviewed_pair_preparation'
        );
      return {
        self: selfAgain.status,
        pair: pairAgain.status,
        sameSelf: self === s.preparedSelfRecovery(f.preparation),
        samePair: token === s.preparedPairedDelivery(f.preparation),
        bytes: bytes === (await f.stored()),
        counts: JSON.stringify(before) === JSON.stringify(f.counts())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    self: 'saved',
    pair: 'prepared',
    sameSelf: true,
    samePair: true,
    bytes: true,
    counts: true
  });
});
