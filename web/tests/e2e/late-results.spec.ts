import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Late from './harness/late-results.ts';
import type * as Legacy from '../../src/lib/nostr/extension.ts';
import type { ApprovedSignResult } from '../../src/lib/nostr/extension.ts';
import type { PublicEffectResult } from '../../src/lib/runtime/effect-ownership.ts';
declare global {
  interface Window {
    hcp055: typeof Late;
    hcp055Fixture: ReturnType<typeof Late.makeFixture>;
    hcp055Other: ReturnType<typeof Late.makeFixture>;
    hcp055Legacy: typeof Legacy;
    hcp055Session: Late.IdentitySession;
    hcp055Operation: Late.IdentityPublicOperation;
    hcp055Database: Parameters<typeof Late.closeBrowserDatabase>[0];
    hcp055Repository: ReturnType<typeof Late.createPublicQuotaRepository> & {};
    hcp055Release(): void;
    hcp055Started(): void;
    hcp055Inner: ApprovedSignResult;
    hcp055Counts: { keys: number; signs: number; work: number };
    hcp055Task: Promise<PublicEffectResult<unknown>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const legacyOnly = process.env.HARVESTCIRCLE_HCP055_LEGACY_RED === '1';
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL(
            legacyOnly
              ? '../../src/lib/nostr/extension.ts'
              : './harness/late-results.ts',
            import.meta.url
          )
        ),
        name: legacyOnly ? 'hcp055Legacy' : 'hcp055',
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
      fixture: 'HCP055_ACTUAL_SDK_IDB_ORIGINAL_LATE_REVIEW',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'controlled local Chromium and genuine SDK; no named extension/UI publication/real relay claim'
    })
  );
});
test.afterAll(async () => server.close());
test.afterEach(async ({ page }) => {
  await page.evaluate(() => {
    window.hcp055Fixture?.close();
    window.hcp055Other?.close();
    if (window.hcp055Database)
      window.hcp055.closeBrowserDatabase(window.hcp055Database);
  });
});
test('legacy adapter capture never revives after observed account loss and same-author reconnect', async ({
  page
}) => {
  await page.goto(server.url + '/search');
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('../../src/lib/nostr/extension.ts', import.meta.url)
        ),
        name: 'hcp055Legacy',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing legacy bundle');
  const chunks = output.output.filter((item) => item.type === 'chunk');
  expect(chunks).toHaveLength(1);
  await page.addScriptTag({ content: chunks[0].code });
  const observed = await page.evaluate(async () => {
    const s = window.hcp055Legacy;
    const owner =
      '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
    const other =
      'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
    const adapter = s.createExtensionAdapter();
    window.nostr = { getPublicKey: () => Promise.resolve(owner) };
    await s.connectExtensionAdapter(adapter);
    const original = s.extensionOwnershipCapture(adapter)!;
    window.nostr = { getPublicKey: () => Promise.resolve(other) };
    await s.recheckExtensionAdapter(adapter);
    const lost = original.current();
    window.nostr = { getPublicKey: () => Promise.resolve(owner) };
    await s.connectExtensionAdapter(adapter);
    return { lost, revived: original.current() };
  });
  expect(observed).toEqual({ lost: false, revived: false });
});
async function load(page: Page) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    const s = window.hcp055,
      f = s.makeFixture();
    window.hcp055Fixture = f;
    window.hcp055Other = s.makeFixture();
    window.hcp055Counts = { keys: 0, signs: 0, work: 0 };
    window.nostr = {
      getPublicKey: () => {
        window.hcp055Counts.keys++;
        return Promise.resolve(f.owner);
      },
      signEvent: (template: Parameters<typeof f.sign>[0]) => {
        window.hcp055Counts.signs++;
        return Promise.resolve(f.sign(template));
      }
    };
    const session = s.createIdentitySession();
    window.hcp055Session = session;
    await s.connectIdentity(session);
    const operation = s.captureIdentityPublicOperation(
      session,
      f.record,
      f.id,
      'reviewed_captured_operation'
    );
    if (!operation) throw new Error('invalid operation');
    window.hcp055Operation = operation;
    const db = await s.openBrowserDatabase();
    if (db.state !== 'ready') throw new Error(db.reason);
    window.hcp055Database = db.owner;
    const repo = s.createPublicQuotaRepository(db.owner, f.owner);
    if (!repo) throw new Error('invalid repository');
    window.hcp055Repository = repo;
    const input = document.createElement('input');
    input.id = 'hcp055-input';
    input.value = f.template.content;
    input.oninput = () => s.invalidateIdentityOperations(session);
    document.body.append(input);
    window.hcp055Counts = { keys: 0, signs: 0, work: 0 };
  });
}
async function begin(
  page: Page,
  forgotten = false,
  mode: 'exact' | 'content' | 'cached' = 'exact'
) {
  await page.evaluate(
    ({ forgotten, mode }) => {
      const s = window.hcp055,
        f = window.hcp055Fixture;
      const started = new Promise<void>((resolve) => {
        window.hcp055Started = resolve;
      });
      window.nostr = {
        getPublicKey: () => {
          window.hcp055Counts.keys++;
          return Promise.resolve(f.owner);
        },
        signEvent: (template: Parameters<typeof f.sign>[0]) => {
          window.hcp055Counts.signs++;
          window.hcp055Started();
          return new Promise((resolve) => {
            window.hcp055Release = () => resolve(f.sign(template, mode));
          });
        }
      };
      const ownership = s.identityPublicOperationOwnership(
        window.hcp055Session,
        window.hcp055Operation
      );
      if (!ownership) throw new Error('missing ownership');
      window.hcp055Task = s.runCapturedPublicEffect(
        window.hcp055Repository,
        f.record,
        f.id,
        ownership,
        'reviewed_captured_operation',
        async (lease) => {
          window.hcp055Counts.work++;
          const task = s.signIdentityPublicOperation(
            window.hcp055Session,
            window.hcp055Operation,
            lease
          );
          if (forgotten) {
            void task.then((result) => {
              window.hcp055Inner = result;
            });
            await started;
            return 'forgotten';
          }
          const result = await task;
          window.hcp055Inner = result;
          return result;
        }
      );
    },
    { forgotten, mode }
  );
  await expect
    .poll(() => page.evaluate(() => window.hcp055Counts.signs))
    .toBe(1);
}
async function settle(page: Page) {
  await page.evaluate(() => window.hcp055Release());
  expect(
    await page.evaluate(async () => (await window.hcp055Task).status)
  ).toBe('unknown');
  await expect
    .poll(() => page.evaluate(() => window.hcp055Inner?.status))
    .toBe('unknown');
  expect(await page.evaluate(() => 'artifact' in window.hcp055Inner)).toBe(
    false
  );
  expect(await page.evaluate(() => window.hcp055Counts.signs)).toBe(1);
}
async function reviewed(page: Page, consent = 'review_original_late_artifact') {
  return page.evaluate((consent) => {
    const s = window.hcp055,
      f = window.hcp055Fixture;
    const artifact = s.reviewIdentityLatePublicArtifact(
      window.hcp055Session,
      window.hcp055Operation,
      f.record,
      consent
    );
    return artifact ? s.capturedArtifactSnapshot(artifact) : undefined;
  }, consent);
}
test('logout during approval retains exact original late result; foreign author cannot review or replay it', async ({
  page
}) => {
  await load(page);
  await begin(page);
  await page.evaluate(() =>
    window.hcp055.disconnectIdentity(window.hcp055Session)
  );
  await settle(page);
  expect(await reviewed(page)).toBeUndefined();
  const after = await page.evaluate(async () => {
    const s = window.hcp055,
      f = window.hcp055Other;
    window.nostr = {
      getPublicKey: () => {
        window.hcp055Counts.keys++;
        return Promise.resolve(f.owner);
      },
      signEvent: () => {
        window.hcp055Counts.signs++;
        throw new Error('unexpected replay');
      }
    };
    await s.connectIdentity(window.hcp055Session);
    return {
      view: s.identityPublicOperationSnapshot(
        window.hcp055Session,
        window.hcp055Operation
      ),
      identity: s.identitySessionSnapshot(window.hcp055Session),
      counts: window.hcp055Counts
    };
  });
  expect(after.view).toMatchObject({
    current: false,
    lateAvailable: true,
    outcome: 'unknown'
  });
  expect(after.identity).toMatchObject({
    publicKey: await page.evaluate(() => window.hcp055Other.owner)
  });
  expect(after.counts.signs).toBe(1);
  expect(await reviewed(page)).toBeUndefined();
});
for (const change of ['route', 'input'] as const)
  test(`${change} invalidation fences pending work and keeps original capture unchanged`, async ({
    page
  }) => {
    await load(page);
    const original = await page.evaluate(() => {
      const s = window.hcp055,
        f = window.hcp055Fixture;
      return s.publicRecordWire(f.record, f.owner, f.id);
    });
    await begin(page);
    if (change === 'input')
      await page.locator('#hcp055-input').fill('new public input');
    else
      await page.evaluate(() => {
        history.pushState({}, '', '/settings');
        window.hcp055.invalidateIdentityOperations(window.hcp055Session);
      });
    await settle(page);
    const state = await page.evaluate(async () => {
      const s = window.hcp055,
        f = window.hcp055Fixture,
        inventory = await s.inspectPublicStorage(window.hcp055Repository);
      return {
        view: s.identityPublicOperationSnapshot(
          window.hcp055Session,
          window.hcp055Operation
        ),
        wire: s.publicRecordWire(f.record, f.owner, f.id),
        count: inventory.ok
          ? s.publicInventorySnapshot(window.hcp055Repository, inventory.value)
              ?.rows.length
          : -1,
        calls: window.hcp055Counts
      };
    });
    expect(state.wire).toBe(original);
    expect(state.count).toBe(1);
    expect(state.view).toMatchObject({
      current: false,
      lateAvailable: true,
      outcome: 'unknown'
    });
    expect(state.calls).toEqual({ keys: 1, signs: 1, work: 1 });
    expect(await reviewed(page, 'unreviewed')).toBeUndefined();
    expect(await reviewed(page)).toBeDefined();
  });
for (const action of ['stop', 'wait_expiry'] as const)
  test(`${action} keeps actual prompt reserved and late artifact requires review`, async ({
    page
  }) => {
    await load(page);
    await begin(page);
    await page.evaluate((action) => {
      const s = window.hcp055;
      if (action === 'stop')
        s.stopIdentityPublicOperation(
          window.hcp055Session,
          window.hcp055Operation
        );
      else
        s.expireIdentityPublicOperationWait(
          window.hcp055Session,
          window.hcp055Operation
        );
    }, action);
    const busy = await page.evaluate(
      async () =>
        (
          await window.hcp055.connectIdentity(
            window.hcp055.createIdentitySession()
          )
        ).admission
    );
    expect(busy).toBe('busy');
    await settle(page);
    expect(await page.evaluate(() => window.hcp055Counts)).toEqual({
      keys: 1,
      signs: 1,
      work: 1
    });
    expect(await reviewed(page, 'unreviewed')).toBeUndefined();
    expect(await reviewed(page)).toBeDefined();
  });
test('forgotten operation retains original late review and never becomes a fresh draft', async ({
  page
}) => {
  await load(page);
  await begin(page, true);
  await settle(page);
  const before = await page.evaluate(() => ({ ...window.hcp055Counts }));
  expect(await reviewed(page)).toBeDefined();
  const after = await page.evaluate(async () => {
    const s = window.hcp055,
      read = await s.inspectPublicStorage(window.hcp055Repository);
    return {
      calls: window.hcp055Counts,
      rows: read.ok
        ? s.publicInventorySnapshot(window.hcp055Repository, read.value)?.rows
        : []
    };
  });
  expect(after.calls).toEqual(before);
  expect(after.rows).toHaveLength(1);
  expect(after.rows?.[0].id).toBe(
    await page.evaluate(() => window.hcp055Fixture.id)
  );
});
for (const mode of ['content', 'cached'] as const)
  test(`invalid late ${mode} response cannot become a review capability`, async ({
    page
  }) => {
    await load(page);
    await begin(page, false, mode);
    await page.evaluate(() =>
      window.hcp055.invalidateIdentityOperations(window.hcp055Session)
    );
    await settle(page);
    expect(await reviewed(page)).toBeUndefined();
    expect(
      await page.evaluate(
        () =>
          window.hcp055.identityPublicOperationSnapshot(
            window.hcp055Session,
            window.hcp055Operation
          )?.lateAvailable
      )
    ).toBe(false);
  });
test('account loss then same-author reconnect cannot revive an original operation generation', async ({
  page
}) => {
  await load(page);
  await page.evaluate(async () => {
    const s = window.hcp055,
      other = window.hcp055Other;
    window.nostr = { getPublicKey: () => Promise.resolve(other.owner) };
    await s.recheckIdentityOwner(window.hcp055Session);
  });
  const old = await page.evaluate(() =>
    window.hcp055.identityPublicOperationSnapshot(
      window.hcp055Session,
      window.hcp055Operation
    )
  );
  expect(old?.current).toBe(false);
  const reconnected = await page.evaluate(async () => {
    const s = window.hcp055,
      f = window.hcp055Fixture;
    window.nostr = {
      getPublicKey: () => Promise.resolve(f.owner),
      signEvent: () => {
        window.hcp055Counts.signs++;
        throw new Error('unexpected replay');
      }
    };
    await s.connectIdentity(window.hcp055Session);
    return s.identityPublicOperationSnapshot(
      window.hcp055Session,
      window.hcp055Operation
    );
  });
  expect(reconnected?.current).toBe(false);
  expect(await page.evaluate(() => window.hcp055Counts.signs)).toBe(0);
});
test('same-author explicit review after reconnect returns only the original verified artifact with zero effects', async ({
  page
}) => {
  await load(page);
  await begin(page);
  await page.evaluate(() =>
    window.hcp055.disconnectIdentity(window.hcp055Session)
  );
  await settle(page);
  await page.evaluate(async () => {
    const s = window.hcp055,
      f = window.hcp055Fixture;
    window.nostr = {
      getPublicKey: () => {
        window.hcp055Counts.keys++;
        return Promise.resolve(f.owner);
      },
      signEvent: () => {
        window.hcp055Counts.signs++;
        throw new Error('unexpected new signature');
      }
    };
    await s.connectIdentity(window.hcp055Session);
  });
  const before = await page.evaluate(() => ({ ...window.hcp055Counts }));
  const artifact = await reviewed(page);
  expect(artifact?.author).toBe(
    await page.evaluate(() => window.hcp055Fixture.owner)
  );
  expect(artifact?.hash).toBe(
    await page.evaluate(() => {
      const s = window.hcp055,
        f = window.hcp055Fixture,
        row = s.publicRecordSnapshot(f.record, f.owner, f.id);
      return row && row.family !== 'public_draft' ? row.capture.hash : null;
    })
  );
  expect(await page.evaluate(() => window.hcp055Counts)).toEqual(before);
  expect(
    await page.evaluate(
      () =>
        window.hcp055.identityPublicOperationSnapshot(
          window.hcp055Session,
          window.hcp055Operation
        )?.current
    )
  ).toBe(false);
});
test('identity continuation cannot publish a signed artifact after its view was invalidated', async ({
  page
}) => {
  let exercised = 0;
  for (let depth = 0; depth <= 32; depth++) {
    await load(page);
    const observed = await page.evaluate(async (depth) => {
      const s = window.hcp055,
        f = window.hcp055Fixture;
      let keys = 0,
        beforeInvalidation: string | undefined;
      window.nostr = {
        getPublicKey: () => {
          keys++;
          if (keys === 2) {
            let remaining = depth;
            const advance = () => {
              if (remaining-- > 0) queueMicrotask(advance);
              else {
                beforeInvalidation = s.identityPublicOperationSnapshot(
                  window.hcp055Session,
                  window.hcp055Operation
                )?.outcome;
                s.invalidateIdentityOperations(window.hcp055Session);
              }
            };
            queueMicrotask(advance);
          }
          return Promise.resolve(f.owner);
        },
        signEvent: (template: Parameters<typeof f.sign>[0]) =>
          Promise.resolve(f.sign(template))
      };
      const ownership = s.identityPublicOperationOwnership(
        window.hcp055Session,
        window.hcp055Operation
      )!;
      let inner: ApprovedSignResult | undefined;
      await s.runCapturedPublicEffect(
        window.hcp055Repository,
        f.record,
        f.id,
        ownership,
        'reviewed_captured_operation',
        async (lease) => {
          inner = await s.signIdentityPublicOperation(
            window.hcp055Session,
            window.hcp055Operation,
            lease
          );
          return inner;
        }
      );
      const result = {
        depth,
        beforeInvalidation,
        inner: inner?.status,
        hasArtifact: inner !== undefined && 'artifact' in inner,
        reviewed:
          s.reviewIdentityLatePublicArtifact(
            window.hcp055Session,
            window.hcp055Operation,
            f.record,
            'review_original_late_artifact'
          ) !== undefined,
        view: s.identityPublicOperationSnapshot(
          window.hcp055Session,
          window.hcp055Operation
        )
      };
      f.close();
      window.hcp055Other.close();
      s.closeBrowserDatabase(window.hcp055Database);
      return result;
    }, depth);
    if (observed.beforeInvalidation === 'pending') {
      exercised++;
      expect(observed, `actual microtask depth ${depth}`).not.toMatchObject({
        inner: 'signed'
      });
      expect(observed.hasArtifact).toBe(false);
      expect(observed.reviewed).toBe(true);
    }
  }
  expect(exercised).toBeGreaterThan(0);
});
