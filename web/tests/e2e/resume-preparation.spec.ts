import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Resume from './harness/resume-preparation.ts';
declare global {
  interface Window {
    hcp083: typeof Resume;
    hcp083Fixture: Awaited<ReturnType<typeof Resume.makeFixture>>;
    hcp083Token: ReturnType<Window['hcp083Fixture']['capture']>;
    hcp083Pending: Promise<
      Awaited<ReturnType<typeof Resume.resumeEncryptedPreparation>>
    >;
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
          new URL('./harness/resume-preparation.ts', import.meta.url)
        ),
        name: 'hcp083',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(built) ? built[0] : built;
  if (!('output' in output)) throw Error('missing resume bundle');
  const chunks = output.output.filter((entry) => entry.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP083_ACTUAL_RELOAD_SDK_IDB',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'Source-only disposable provider, actual SDK/IDB/WebLocks; no installed extension/relay/client Q'
    })
  );
});
test.afterAll(async () => server.close());
async function load(page: Page, initialise = true) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async (initialise) => {
    window.hcp083Fixture = await window.hcp083.makeFixture(initialise);
  }, initialise);
}
test('actual reload loses presentation capabilities and explicitly recovers the same rumor and missing peer without rebuilding self', async ({
  page
}) => {
  let sockets = 0;
  page.on('websocket', () => sockets++);
  await load(page);
  const before = await page.evaluate(async () => {
    const row = await window.hcp083Fixture.row();
    if (!row) throw Error('missing self');
    return row.record;
  });
  await page.reload();
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp083Fixture = await window.hcp083.makeFixture(false);
  });
  const result = await page.evaluate(async () => {
    const f = window.hcp083Fixture,
      s = window.hcp083;
    try {
      const token = f.capture(),
        initial = f.counts(),
        raw = await f.raw();
      const recovered = await s.resumeEncryptedPreparation(
          token,
          undefined,
          'reviewed_private_resume'
        ),
        rumor = s.resumeRecoveredRumor(token),
        memory = rumor && s.rumorPlanSnapshot(rumor),
        afterRecovery = f.counts();
      const pair = await s.resumeEncryptedPreparation(
          token,
          f.context,
          'reviewed_private_resume'
        ),
        row = await f.row(),
        peer =
          row?.record.peerArtifact &&
          f.decryptPeer(row.record.peerArtifact.wire);
      return {
        recovered: recovered.status,
        pair: pair.status,
        memory,
        record: row?.record,
        peer,
        noSelfCrypto:
          afterRecovery.encrypts === initial.encrypts &&
          afterRecovery.signs === initial.signs,
        privateOnly:
          !raw.includes(f.marker) && !(await f.raw()).includes(f.marker)
      };
    } finally {
      f.close();
    }
  });
  expect(result.recovered).toBe('recovered');
  expect(result.pair).toBe('prepared');
  expect(result.noSelfCrypto).toBe(true);
  expect(result.privateOnly).toBe(true);
  expect(result.record?.self).toEqual(before.self);
  expect(result.record?.rumorHash).toBe(before.rumorHash);
  expect(result.record?.createdAt).toBe(before.createdAt);
  expect(result.memory?.id).toBe(before.rumorHash);
  expect(result.memory?.wire).toContain(
    'HCP083_PRIVATE_RECOVERY_MEMORY_ONLY_SENTINEL'
  );
  expect(result.peer?.id).toBe(before.rumorHash);
  expect(result.peer?.created_at).toBe(before.createdAt);
  const originalRumor = JSON.parse(result.memory!.wire) as { content: string };
  expect(result.peer?.content).toBe(originalRumor.content);
  expect(result.peer?.content).toContain('Enquiry about "Carrots"');
  expect(result.peer?.content).toContain(
    'HCP083_PRIVATE_RECOVERY_MEMORY_ONLY_SENTINEL'
  );
  expect(sockets).toBe(0);
});
test('capture and unreviewed Resume spend no prompts or private effects', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp083Fixture,
      s = window.hcp083;
    try {
      const before = f.counts(),
        raw = await f.raw(),
        token = f.capture(),
        result = await s.resumeEncryptedPreparation(token, f.context, 'wrong');
      return {
        status: result.status,
        inert: JSON.stringify(before) === JSON.stringify(f.counts()),
        unchanged: raw === (await f.raw()),
        noRumor: !s.resumeRecoveredRumor(token)
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'invalid',
    inert: true,
    unchanged: true,
    noRumor: true
  });
});
test('erased evidence explicitly discloses loss without a new reservation or SDK prompt', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp083Fixture,
      s = window.hcp083;
    try {
      await f.write('erase');
      const before = f.counts(),
        token = f.capture(),
        r = await s.resumeEncryptedPreparation(
          token,
          f.context,
          'reviewed_private_resume'
        );
      return {
        status: r.status,
        copy: s.resumePreparationSnapshot(token)?.copy,
        inert: JSON.stringify(before) === JSON.stringify(f.counts()),
        raw: await f.raw(),
        noRumor: !s.resumeRecoveredRumor(token)
      };
    } finally {
      f.close();
    }
  });
  expect(result.status).toBe('lost_evidence');
  expect(result.copy).toContain('lost');
  expect(result.inert).toBe(true);
  expect(result.raw).toBe('[]');
  expect(result.noRumor).toBe(true);
});
test('corrupt stored recovery stays intact and blocks decryption and silent reset', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp083Fixture,
      s = window.hcp083;
    try {
      await f.write('corrupt');
      const raw = await f.raw(),
        before = f.counts(),
        r = await s.resumeEncryptedPreparation(
          f.capture(),
          f.context,
          'reviewed_private_resume'
        );
      return {
        status: r.status,
        unchanged: raw === (await f.raw()),
        inert: JSON.stringify(before) === JSON.stringify(f.counts())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'corrupt_record',
    unchanged: true,
    inert: true
  });
});
test('fresh changed extension key blocks before the first recovery decrypt', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp083Fixture,
      s = window.hcp083;
    try {
      const raw = await f.raw(),
        before = f.counts(),
        token = f.capture();
      f.mode('changed_key');
      const r = await s.resumeEncryptedPreparation(
        token,
        f.context,
        'reviewed_private_resume'
      );
      return {
        status: r.status,
        unchanged: raw === (await f.raw()),
        noDecrypt: f.counts().decrypts === before.decrypts,
        noRumor: !s.resumeRecoveredRumor(token)
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'stopped',
    unchanged: true,
    noDecrypt: true,
    noRumor: true
  });
});
test('late decryption after Disconnect cannot restore memory or continue peer work', async ({
  page
}) => {
  await load(page);
  await page.evaluate(() => {
    const f = window.hcp083Fixture;
    f.mode('hold_decrypt');
    window.hcp083Token = f.capture();
    window.hcp083Pending = window.hcp083.resumeEncryptedPreparation(
      window.hcp083Token,
      f.context,
      'reviewed_private_resume'
    );
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp083Fixture.pending()))
    .toBe(true);
  const result = await page.evaluate(async () => {
    const f = window.hcp083Fixture,
      s = window.hcp083;
    try {
      const raw = await f.raw(),
        before = f.counts();
      f.disconnect();
      f.mode('normal');
      f.settle();
      const r = await window.hcp083Pending;
      return {
        status: r.status,
        unchanged: raw === (await f.raw()),
        noPeer:
          f.counts().encrypts === before.encrypts &&
          f.counts().signs === before.signs,
        noRumor: !s.resumeRecoveredRumor(window.hcp083Token)
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'stopped',
    unchanged: true,
    noPeer: true,
    noRumor: true
  });
});
test('stopped pending decryption occupies the shared owner until actual settlement', async ({
  page
}) => {
  await load(page);
  await page.evaluate(() => {
    const f = window.hcp083Fixture;
    f.mode('hold_decrypt');
    window.hcp083Token = f.capture();
    window.hcp083Pending = window.hcp083.resumeEncryptedPreparation(
      window.hcp083Token,
      undefined,
      'reviewed_private_resume'
    );
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp083Fixture.pending()))
    .toBe(true);
  const result = await page.evaluate(async () => {
    const f = window.hcp083Fixture,
      s = window.hcp083;
    try {
      s.stopResumePreparation(window.hcp083Token);
      const before = f.counts(),
        other = f.capture(),
        r = await s.resumeEncryptedPreparation(
          other,
          undefined,
          'reviewed_private_resume'
        );
      const noQueue = JSON.stringify(before) === JSON.stringify(f.counts());
      f.mode('normal');
      f.settle();
      return {
        second: r.status,
        first: (await window.hcp083Pending).status,
        noQueue,
        noRumor: !s.resumeRecoveredRumor(window.hcp083Token)
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    second: 'busy',
    first: 'stopped',
    noQueue: true,
    noRumor: true
  });
});
test('decryption refusal preserves self ciphertext and never falls back to encryption or signing', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp083Fixture,
      s = window.hcp083;
    try {
      const raw = await f.raw(),
        before = f.counts(),
        token = f.capture();
      f.mode('declined_decrypt');
      const r = await s.resumeEncryptedPreparation(
        token,
        f.context,
        'reviewed_private_resume'
      );
      return {
        status: r.status,
        unchanged: raw === (await f.raw()),
        noFallback:
          f.counts().encrypts === before.encrypts &&
          f.counts().signs === before.signs,
        noRumor: !s.resumeRecoveredRumor(token)
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'refused',
    unchanged: true,
    noFallback: true,
    noRumor: true
  });
});
test('substituted decrypted plaintext fails authenticated rumor recovery', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp083Fixture,
      s = window.hcp083;
    try {
      const raw = await f.raw(),
        token = f.capture();
      f.mode('wrong_plaintext');
      const r = await s.resumeEncryptedPreparation(
        token,
        f.context,
        'reviewed_private_resume'
      );
      return {
        status: r.status,
        unchanged: raw === (await f.raw()),
        noRumor: !s.resumeRecoveredRumor(token)
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'mismatch',
    unchanged: true,
    noRumor: true
  });
});
test('completed pair recovery and repeated Resume retain both original artifacts without peer regeneration', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp083Fixture,
      s = window.hcp083;
    try {
      const first = f.capture();
      await s.resumeEncryptedPreparation(
        first,
        f.context,
        'reviewed_private_resume'
      );
      const raw = await f.raw(),
        before = f.counts(),
        repeat = await s.resumeEncryptedPreparation(
          first,
          f.context,
          'reviewed_private_resume'
        ),
        inert = JSON.stringify(before) === JSON.stringify(f.counts());
      s.stopResumePreparation(first);
      const fresh = f.capture(),
        nextBefore = f.counts(),
        r = await s.resumeEncryptedPreparation(
          fresh,
          f.context,
          'reviewed_private_resume'
        );
      return {
        repeat: repeat.status,
        inert,
        fresh: r.status,
        noPeer:
          f.counts().encrypts === nextBefore.encrypts &&
          f.counts().signs === nextBefore.signs,
        unchanged: raw === (await f.raw())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    repeat: 'prepared',
    inert: true,
    fresh: 'already_prepared',
    noPeer: true,
    unchanged: true
  });
});
test('unacknowledged full-wire mutation while decrypting is refused and retained', async ({
  page
}) => {
  await load(page);
  await page.evaluate(() => {
    const f = window.hcp083Fixture;
    f.mode('hold_decrypt');
    window.hcp083Token = f.capture();
    window.hcp083Pending = window.hcp083.resumeEncryptedPreparation(
      window.hcp083Token,
      f.context,
      'reviewed_private_resume'
    );
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp083Fixture.pending()))
    .toBe(true);
  const result = await page.evaluate(async () => {
    const f = window.hcp083Fixture,
      s = window.hcp083;
    try {
      await f.write('revision');
      const raw = await f.raw(),
        before = f.counts();
      f.mode('normal');
      f.settle();
      const r = await window.hcp083Pending;
      return {
        status: r.status,
        unchanged: raw === (await f.raw()),
        noPeer:
          f.counts().encrypts === before.encrypts &&
          f.counts().signs === before.signs,
        noRumor: !s.resumeRecoveredRumor(window.hcp083Token)
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    status: 'conflict',
    unchanged: true,
    noPeer: true,
    noRumor: true
  });
});
