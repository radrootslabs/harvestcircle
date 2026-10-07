import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Seals from './harness/seal-builder.ts';
declare global {
  interface Window {
    hcp075: typeof Seals;
    hcp075Fixture: Awaited<ReturnType<typeof Seals.makeFixture>>;
    hcp075Job: ReturnType<typeof Seals.buildPrivateSeal>;
    hcp075Release: () => void;
    hcp075Held: Promise<unknown>;
    hcp075Operation: NonNullable<
      ReturnType<typeof Seals.capturePrivateSealOperation>
    >;
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
          new URL('./harness/seal-builder.ts', import.meta.url)
        ),
        name: 'hcp075',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('missing bundle');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP075_ACTUAL_SDK_CRYPTO_IDB_OWNER_LOCK',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'Ephemeral controlled provider; not installed extension/relay/client Q'
    })
  );
});
test.afterAll(async () => server.close());
async function load(page: Page) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp075Fixture = await window.hcp075.makeFixture();
  });
}
for (const role of ['peer', 'self'] as const)
  test(`actual SDK ${role} ciphertext preserves original rumor; sender signs empty-tag13 only`, async ({
    page
  }) => {
    await load(page);
    const result = await page.evaluate(async (role) => {
      const f = window.hcp075Fixture,
        s = window.hcp075,
        before = f.counts(),
        operation = f.operation(role);
      if (!operation) throw Error('missing operation');
      const outcome = await s.buildPrivateSeal(operation);
      if (outcome.status !== 'sealed') return { status: outcome.status };
      const saved = s.privateSealSnapshot(outcome.seal);
      if (!saved) throw Error('missing seal');
      const decrypted = f.decrypt(saved.wire, role),
        stored = await f.stored(),
        after = f.counts(),
        again = await s.buildPrivateSeal(operation);
      f.close();
      return {
        status: outcome.status,
        equal: decrypted.equal,
        kind: (JSON.parse(saved.wire) as { kind: number }).kind,
        tags: (JSON.parse(saved.wire) as { tags: string[][] }).tags,
        owner: saved.owner,
        target: saved.destination,
        expected: role === 'self' ? f.owner : f.peer,
        seven: Object.keys(JSON.parse(saved.wire) as Record<string, unknown>)
          .length,
        noPlaintext: !stored.includes('sentinel'),
        randomizedBounds:
          (JSON.parse(saved.wire) as { created_at: number }).created_at <=
            Math.floor(Date.now() / 1000) &&
          (JSON.parse(saved.wire) as { created_at: number }).created_at >=
            Math.floor(Date.now() / 1000) - 3599,
        signs: after.signs - before.signs,
        encrypts: after.encrypts - before.encrypts,
        keys: after.keys - before.keys,
        again: again.status
      };
    }, role);
    expect(result).toMatchObject({
      status: 'sealed',
      equal: true,
      kind: 13,
      tags: [],
      seven: 7,
      noPlaintext: true,
      randomizedBounds: true,
      signs: 1,
      encrypts: 1,
      keys: 4,
      again: 'stopped'
    });
    expect(result.target).toBe(result.expected);
  });
for (const mode of [
  'wrong_cipher',
  'wrong_author',
  'wrong_tags',
  'wrong_time',
  'cached_bad_id'
] as const)
  test(`rejects ${mode} from provider without a seal capability`, async ({
    page
  }) => {
    await load(page);
    const result = await page.evaluate(async (mode) => {
      const f = window.hcp075Fixture,
        op = f.operation();
      if (!op) throw Error('missing operation');
      f.mode(mode);
      const outcome = await window.hcp075.buildPrivateSeal(op);
      f.close();
      return outcome.status;
    }, mode);
    expect(result).toBe('mismatch');
  });
for (const mode of ['missing', 'declined', 'changed_key'] as const)
  test(`${mode} pauses without extra signature or retry`, async ({ page }) => {
    await load(page);
    const result = await page.evaluate(async (mode) => {
      const f = window.hcp075Fixture,
        op = f.operation(),
        before = f.counts();
      if (!op) throw Error('missing operation');
      f.mode(mode);
      const result = await window.hcp075.buildPrivateSeal(op),
        after = f.counts();
      f.close();
      return { status: result.status, signs: after.signs - before.signs };
    }, mode);
    expect(['unavailable', 'refused', 'stopped']).toContain(result.status);
    expect(result.signs).toBe(0);
  });
test('detached reservation, wrong role or absent explicit review cannot acquire crypto authority', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(() => {
    const f = window.hcp075Fixture,
      s = window.hcp075,
      before = f.counts();
    const invalid = [
      f.operation('other'),
      f.operation('peer', 'connect'),
      s.capturePrivateSealOperation(
        f.identity,
        {} as typeof f.reserved,
        'peer',
        'reviewed_private_seal'
      )
    ];
    const after = f.counts();
    f.close();
    return {
      absent: invalid.every((x) => x === undefined),
      same: JSON.stringify(before) === JSON.stringify(after)
    };
  });
  expect(result).toEqual({ absent: true, same: true });
});
for (const phase of ['hold_encrypt', 'hold_decrypt', 'hold_sign'] as const)
  test(`disconnect during ${phase} retains real slot until settlement and fences late result`, async ({
    page
  }) => {
    await load(page);
    await page.evaluate((phase) => {
      const f = window.hcp075Fixture,
        op = f.operation();
      if (!op) throw Error('missing operation');
      window.hcp075Operation = op;
      f.mode(phase);
      window.hcp075Job = window.hcp075.buildPrivateSeal(op);
    }, phase);
    await expect
      .poll(() => page.evaluate(() => window.hcp075Fixture.slot()))
      .toMatchObject({
        state: 'active',
        pending:
          phase === 'hold_encrypt'
            ? 'encrypt'
            : phase === 'hold_decrypt'
              ? 'decrypt'
              : 'sign'
      });
    const pending = await page.evaluate(() => {
      const f = window.hcp075Fixture;
      f.disconnect();
      window.hcp075.stopPrivateSeal(window.hcp075Operation);
      return f.slot();
    });
    expect(pending).toMatchObject({ state: 'active' });
    const result = await page.evaluate(async () => {
      const f = window.hcp075Fixture,
        before = f.counts();
      f.settle();
      const outcome = await window.hcp075Job,
        after = f.counts(),
        slot = f.slot();
      f.close();
      return {
        status: outcome.status,
        slot,
        same: JSON.stringify(before) === JSON.stringify(after)
      };
    });
    expect(result).toEqual({
      status: 'stopped',
      slot: { state: 'idle' },
      same: true
    });
  });
test('wait expiry prevents more calls but does not pretend to cancel pending extension approval', async ({
  page
}) => {
  await load(page);
  await page.evaluate(() => {
    const f = window.hcp075Fixture,
      op = f.operation();
    if (!op) throw Error('missing operation');
    f.mode('hold_encrypt');
    window.hcp075Operation = op;
    window.hcp075Job = window.hcp075.buildPrivateSeal(op);
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp075Fixture.slot()))
    .toMatchObject({ pending: 'encrypt' });
  const result = await page.evaluate(async () => {
    const f = window.hcp075Fixture,
      s = window.hcp075;
    s.expirePrivateSealWait(window.hcp075Operation);
    const pending = f.slot();
    f.settle();
    const outcome = await window.hcp075Job,
      slot = f.slot();
    f.close();
    return { pending, status: outcome.status, slot };
  });
  expect(result.pending).toMatchObject({
    state: 'active',
    pending: 'encrypt',
    phase: 'wait_expired'
  });
  expect(result.status).toBe('stopped');
  expect(result.slot).toEqual({ state: 'idle' });
});
test('held origin owner lock fails busy before any extension call and never queues a retry', async ({
  page
}) => {
  await load(page);
  await page.evaluate(async () => {
    let acquired: () => void = () => {};
    const ready = new Promise<void>((resolve) => {
      acquired = resolve;
    });
    window.hcp075Held = navigator.locks.request(
      'harvestcircle:owner:' + window.hcp075Fixture.owner,
      async () => {
        acquired();
        await new Promise<void>((resolve) => {
          window.hcp075Release = resolve;
        });
      }
    );
    await ready;
  });
  const result = await page.evaluate(async () => {
    const f = window.hcp075Fixture,
      op = f.operation(),
      before = f.counts();
    if (!op) throw Error('missing operation');
    const outcome = await window.hcp075.buildPrivateSeal(op),
      after = f.counts();
    window.hcp075Release();
    await window.hcp075Held;
    const again = await window.hcp075.buildPrivateSeal(op);
    f.close();
    return {
      status: outcome.status,
      same: JSON.stringify(before) === JSON.stringify(after),
      again: again.status
    };
  });
  expect(result).toEqual({ status: 'busy', same: true, again: 'stopped' });
});
test('second peer/self operation cannot overlap the held scheduler/owner scope', async ({
  page
}) => {
  await load(page);
  await page.evaluate(() => {
    const f = window.hcp075Fixture,
      op = f.operation('self');
    if (!op) throw Error('missing operation');
    f.mode('hold_encrypt');
    window.hcp075Operation = op;
    window.hcp075Job = window.hcp075.buildPrivateSeal(op);
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp075Fixture.slot()))
    .toMatchObject({ state: 'active', pending: 'encrypt' });
  const result = await page.evaluate(async () => {
    const f = window.hcp075Fixture,
      s = window.hcp075,
      second = f.operation('peer'),
      before = f.counts();
    if (!second) throw Error('missing second operation');
    const blocked = await s.buildPrivateSeal(second),
      after = f.counts();
    s.stopPrivateSeal(window.hcp075Operation);
    f.settle();
    const original = await window.hcp075Job;
    f.close();
    return {
      blocked: blocked.status,
      same: JSON.stringify(before) === JSON.stringify(after),
      original: original.status
    };
  });
  expect(result).toEqual({ blocked: 'busy', same: true, original: 'stopped' });
});
test('provider method getter is captured before scheduler encryption admission, never re-read with private input', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp075Fixture,
      op = f.operation();
    if (!op) throw Error('missing operation');
    f.mode('getter_reentry');
    const outcome = await window.hcp075.buildPrivateSeal(op),
      counts = f.counts();
    f.close();
    return {
      status: outcome.status,
      reentrant: counts.reentrantReads,
      postStop: counts.postStopEncrypts
    };
  });
  expect(result).toEqual({ status: 'sealed', reentrant: 0, postStop: 0 });
});
for (const mode of [
  'stop_capability_getter',
  'stop_capability_after_key'
] as const)
  test(`${mode} stops only the original operation and preserves the current shared identity`, async ({
    page
  }) => {
    await load(page);
    const result = await page.evaluate(async (mode) => {
      const f = window.hcp075Fixture,
        op = f.operation(),
        before = f.counts();
      if (!op) throw Error('missing operation');
      f.mode(mode);
      const outcome = await window.hcp075.buildPrivateSeal(op),
        state = f.identityState(),
        after = f.counts();
      f.close();
      return {
        status: outcome.status,
        state,
        signs: after.signs - before.signs,
        encrypts: after.encrypts - before.encrypts
      };
    }, mode);
    expect(result).toEqual({
      status: 'stopped',
      state: 'messaging_capable',
      signs: 0,
      encrypts: 0
    });
  });
