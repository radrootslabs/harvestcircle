import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Sizes from './harness/nested-envelope-sizes.ts';
declare global {
  interface Window {
    hcp079: typeof Sizes;
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
          new URL('./harness/nested-envelope-sizes.ts', import.meta.url)
        ),
        name: 'hcp079',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(built) ? built[0] : built;
  if (!('output' in output)) throw Error('missing bundle');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => server.close());
async function load(page: Page) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
}
for (const shape of [
  'ASCII',
  'BMP',
  'astral',
  'maximum_escaped_rumor'
] as const)
  test(`${shape}: actual genuine pair measures every encoded layer and keeps one original`, async ({
    page
  }) => {
    await load(page);
    const result = await page.evaluate(async (shape) => {
      const s = window.hcp079,
        bytes = (value: string) => new TextEncoder().encode(value).length;
      let fixture = await s.makeFixture();
      try {
        const initial = s.reservedSendRumorWire(fixture.reserved);
        if (!initial) throw Error('missing original');
        const before = fixture.counts();
        let text: string;
        function inspect(text: string) {
          const plan = fixture.textPlan(text);
          if (!plan) return undefined;
          try {
            return s.rumorPlanSnapshot(plan);
          } finally {
            s.stopRumorPlan(plan);
          }
        }
        if (shape === 'maximum_escaped_rumor') {
          let low = 0,
            high = 4096;
          while (low < high) {
            const middle = Math.ceil((low + high) / 2);
            if (inspect('\u0001'.repeat(middle))) low = middle;
            else high = middle - 1;
          }
          text = '\u0001'.repeat(low);
          for (let rest = 0; rest < 6; rest++) {
            if (inspect(text + 'x')) text += 'x';
            else break;
          }
          const top = inspect(text);
          if (!top || bytes(top.wire) !== 8192)
            throw Error('maximum rumor not reached');
        } else {
          const first = JSON.parse(initial) as { content: string };
          const overhead =
              bytes(first.content) - bytes('Private seal sentinel'),
            remaining = 4096 - overhead;
          const width = shape === 'ASCII' ? 1 : shape === 'BMP' ? 2 : 4,
            character = shape === 'ASCII' ? 'x' : shape === 'BMP' ? 'é' : '😀';
          text =
            character.repeat(Math.floor(remaining / width)) +
            'x'.repeat(remaining % width);
        }
        const accepted = inspect(text),
          rejected = inspect(text + 'x');
        if (!accepted) throw Error('missing maximum plan');
        const unchanged =
          JSON.stringify(before) === JSON.stringify(fixture.counts()) &&
          s.reservedSendRumorWire(fixture.reserved) === initial;
        const planning = {
          accepted: true,
          plusOneRejected: !rejected,
          noSDK: unchanged
        };
        fixture.close();
        fixture = await s.makeFixture(text);
        const original = s.reservedSendRumorWire(fixture.reserved);
        if (!original) throw Error('missing new original');
        const rumor = JSON.parse(original) as {
          id: string;
          created_at: number;
          content: string;
          tags: string[][];
        };
        const pair = s.captureEnvelopePreparation(
          fixture.identity,
          fixture.reserved,
          'reviewed_envelope_pair'
        );
        if (!pair) throw Error('missing pair');
        const started = fixture.counts();
        const self = await s.prepareEnvelopeRole(
            pair,
            'self',
            'reviewed_pair_role'
          ),
          peer = await s.prepareEnvelopeRole(
            pair,
            'peer',
            'reviewed_pair_role'
          ),
          complete = s.envelopePreparationSnapshot(pair);
        if (!complete?.self || !complete.peer) throw Error('pair incomplete');
        const rows = [];
        for (const role of ['self', 'peer'] as const) {
          const outer = role === 'self' ? complete.self : complete.peer,
            sealWire = fixture.decryptWrap(outer.wire, role),
            seal = JSON.parse(sealWire) as { content: string },
            wrap = JSON.parse(outer.wire) as {
              content: string;
              tags: string[][];
            };
          rows.push({
            role,
            body_utf8: bytes(rumor.content),
            rumor_utf8: bytes(original),
            seal_ciphertext_utf8: bytes(seal.content),
            signed_seal_utf8: bytes(sealWire),
            outer_ciphertext_utf8: bytes(wrap.content),
            signed_outer_utf8: bytes(outer.wire),
            equal: fixture.decrypt(sealWire, role).equal,
            sameHash: outer.rumorHash === rumor.id,
            target:
              wrap.tags[0]?.[1] ===
              (role === 'self' ? fixture.owner : fixture.peer),
            soleTarget: wrap.tags.length === 1 && wrap.tags[0]?.length === 2
          });
        }
        const counts = fixture.counts();
        return {
          planning,
          self: self.status,
          peer: peer.status,
          originalUnchanged:
            s.reservedSendRumorWire(fixture.reserved) === original,
          signs: counts.signs - started.signs,
          encrypts: counts.encrypts - started.encrypts,
          rows
        };
      } finally {
        fixture.close();
      }
    }, shape);
    expect(result.planning).toEqual({
      accepted: true,
      plusOneRejected: true,
      noSDK: true
    });
    expect(result).toMatchObject({
      self: 'prepared',
      peer: 'complete',
      originalUnchanged: true,
      signs: 2,
      encrypts: 2
    });
    expect(result.rows).toHaveLength(2);
    for (const row of result.rows) {
      expect(row).toMatchObject({
        equal: true,
        sameHash: true,
        target: true,
        soleTarget: true
      });
      expect(row.body_utf8).toBeLessThanOrEqual(4096);
      expect(row.rumor_utf8).toBeLessThanOrEqual(8192);
      expect(row.signed_seal_utf8).toBeLessThanOrEqual(16384);
      expect(row.signed_outer_utf8).toBeLessThanOrEqual(32768);
      if (shape === 'maximum_escaped_rumor') {
        expect(row.rumor_utf8).toBe(8192);
        expect(row.seal_ciphertext_utf8).toBe(11012);
      } else expect(row.body_utf8).toBe(4096);
    }
    console.log(
      JSON.stringify({
        fixture: 'HCP079_ACTUAL_GENUINE_FACTORY_NESTED_MEASUREMENTS',
        shape,
        rows: result.rows
      })
    );
  });
