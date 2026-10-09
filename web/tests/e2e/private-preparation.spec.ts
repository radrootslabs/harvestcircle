import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Preparation from './harness/private-preparation.ts';
declare global {
  interface Window {
    hcp084: typeof Preparation;
    hcp084Fixture: Awaited<ReturnType<typeof Preparation.makeFixture>>;
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
          new URL('./harness/private-preparation.ts', import.meta.url)
        ),
        name: 'hcp084',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(built) ? built[0] : built;
  if (!('output' in output)) throw Error('private fixture bundle missing');
  const chunks = output.output.filter((entry) => entry.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => server.close());
test.afterEach(async ({ page }) => {
  await page.evaluate(() => window.hcp084Fixture?.close());
});
test('genuine enquiry binding admits only exact canonical captured body and preserves prior acknowledgement on mismatch', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp084Fixture,
      s = window.hcp084,
      context = await f.enquiryContext(),
      different = await f.enquiryContext('Different carrots');
    s.updatePrivateComposerText(f.composer, f.marker, 'reviewed_private_text');
    const bound = s.bindPrivateComposerPreparation(
      f.composer,
      f.reserved,
      f.preparation,
      context,
      'reviewed_composer_preparation'
    );
    await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
    const saved = s.privateComposerSnapshot(f.composer),
      bytes = await f.stored(),
      wrongContext = s.bindPrivateComposerPreparation(
        f.composer,
        f.reserved,
        f.preparation,
        different,
        'reviewed_composer_preparation'
      ),
      afterContext = s.privateComposerSnapshot(f.composer);
    s.updatePrivateComposerText(
      f.composer,
      'different private body',
      'reviewed_private_text'
    );
    const wrongBody = s.bindPrivateComposerPreparation(
      f.composer,
      f.reserved,
      f.preparation,
      context,
      'reviewed_composer_preparation'
    );
    return {
      bound,
      saved: saved?.state,
      wrongContext,
      unchangedAck: afterContext?.state === saved?.state,
      wrongBody,
      dirty: s.privateComposerSnapshot(f.composer)?.dirty,
      text: s.privateComposerText(f.composer),
      bytes: bytes === (await f.stored())
    };
  });
  expect(result).toEqual({
    bound: true,
    saved: 'saved_encrypted',
    wrongContext: false,
    unchangedAck: true,
    wrongBody: false,
    dirty: true,
    text: 'different private body',
    bytes: true
  });
});
async function load(page: Page) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp084Fixture = await window.hcp084.makeFixture();
  });
}
test('same-page confirmation defaults Keep editing and Escape preserves exact text and privacy', async ({
  page
}) => {
  const logs: string[] = [];
  page.on('console', (entry) => logs.push(entry.text()));
  await load(page);
  const original = await page.evaluate(() => {
    const f = window.hcp084Fixture;
    return {
      text: f.original,
      result: f.requestClose(),
      snapshot: window.hcp084.privateComposerSnapshot(f.composer)
    };
  });
  expect(original.result).toBe('confirmation_required');
  expect(original.snapshot?.dirty).toBe(true);
  await expect(
    page.getByRole('button', { name: 'Keep editing', exact: true })
  ).toBeFocused();
  await page.getByRole('button', { name: 'Keep editing', exact: true }).click();
  await expect(
    page.getByRole('textbox', { name: 'Private text', exact: true })
  ).toHaveValue(original.text);
  await page.evaluate(() => window.hcp084Fixture.requestClose());
  await page.keyboard.press('Escape');
  await expect(
    page.getByRole('textbox', { name: 'Private text', exact: true })
  ).toHaveValue(original.text);
  const privacy = await page.evaluate(async () => {
    const f = window.hcp084Fixture;
    return {
      public: (await f.publicDraftRows()).includes(f.marker),
      private: (await f.stored()).includes(f.marker),
      storage: JSON.stringify(localStorage).includes(f.marker),
      url: location.href.includes(f.marker),
      snapshot: JSON.stringify(
        window.hcp084.privateComposerSnapshot(f.composer)
      ).includes(f.marker)
    };
  });
  expect(privacy).toEqual({
    public: false,
    private: false,
    storage: false,
    url: false,
    snapshot: false
  });
  expect(
    logs.some((line) =>
      line.includes('HCP081_PRIVATE_TEXT_MEMORY_ONLY_SENTINEL')
    )
  ).toBe(false);
});
test('stale discard cannot clear new edits; reviewed same-revision discard is one-shot', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(() => {
    const f = window.hcp084Fixture,
      s = window.hcp084;
    f.requestClose();
    s.updatePrivateComposerText(
      f.composer,
      'newer private value',
      'reviewed_private_text'
    );
    const stale = s.confirmDiscardNavigation(
        f.navigation,
        'reviewed_discard_private_text'
      ),
      kept = s.privateComposerText(f.composer);
    f.requestClose();
    const denied = s.confirmDiscardNavigation(f.navigation, 'unreviewed'),
      accepted = s.confirmDiscardNavigation(
        f.navigation,
        'reviewed_discard_private_text'
      );
    return {
      stale,
      kept,
      denied,
      accepted,
      text: s.privateComposerText(f.composer),
      repeated: s.confirmDiscardNavigation(
        f.navigation,
        'reviewed_discard_private_text'
      )
    };
  });
  expect(result).toEqual({
    stale: 'review_required',
    kept: 'newer private value',
    denied: 'invalid',
    accepted: 'allowed',
    text: '',
    repeated: 'invalid'
  });
});
test('beforeunload warns only while exact private text lacks encrypted acknowledgement', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp084Fixture,
      s = window.hcp084,
      detach = s.attachPrivateBeforeUnload(f.navigation);
    const first = new Event('beforeunload', { cancelable: true });
    window.dispatchEvent(first);
    await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
    const saved = s.privateComposerSnapshot(f.composer),
      second = new Event('beforeunload', { cancelable: true });
    window.dispatchEvent(second);
    detach?.();
    return {
      dirtyPrevented: first.defaultPrevented,
      savedPrevented: second.defaultPrevented,
      saved: saved?.state,
      dirty: saved?.dirty
    };
  });
  expect(result).toEqual({
    dirtyPrevented: true,
    savedPrevented: false,
    saved: 'saved_encrypted',
    dirty: false
  });
});
test('acknowledged ciphertext labels only captured text revision; newer value stays unsaved', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp084Fixture,
      s = window.hcp084;
    await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
    const saved = s.privateComposerSnapshot(f.composer),
      bytes = await f.stored();
    s.updatePrivateComposerText(
      f.composer,
      'edited after capture',
      'reviewed_private_text'
    );
    const changed = s.privateComposerSnapshot(f.composer);
    return {
      saved: saved?.state,
      changed: changed?.state,
      dirty: changed?.dirty,
      value: s.privateComposerText(f.composer),
      unchanged: bytes === (await f.stored()),
      plaintext: (await f.stored()).includes('edited after capture')
    };
  });
  expect(result).toEqual({
    saved: 'saved_encrypted',
    changed: 'unsaved',
    dirty: true,
    value: 'edited after capture',
    unchanged: true,
    plaintext: false
  });
});
for (const mode of ['declined', 'missing', 'changed_key'] as const)
  test(
    mode + ' self preparation never labels dirty text saved',
    async ({ page }) => {
      await load(page);
      const result = await page.evaluate(async (mode) => {
        const f = window.hcp084Fixture,
          s = window.hcp084;
        f.mode(mode);
        await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
        const state = s.privateComposerSnapshot(f.composer);
        return {
          saved: state?.state === 'saved_encrypted',
          dirty: state?.dirty ?? true,
          receipt: !!s.preparedSelfRecovery(f.preparation),
          plaintext: (await f.stored()).includes(f.marker)
        };
      }, mode);
      expect(result).toEqual({
        saved: false,
        dirty: true,
        receipt: false,
        plaintext: false
      });
    }
  );
for (const mode of ['abort', 'unknown'] as const)
  test(
    mode + ' actual self transaction cannot turn draft into a fresh send',
    async ({ page }) => {
      await load(page);
      const result = await page.evaluate(async (mode) => {
        const f = window.hcp084Fixture,
          s = window.hcp084,
          hits = f.fault(mode);
        const outcome = await s.prepareSelfRecovery(
            f.preparation,
            'reviewed_self_recovery'
          ),
          state = s.privateComposerSnapshot(f.composer);
        return {
          outcome: outcome.status,
          hits: hits(),
          state: state?.state,
          dirty: state?.dirty,
          plaintext: (await f.stored()).includes(f.marker),
          value: s.privateComposerText(f.composer) === f.original
        };
      }, mode);
      expect(result).toEqual(
        mode === 'abort'
          ? {
              outcome: 'aborted',
              hits: 1,
              state: 'needs_action',
              dirty: true,
              plaintext: false,
              value: true
            }
          : {
              outcome: 'reconciled',
              hits: 1,
              state: 'saved_encrypted',
              dirty: false,
              plaintext: false,
              value: true
            }
      );
    }
  );
for (const action of ['discard', 'disconnect'] as const)
  test(
    action +
      ' during real SDK wait preserves pending owner occupancy and cannot advance effects',
    async ({ page }) => {
      await load(page);
      await page.evaluate(() => {
        const f = window.hcp084Fixture;
        f.mode('hold_encrypt');
        Reflect.set(
          window,
          'hcp084Pending',
          window.hcp084.prepareSelfRecovery(
            f.preparation,
            'reviewed_self_recovery'
          )
        );
      });
      await expect
        .poll(() =>
          page.evaluate(() => window.hcp084Fixture.pendingEncryption())
        )
        .toBe(true);
      const waiting = await page.evaluate((action) => {
        const f = window.hcp084Fixture,
          s = window.hcp084;
        if (action === 'discard') {
          f.requestClose();
          s.confirmDiscardNavigation(
            f.navigation,
            'reviewed_discard_private_text'
          );
        } else s.disconnectIdentity(f.identity);
        return {
          pending: f.pendingEncryption(),
          receipt: !!s.preparedSelfRecovery(f.preparation),
          text: s.privateComposerText(f.composer) ?? '',
          bytes: f.counts().encrypts
        };
      }, action);
      expect(waiting.pending).toBe(true);
      expect(waiting.receipt).toBe(false);
      expect(waiting.text).toBe('');
      const settled = await page.evaluate(async () => {
        const f = window.hcp084Fixture;
        f.settle();
        const result = (await Reflect.get(window, 'hcp084Pending')) as {
          status: string;
        };
        return {
          status: result.status,
          pending: f.pendingEncryption(),
          receipt: !!window.hcp084.preparedSelfRecovery(f.preparation),
          plaintext: (await f.stored()).includes(f.marker)
        };
      });
      expect(settled).toEqual({
        status: 'stopped',
        pending: false,
        receipt: false,
        plaintext: false
      });
    }
  );
for (const mode of ['abort', 'unknown'] as const)
  test(
    mode +
      ' pair transaction retains only genuine local self fact and no remote effect',
    async ({ page }) => {
      let sockets = 0;
      page.on('websocket', () => sockets++);
      await load(page);
      const result = await page.evaluate(async (mode) => {
        const f = window.hcp084Fixture,
          s = window.hcp084;
        await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
        const old = await f.stored(),
          hits = f.fault(mode),
          result = await s.preparePairedDelivery(
            f.preparation,
            f.context,
            'reviewed_pair_preparation'
          ),
          snapshot = s.privateComposerSnapshot(f.composer);
        return {
          status: result.status,
          hits: hits(),
          state: snapshot?.state,
          pair: snapshot?.pairState,
          dirty: snapshot?.dirty,
          unchanged: old === (await f.stored()),
          plaintext: (await f.stored()).includes(f.marker)
        };
      }, mode);
      expect(result).toEqual(
        mode === 'abort'
          ? {
              status: 'aborted',
              hits: 1,
              state: 'saved_encrypted',
              pair: 'needs_action',
              dirty: false,
              unchanged: true,
              plaintext: false
            }
          : {
              status: 'reconciled',
              hits: 1,
              state: 'saved_encrypted',
              pair: 'prepared',
              dirty: false,
              unchanged: false,
              plaintext: false
            }
      );
      expect(sockets).toBe(0);
    }
  );
test('body budget and unreviewed edits preserve prior exact memory value', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(() => {
    const f = window.hcp084Fixture,
      s = window.hcp084;
    return {
      oversized: s.updatePrivateComposerText(
        f.composer,
        'x'.repeat(4097),
        'reviewed_private_text'
      ),
      unreviewed: s.updatePrivateComposerText(
        f.composer,
        'replacement',
        'unreviewed'
      ),
      equal: s.privateComposerText(f.composer) === f.original
    };
  });
  expect(result).toEqual({ oversized: false, unreviewed: false, equal: true });
});

for (const role of ['self', 'pair'] as const)
  test(
    role +
      ' actual committed write with unavailable readback remains uncertain instead of fresh',
    async ({ page }) => {
      await load(page);
      const result = await page.evaluate(async (role) => {
        const f = window.hcp084Fixture,
          s = window.hcp084;
        if (role === 'pair')
          await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
        const restore = f.loseReadback();
        const outcome =
          role === 'self'
            ? await s.prepareSelfRecovery(
                f.preparation,
                'reviewed_self_recovery'
              )
            : await s.preparePairedDelivery(
                f.preparation,
                f.context,
                'reviewed_pair_preparation'
              );
        const state = s.privateComposerSnapshot(f.composer),
          rebinding = s.bindPrivateComposerPreparation(
            f.composer,
            f.reserved,
            f.preparation,
            undefined,
            'reviewed_composer_preparation'
          );
        restore();
        s.stopSelfRecoveryPreparation(f.preparation);
        const fresh = s.captureSelfRecoveryPreparation(
          f.repository,
          f.identity,
          f.reserved,
          'reviewed_self_recovery'
        );
        if (!fresh) throw Error('fresh genuine controller missing');
        const freshBinding = s.bindPrivateComposerPreparation(
            f.composer,
            f.reserved,
            fresh,
            undefined,
            'reviewed_composer_preparation'
          ),
          retired = s.privateComposerSnapshot(f.composer);
        s.stopSelfRecoveryPreparation(fresh);
        return {
          outcome: outcome.status,
          readbackFailures: f.readbackFailures(),
          state: state?.state,
          pair: state?.pairState,
          dirty: state?.dirty,
          rebinding,
          freshBinding,
          uncertaintyRetained:
            retired?.state === state?.state &&
            retired?.pairState === state?.pairState,
          original: s.privateComposerText(f.composer) === f.original,
          ciphertext: !!(await f.storedPair()),
          plaintext: (await f.stored()).includes(f.marker)
        };
      }, role);
      expect(result).toEqual(
        role === 'self'
          ? {
              outcome: 'unknown_completion',
              readbackFailures: 1,
              state: 'unknown_completion',
              pair: 'unprepared',
              dirty: true,
              rebinding: false,
              freshBinding: false,
              uncertaintyRetained: true,
              original: true,
              ciphertext: true,
              plaintext: false
            }
          : {
              outcome: 'unknown_completion',
              readbackFailures: 1,
              state: 'saved_encrypted',
              pair: 'unknown_completion',
              dirty: false,
              rebinding: false,
              freshBinding: false,
              uncertaintyRetained: true,
              original: true,
              ciphertext: true,
              plaintext: false
            }
      );
    }
  );

test('pair readback conflict cannot keep a prepared presentation from its prior genuine acknowledgement', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp084Fixture,
      s = window.hcp084;
    await s.prepareSelfRecovery(f.preparation, 'reviewed_self_recovery');
    await s.preparePairedDelivery(
      f.preparation,
      f.context,
      'reviewed_pair_preparation'
    );
    const before = s.privateComposerSnapshot(f.composer)?.pairState;
    await f.changePairRevision();
    const bytes = await f.stored();
    const retry = await s.preparePairedDelivery(
      f.preparation,
      f.context,
      'reviewed_pair_preparation'
    );
    return {
      before,
      retry: retry.status,
      after: s.privateComposerSnapshot(f.composer)?.pairState,
      unchanged: bytes === (await f.stored())
    };
  });
  expect(result).toEqual({
    before: 'prepared',
    retry: 'conflict',
    after: 'needs_action',
    unchanged: true
  });
});
