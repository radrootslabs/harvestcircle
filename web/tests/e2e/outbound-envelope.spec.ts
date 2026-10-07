import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Layers from './harness/outbound-envelope.ts';
declare global {
  interface Window {
    hcp077: typeof Layers;
    hcp077Fixture: Awaited<ReturnType<typeof Layers.makeFixture>>;
    hcp077Operation: Parameters<typeof Layers.stopPrivateSeal>[0];
    hcp077Job: ReturnType<typeof Layers.buildPrivateSeal>;
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
          new URL('./harness/outbound-envelope.ts', import.meta.url)
        ),
        name: 'hcp077',
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
    window.hcp077Fixture = await window.hcp077.makeFixture();
  });
}
for (const role of ['peer', 'self'] as const)
  test(`genuine ${role} nested proof binds original reservation and expires on disconnect`, async ({
    page
  }) => {
    await load(page);
    const result = await page.evaluate(async (role) => {
      const f = window.hcp077Fixture,
        s = window.hcp077;
      try {
        const op = f.operation(role);
        if (!op) throw Error('missing operation');
        const sealed = await s.buildPrivateSeal(op);
        if (sealed.status !== 'sealed') throw Error('missing seal');
        const wrap = s.buildPrivateGiftwrap(
          sealed.seal,
          'reviewed_private_wrap'
        );
        if (!wrap) throw Error('missing wrap');
        const proof = s.verifyOutboundEnvelope(
          f.reserved,
          wrap,
          'reviewed_outbound_layers'
        );
        const snapshot = proof && s.verifiedOutboundSnapshot(proof);
        const cast = s.verifyOutboundEnvelope(
          f.reserved,
          {} as typeof wrap,
          'reviewed_outbound_layers'
        );
        const wrongReview = s.verifyOutboundEnvelope(f.reserved, wrap, 'wrong');
        const second = await f.reserveAgain();
        const wrongReservation = s.verifyOutboundEnvelope(
          second,
          wrap,
          'reviewed_outbound_layers'
        );
        f.disconnect();
        return {
          admitted: !!snapshot,
          role: snapshot?.role,
          cast: !!cast,
          wrongReview: !!wrongReview,
          differentGenuineReservation: second !== f.reserved,
          wrongReservation: !!wrongReservation,
          expired: proof && !s.verifiedOutboundSnapshot(proof)
        };
      } finally {
        f.close();
      }
    }, role);
    expect(result).toEqual({
      admitted: true,
      role,
      cast: false,
      wrongReview: false,
      differentGenuineReservation: true,
      wrongReservation: false,
      expired: true
    });
  });
for (const mode of ['wrong_plaintext', 'declined_decrypt'] as const)
  test(`actual ${mode} never spends SIGN or grants a sender seal`, async ({
    page
  }) => {
    await load(page);
    const result = await page.evaluate(async (mode) => {
      const f = window.hcp077Fixture;
      try {
        const op = f.operation();
        if (!op) throw Error('missing operation');
        f.mode(mode);
        const before = f.counts(),
          identity = f.identityState();
        const result = await window.hcp077.buildPrivateSeal(op),
          after = f.counts();
        return {
          status: result.status,
          signs: after.signs - before.signs,
          decrypts: after.decrypts - before.decrypts,
          identityPreserved: f.identityState() === identity
        };
      } finally {
        f.close();
      }
    }, mode);
    expect(result.status).toBe(
      mode === 'wrong_plaintext' ? 'mismatch' : 'refused'
    );
    expect(result.signs).toBe(0);
    expect(result.decrypts).toBe(1);
    expect(result.identityPreserved).toBe(true);
  });
for (const action of ['stop', 'expire'] as const)
  test(`${action} during actual decrypt keeps slot and owner lock until settlement without late SIGN`, async ({
    page
  }) => {
    await load(page);
    await page.evaluate(() => {
      const f = window.hcp077Fixture,
        op = f.operation();
      if (!op) throw Error('missing operation');
      f.mode('hold_decrypt');
      window.hcp077Operation = op;
      window.hcp077Job = window.hcp077.buildPrivateSeal(op);
    });
    await expect
      .poll(() => page.evaluate(() => window.hcp077Fixture.slot()))
      .toMatchObject({ state: 'active', pending: 'decrypt' });
    const pending = await page.evaluate(async (action) => {
      const f = window.hcp077Fixture,
        s = window.hcp077,
        identity = f.identityState();
      if (action === 'expire') s.expirePrivateSealWait(window.hcp077Operation);
      else s.stopPrivateSeal(window.hcp077Operation);
      const other = f.operation();
      if (!other) throw Error('missing contender');
      return {
        slot: f.slot(),
        busy: (await s.buildPrivateSeal(other)).status,
        identityPreserved: f.identityState() === identity
      };
    }, action);
    expect(pending.slot).toMatchObject({ state: 'active', pending: 'decrypt' });
    expect(pending.busy).toBe('busy');
    expect(pending.identityPreserved).toBe(true);
    const settled = await page.evaluate(async () => {
      const f = window.hcp077Fixture,
        before = f.counts();
      try {
        f.settle();
        const result = await window.hcp077Job;
        return {
          status: result.status,
          slot: f.slot(),
          unchanged: JSON.stringify(before) === JSON.stringify(f.counts())
        };
      } finally {
        f.close();
      }
    });
    expect(settled).toEqual({
      status: 'stopped',
      slot: { state: 'idle' },
      unchanged: true
    });
  });
