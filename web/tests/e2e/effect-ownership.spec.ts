import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Effects from './harness/effect-ownership.ts';
import type { PublicRecordHandle } from '../../src/lib/persistence/records.ts';
import type { PublicQuotaRepository } from '../../src/lib/persistence/quota.ts';
import type { BrowserDatabase } from '../../src/lib/persistence/database.ts';
import type {
  ExtensionAdapter,
  ApprovedSignResult
} from '../../src/lib/nostr/extension.ts';
import type { ApprovedPublicSigning } from '../../src/lib/nostr/approved-signing.ts';
import type {
  PublicEffectLease,
  PublicEffectResult
} from '../../src/lib/runtime/effect-ownership.ts';
declare global {
  interface Window {
    hcp054: typeof Effects;
    hcp054Fixture: ReturnType<typeof Effects.makeFixture>;
    hcp054Record: PublicRecordHandle;
    hcp054Repository: PublicQuotaRepository;
    hcp054Database: BrowserDatabase;
    hcp054Adapter: ExtensionAdapter;
    hcp054Approval: ApprovedPublicSigning;
    hcp054Owner: string;
    hcp054Id: string;
    hcp054Wire: string;
    hcp054Counts: { keys: number; signs: number; work: number };
    hcp054Release: () => void;
    hcp054SignStarted: () => void;
    hcp054Inner: ApprovedSignResult;
    hcp054Lease: PublicEffectLease;
    hcp054Task: Promise<PublicEffectResult<unknown>>;
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
          new URL('./harness/effect-ownership.ts', import.meta.url)
        ),
        name: 'hcp054',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing bundle');
  const chunks = output.output.filter((item) => item.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP054_ACTUAL_WEB_LOCKS_IDB_SDK_MULTITAB',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'real local Chromium/source only; no named extension/externalclient/global cancellation claim'
    })
  );
});
test.afterAll(async () => server.close());
type Shared = { owner: string; id: string; wire: string };
async function load(page: Page, shared?: Shared): Promise<Shared> {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  return page.evaluate(async (shared) => {
    const s = window.hcp054;
    const f = shared ? undefined : s.makeFixture();
    if (f) window.hcp054Fixture = f;
    const owner = shared?.owner ?? f!.owner,
      id = shared?.id ?? f!.id,
      wire = shared?.wire ?? s.publicRecordWire(f!.record, owner, id)!;
    const decoded = s.decodePublicRecord(wire, owner, id);
    if (!decoded.ok) throw new Error('invalid public fixture');
    window.hcp054Owner = owner;
    window.hcp054Id = id;
    window.hcp054Wire = wire;
    window.hcp054Record = decoded.value;
    const opened = await s.openBrowserDatabase();
    if (opened.state !== 'ready') throw new Error(opened.reason);
    window.hcp054Database = opened.owner;
    const repo = s.createPublicQuotaRepository(opened.owner, owner);
    if (!repo) throw new Error('invalid repository');
    window.hcp054Repository = repo;
    window.hcp054Counts = { keys: 0, signs: 0, work: 0 };
    window.nostr = {
      getPublicKey: () => {
        window.hcp054Counts.keys++;
        return Promise.resolve(owner);
      },
      signEvent: (
        template: Parameters<ReturnType<typeof s.makeFixture>['sign']>[0]
      ) => {
        window.hcp054Counts.signs++;
        if (!f) throw new Error('unexpected second-tab signature');
        return Promise.resolve(f.sign(template));
      }
    };
    window.hcp054Adapter = s.createExtensionAdapter();
    await s.connectExtensionAdapter(window.hcp054Adapter);
    const approval = s.approveCapturedPublicSigning(
      decoded.value,
      owner,
      id,
      'reviewed_captured_operation'
    );
    if (!approval) throw new Error('missing approval');
    window.hcp054Approval = approval;
    window.hcp054Counts = { keys: 0, signs: 0, work: 0 };
    return { owner, id, wire };
  }, shared);
}
async function begin(
  page: Page,
  mode: 'hold' | 'sign_hold' | 'forgotten' | 'commit' | 'wrong_session' = 'hold'
) {
  await page.evaluate((mode) => {
    const s = window.hcp054,
      context = s.extensionOwnershipCapture(window.hcp054Adapter);
    if (!context) throw new Error('missing context');
    const signStarted = new Promise<void>((resolve) => {
      window.hcp054SignStarted = resolve;
    });
    if (mode === 'sign_hold' || mode === 'forgotten')
      window.nostr = {
        getPublicKey: () => {
          window.hcp054Counts.keys++;
          return Promise.resolve(window.hcp054Owner);
        },
        signEvent: (
          template: Parameters<ReturnType<typeof s.makeFixture>['sign']>[0]
        ) => {
          window.hcp054Counts.signs++;
          window.hcp054SignStarted();
          return new Promise((resolve) => {
            window.hcp054Release = () =>
              resolve(window.hcp054Fixture.sign(template));
          });
        }
      };
    const captured =
      mode === 'wrong_session' ? { ...context, session: Symbol() } : context;
    window.hcp054Task = s.runCapturedPublicEffect(
      window.hcp054Repository,
      window.hcp054Record,
      window.hcp054Id,
      captured,
      'reviewed_captured_operation',
      async (lease) => {
        window.hcp054Counts.work++;
        window.hcp054Lease = lease;
        if (mode === 'hold') {
          await new Promise<void>((resolve) => {
            window.hcp054Release = resolve;
          });
          return 'original';
        }
        if (mode === 'forgotten') {
          void s
            .signApprovedExtensionAdapter(
              window.hcp054Adapter,
              window.hcp054Approval,
              lease
            )
            .then((result) => {
              window.hcp054Inner = result;
            });
          await signStarted;
          return 'forgotten';
        }
        const result = await s.signApprovedExtensionAdapter(
          window.hcp054Adapter,
          window.hcp054Approval,
          lease
        );
        if (mode !== 'commit' || result.status !== 'signed') return result;
        const next = s.preparePublicArtifactTransition(
          window.hcp054Record,
          window.hcp054Owner,
          window.hcp054Id,
          result.artifact
        );
        if (!next.ok) throw new Error(next.reason);
        const stored = await s.commitPublicOperationTransition(
          window.hcp054Repository,
          next.value
        );
        if (!stored.ok) throw new Error(stored.reason);
        return s.capturedArtifactSnapshot(result.artifact)?.wire;
      }
    );
  }, mode);
}
async function retry(page: Page) {
  return page.evaluate(async () => {
    const s = window.hcp054,
      context = s.extensionOwnershipCapture(window.hcp054Adapter);
    if (!context) throw new Error('missing context');
    const result = await s.runCapturedPublicEffect(
      window.hcp054Repository,
      window.hcp054Record,
      window.hcp054Id,
      context,
      'reviewed_captured_operation',
      () => {
        window.hcp054Counts.work++;
        return Promise.resolve('unexpected replay');
      }
    );
    const row =
      'record' in result
        ? s.publicRecordSnapshot(
            result.record,
            window.hcp054Owner,
            window.hcp054Id
          )
        : undefined;
    return { status: result.status, row, counts: window.hcp054Counts };
  });
}
test.afterEach(async ({ context }) => {
  for (const page of context.pages())
    if (!page.isClosed())
      await page.evaluate(() => {
        window.hcp054Fixture?.close();
        if (window.hcp054Database)
          window.hcp054.closeBrowserDatabase(window.hcp054Database);
      });
});
test('two tabs same command create one durable original plan and only one active callback', async ({
  page,
  context
}) => {
  const original = await load(page),
    other = await context.newPage();
  await load(other, original);
  await begin(page);
  await expect
    .poll(() => page.evaluate(() => window.hcp054Counts.work))
    .toBe(1);
  expect((await retry(other)).status).toBe('busy');
  expect((await retry(other)).counts).toEqual({ keys: 0, signs: 0, work: 0 });
  await page.evaluate(() => window.hcp054Release());
  expect(
    await page.evaluate(async () => (await window.hcp054Task).status)
  ).toBe('completed');
  const recovered = await retry(other);
  expect(recovered.status).toBe('unknown');
  expect(
    recovered.row &&
      recovered.row.family !== 'public_draft' &&
      recovered.row.capture.wire
  ).toBe(
    (JSON.parse(original.wire) as { capture: { wire: string } }).capture.wire
  );
  expect(recovered.counts.work).toBe(0);
});
test('page loss during actual SDK prompt retains original unknown capture without replan or signature', async ({
  page,
  context
}) => {
  const original = await load(page),
    other = await context.newPage();
  await load(other, original);
  await begin(page, 'sign_hold');
  await expect
    .poll(() => page.evaluate(() => window.hcp054Counts.signs))
    .toBe(1);
  expect((await retry(other)).status).toBe('busy');
  await page.evaluate(() => window.hcp054Fixture.close());
  await page.close();
  const recovered = await retry(other);
  expect(recovered.status).toBe('unknown');
  expect(
    recovered.row &&
      recovered.row.family !== 'public_draft' &&
      recovered.row.artifact
  ).toBeNull();
  expect(recovered.row && JSON.stringify(recovered.row)).toBe(original.wire);
  expect(recovered.counts).toEqual({ keys: 0, signs: 0, work: 0 });
});
test('forgotten SDK await keeps the browser lock and reports unknown after actual settlement', async ({
  page,
  context
}) => {
  const original = await load(page),
    other = await context.newPage();
  await load(other, original);
  await begin(page, 'forgotten');
  await expect
    .poll(() => page.evaluate(() => window.hcp054Counts.signs))
    .toBe(1);
  expect((await retry(other)).status).toBe('busy');
  await page.evaluate(() => window.hcp054Release());
  expect(
    await page.evaluate(async () => (await window.hcp054Task).status)
  ).toBe('unknown');
  expect((await retry(other)).status).toBe('unknown');
});
test('explicit stop during prompt fences late signature and retains lock until settlement', async ({
  page,
  context
}) => {
  const original = await load(page),
    other = await context.newPage();
  await load(other, original);
  await begin(page, 'sign_hold');
  await expect
    .poll(() => page.evaluate(() => window.hcp054Counts.signs))
    .toBe(1);
  await page.evaluate(() => window.hcp054.stopPublicEffect(window.hcp054Lease));
  expect((await retry(other)).status).toBe('busy');
  await page.evaluate(() => window.hcp054Release());
  expect(
    await page.evaluate(async () => (await window.hcp054Task).status)
  ).toBe('unknown');
  expect(await page.evaluate(() => window.hcp054Counts)).toEqual({
    keys: 1,
    signs: 1,
    work: 1
  });
});
test('unrelated owners acquire independent locks while same owner remains busy', async ({
  page,
  context
}) => {
  await load(page);
  const other = await context.newPage();
  await load(other);
  await begin(page);
  await begin(other);
  await expect
    .poll(() => other.evaluate(() => window.hcp054Counts.work))
    .toBe(1);
  await page.evaluate(() => window.hcp054Release());
  await other.evaluate(() => window.hcp054Release());
  expect(
    await page.evaluate(async () => (await window.hcp054Task).status)
  ).toBe('completed');
  expect(
    await other.evaluate(async () => (await window.hcp054Task).status)
  ).toBe('completed');
});
test('unsupported locks perform zero writes/signing and public browsing remains usable', async ({
  page
}) => {
  await load(page);
  await page.evaluate(() =>
    Object.defineProperty(navigator, 'locks', {
      value: undefined,
      configurable: true
    })
  );
  expect((await retry(page)).status).toBe('unavailable');
  const size = await page.evaluate(async () => {
    const s = window.hcp054,
      inventory = await s.inspectPublicStorage(window.hcp054Repository);
    return inventory.ok
      ? s.publicInventorySnapshot(window.hcp054Repository, inventory.value)
          ?.rows.length
      : -1;
  });
  expect(size).toBe(0);
  expect(await page.evaluate(() => window.hcp054Counts)).toEqual({
    keys: 0,
    signs: 0,
    work: 0
  });
  expect(page.url()).toContain('/search');
});
test('a genuine lease with another session cannot authorize any SDK call', async ({
  page
}) => {
  await load(page);
  await begin(page, 'wrong_session');
  const result = await page.evaluate(async () => {
    const value = await window.hcp054Task;
    return {
      status: value.status,
      inner:
        'value' in value ? (value.value as { status: string }).status : null,
      counts: window.hcp054Counts
    };
  });
  expect(result.inner).toBe('invalid_ownership');
  expect(result.counts).toEqual({ keys: 0, signs: 0, work: 1 });
});
test('retained signed artifact stays exact and conflicting capture cannot create another effect', async ({
  page,
  context
}) => {
  const original = await load(page);
  await begin(page, 'commit');
  const signed = await page.evaluate(async () => {
    const value = await window.hcp054Task;
    return 'value' in value ? value.value : undefined;
  });
  expect(typeof signed).toBe('string');
  const other = await context.newPage();
  await load(other, original);
  const replay = await retry(other);
  expect(replay.status).toBe('retained');
  expect(
    replay.row &&
      replay.row.family !== 'public_draft' &&
      replay.row.artifact?.wire
  ).toBe(signed);
  expect(replay.counts).toEqual({ keys: 0, signs: 0, work: 0 });
  const conflict = await other.evaluate(async () => {
    const s = window.hcp054,
      row = s.publicRecordSnapshot(
        window.hcp054Record,
        window.hcp054Owner,
        window.hcp054Id
      );
    if (!row || row.family === 'public_draft')
      throw new Error('invalid fixture');
    const decoded = s.decodePublicRecord(
      JSON.stringify({
        ...row,
        capture: { ...row.capture, targets: ['wss://other.example.org'] }
      }),
      window.hcp054Owner,
      window.hcp054Id
    );
    if (!decoded.ok) throw new Error(decoded.reason);
    const capture = s.extensionOwnershipCapture(window.hcp054Adapter)!;
    return (
      await s.runCapturedPublicEffect(
        window.hcp054Repository,
        decoded.value,
        window.hcp054Id,
        capture,
        'reviewed_captured_operation',
        () => Promise.resolve('forbidden')
      )
    ).status;
  });
  expect(conflict).toBe('conflict');
});

test('one captured lease never signs the same operation twice', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const s = window.hcp054,
      context = s.extensionOwnershipCapture(window.hcp054Adapter)!;
    const result = await s.runCapturedPublicEffect(
      window.hcp054Repository,
      window.hcp054Record,
      window.hcp054Id,
      context,
      'reviewed_captured_operation',
      async (lease) => {
        const first = await s.signApprovedExtensionAdapter(
          window.hcp054Adapter,
          window.hcp054Approval,
          lease
        );
        const second = await s.signApprovedExtensionAdapter(
          window.hcp054Adapter,
          window.hcp054Approval,
          lease
        );
        return {
          first: first.status,
          second: second.status,
          secondArtifact: 'artifact' in second
        };
      }
    );
    return { result, counts: window.hcp054Counts };
  });
  expect(result.counts.signs).toBe(1);
  expect(result.counts.keys).toBe(2);
  expect(result.result.status).toBe('completed');
  expect('value' in result.result ? result.result.value : null).toEqual({
    first: 'signed',
    second: 'stale',
    secondArtifact: false
  });
});

test('orphaned signing job cannot expose a late artifact or issue another SDK call', async ({
  page
}) => {
  await load(page);
  await begin(page, 'forgotten');
  await expect
    .poll(() => page.evaluate(() => window.hcp054Counts.signs))
    .toBe(1);
  await page.evaluate(() => window.hcp054Release());
  expect(
    await page.evaluate(async () => (await window.hcp054Task).status)
  ).toBe('unknown');
  await expect
    .poll(() => page.evaluate(() => window.hcp054Inner?.status))
    .toBe('unknown');
  expect(await page.evaluate(() => 'artifact' in window.hcp054Inner)).toBe(
    false
  );
  expect(await page.evaluate(() => window.hcp054Counts.keys)).toBe(1);
});
test('same author different operation contends before creating a second durable plan', async ({
  page,
  context
}) => {
  const original = await load(page),
    other = await context.newPage();
  const id = crypto.randomUUID(),
    row = JSON.parse(original.wire) as { id: string };
  await load(other, { ...original, id, wire: JSON.stringify({ ...row, id }) });
  await begin(page);
  await expect
    .poll(() => page.evaluate(() => window.hcp054Counts.work))
    .toBe(1);
  expect((await retry(other)).status).toBe('busy');
  expect(
    await other.evaluate(async () => {
      const s = window.hcp054,
        read = await s.inspectPublicStorage(window.hcp054Repository);
      return read.ok
        ? s.publicInventorySnapshot(window.hcp054Repository, read.value)?.rows
            .length
        : -1;
    })
  ).toBe(1);
  expect(await other.evaluate(() => window.hcp054Counts)).toEqual({
    keys: 0,
    signs: 0,
    work: 0
  });
  await page.evaluate(() => window.hcp054Release());
  await page.evaluate(() => window.hcp054Task);
  await begin(other);
  await expect
    .poll(() => other.evaluate(() => window.hcp054Counts.work))
    .toBe(1);
  await other.evaluate(() => window.hcp054Release());
  expect(
    await other.evaluate(async () => (await window.hcp054Task).status)
  ).toBe('completed');
});
test('mismatched full-record approval and fabricated lease perform zero SDK calls', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const s = window.hcp054,
      capture = s.extensionOwnershipCapture(window.hcp054Adapter)!;
    const row = s.publicRecordSnapshot(
      window.hcp054Record,
      window.hcp054Owner,
      window.hcp054Id
    );
    if (!row || row.family === 'public_draft')
      throw new Error('invalid fixture');
    const changed = s.decodePublicRecord(
      JSON.stringify({
        ...row,
        capture: { ...row.capture, targets: ['wss://other.example.org'] }
      }),
      window.hcp054Owner,
      window.hcp054Id
    );
    if (!changed.ok) throw new Error(changed.reason);
    const approval = s.approveCapturedPublicSigning(
      changed.value,
      window.hcp054Owner,
      window.hcp054Id,
      'reviewed_captured_operation'
    );
    if (!approval) throw new Error('invalid changed approval');
    return s.runCapturedPublicEffect(
      window.hcp054Repository,
      window.hcp054Record,
      window.hcp054Id,
      capture,
      'reviewed_captured_operation',
      async (lease) => {
        const mismatched = await s.signApprovedExtensionAdapter(
          window.hcp054Adapter,
          approval,
          lease
        );
        const forged = await s.signApprovedExtensionAdapter(
          window.hcp054Adapter,
          window.hcp054Approval,
          {} as PublicEffectLease
        );
        return {
          mismatched: mismatched.status,
          forged: forged.status,
          counts: window.hcp054Counts
        };
      }
    );
  });
  expect(result.status).toBe('completed');
  expect('value' in result ? result.value : null).toEqual({
    mismatched: 'invalid_ownership',
    forged: 'invalid_ownership',
    counts: { keys: 0, signs: 0, work: 0 }
  });
});
