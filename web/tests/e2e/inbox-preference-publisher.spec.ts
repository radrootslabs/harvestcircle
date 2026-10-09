import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-preference-publisher.ts';
import type { PublicQuotaRepository } from '../../src/lib/persistence/quota.ts';
import type { BrowserDatabase } from '../../src/lib/persistence/database.ts';
import type { InboxPreferenceAction } from '../../src/lib/messaging/inbox-setup-operation.ts';
declare global {
  interface Window {
    hcp066: typeof Fixture;
    hcp066Fixture: Awaited<ReturnType<typeof Fixture.makePublisherFixture>>;
    hcp066Repo: PublicQuotaRepository;
    hcp066Db: BrowserDatabase;
    hcp066Action: InboxPreferenceAction;
    hcp066Id: string;
    hcp066Restore?: () => void;
    hcp066Trace: ReturnType<typeof Fixture.installEffectTrace>;
    hcp066ReleaseFault?: ReturnType<typeof Fixture.installUnsubscribeFailure>;
    hcp066Run?: Promise<{ status: string }>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  socketServer: WebSocketServer,
  bundle: string,
  endpoint: string;
let frames: string[] = [],
  response: 'accepted' | 'refused' | 'silent' | 'timeout-refusal' = 'accepted';
test.beforeAll(async () => {
  server = await createStaticHarness();
  socketServer = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  await once(socketServer, 'listening');
  const address = socketServer.address();
  if (!address || typeof address === 'string')
    throw new Error('missing loopback');
  endpoint = 'ws://127.0.0.1:' + address.port;
  socketServer.on('connection', (socket) =>
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
          ? bytes.toString()
          : Array.isArray(bytes)
            ? Buffer.concat(bytes).toString()
            : Buffer.from(bytes).toString(),
        frame = JSON.parse(raw) as [string, { id: string }];
      if (frame[0] === 'EVENT') {
        frames.push(raw);
        if (response === 'silent') return;
        socket.send(
          JSON.stringify([
            'OK',
            frame[1].id,
            response === 'accepted',
            response === 'accepted'
              ? ''
              : response === 'timeout-refusal'
                ? 'Timeout'
                : 'blocked: controlled refusal'
          ])
        );
      }
    })
  );
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/inbox-preference-publisher.ts', import.meta.url)
        ),
        name: 'hcp066',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing output');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP066_ACTUAL_SDK_REAL_IDB_WEBLOCK_LOOPBACK',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'controlled local source only; no real extension/operator/retention/client qualification'
    })
  );
});
test.afterAll(async () => {
  for (const socket of socketServer.clients) socket.terminate();
  await new Promise<void>((resolve) => socketServer.close(() => resolve()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  frames = [];
  response = 'accepted';
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      class LoopbackSocket extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (String(url) !== 'wss://discovery.example.org/')
            throw new Error('unexpected fixture destination');
          super(endpoint, protocols);
        }
      }
      window.WebSocket = LoopbackSocket;
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    const s = window.hcp066,
      f = await s.makePublisherFixture();
    window.hcp066Fixture = f;
    const opened = await s.openBrowserDatabase();
    if (opened.state !== 'ready') throw new Error(opened.reason);
    window.hcp066Db = opened.owner;
    const repo = s.createPublicQuotaRepository(opened.owner, f.owner);
    if (!repo) throw new Error('missing repo');
    window.hcp066Repo = repo;
    const reviewed = await s.reviewInboxSetup(
      f.identity,
      f.resolve(),
      f.policy,
      s.setupInput(),
      () => 101
    );
    if (reviewed.status !== 'review') throw new Error(reviewed.reason);
    const id = crypto.randomUUID();
    window.hcp066Id = id;
    await s.saveInboxPreferenceOperation(
      repo,
      reviewed.review,
      f.resolve(),
      id,
      'reviewed_global_inbox_replacement'
    );
    const action = await s.beginInboxPreferenceOperation(
      repo,
      f.identity,
      f.policy,
      reviewed.review,
      id,
      () => Promise.resolve(f.resolve()),
      'reviewed_global_inbox_replacement'
    );
    if (action.status !== 'prepared') throw new Error(action.status);
    window.hcp066Action = action.action;
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => {
      window.hcp066Restore?.();
      window.hcp066Trace?.restore();
      window.hcp066ReleaseFault?.restore();
      window.hcp066Fixture?.close();
      if (window.hcp066Db) window.hcp066.closeBrowserDatabase(window.hcp066Db);
    });
});
test('actual marker is acknowledged before signing and exact artifact before the first SDK EVENT', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const s = window.hcp066,
      f = window.hcp066Fixture;
    window.hcp066Trace = s.installEffectTrace();
    let marker = -1;
    f.beforeSign(async () => {
      const stored = await s.loadPreferenceOperation(
        window.hcp066Repo,
        window.hcp066Id
      );
      if (stored.ok)
        marker = s.publicRecordSnapshot(
          stored.value,
          f.owner,
          window.hcp066Id
        )!.revision;
    });
    const result = await s.runInboxPreferenceOperation(window.hcp066Action);
    const stored = await s.loadPreferenceOperation(
      window.hcp066Repo,
      window.hcp066Id
    );
    return {
      status: result.status,
      marker,
      trace: window.hcp066Trace.events(),
      signs: f.signs(),
      row: stored.ok
        ? s.publicRecordSnapshot(stored.value, f.owner, window.hcp066Id)
        : null
    };
  });
  expect(result.status).toBe('completed');
  expect(result.marker).toBe(1);
  expect(result.signs).toBe(1);
  expect(result.trace.indexOf('ack:2')).toBeGreaterThan(-1);
  expect(result.trace.indexOf('EVENT')).toBeGreaterThan(
    result.trace.indexOf('ack:2')
  );
  expect(frames).toHaveLength(1);
  expect(JSON.parse(frames[0]) as unknown[]).toEqual([
    'EVENT',
    JSON.parse(
      result.row!.family !== 'public_draft' ? result.row!.artifact!.wire : ''
    ) as unknown
  ]);
  expect(
    result.row!.family !== 'public_draft' && result.row!.receipts[0].status
  ).toBe('accepted');
});
for (const revision of [1, 2])
  test(`real IDB abort at revision ${revision} prevents EVENT and unknown attempt cannot sign again`, async ({
    page
  }) => {
    const result = await page.evaluate(async (revision) => {
      const s = window.hcp066,
        f = window.hcp066Fixture;
      window.hcp066Restore = s.failPreferenceWrite(revision);
      const first = await s.runInboxPreferenceOperation(window.hcp066Action);
      const resume = await s.resumeInboxPreferenceOperation(
        window.hcp066Repo,
        f.identity,
        f.policy,
        window.hcp066Id,
        () => Promise.resolve(f.resolve()),
        'reviewed_stored_inbox_preference'
      );
      if (revision === 2 && resume.status === 'prepared')
        await s.runInboxPreferenceOperation(resume.action);
      const stored = await s.loadPreferenceOperation(
        window.hcp066Repo,
        window.hcp066Id
      );
      return {
        status: first.status,
        signs: f.signs(),
        revision: stored.ok
          ? s.publicRecordSnapshot(stored.value, f.owner, window.hcp066Id)!
              .revision
          : -1
      };
    }, revision);
    expect(result.status).toBe('storage_failed');
    expect(result.signs).toBe(revision === 1 ? 0 : 1);
    expect(result.revision).toBe(revision - 1);
    expect(frames).toHaveLength(0);
  });
test('late changed extension owner keeps unsigned uncertainty and never publishes', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const s = window.hcp066,
      f = window.hcp066Fixture;
    f.beforeSign(() => {
      f.changeOwner();
      return Promise.resolve();
    });
    const result = await s.runInboxPreferenceOperation(window.hcp066Action);
    const stored = await s.loadPreferenceOperation(
      window.hcp066Repo,
      window.hcp066Id
    );
    return {
      status: result.status,
      signs: f.signs(),
      row: stored.ok
        ? s.publicRecordSnapshot(stored.value, f.owner, window.hcp066Id)
        : null
    };
  });
  expect(result.status).not.toBe('completed');
  expect(result.signs).toBe(1);
  expect(result.row!.revision).toBe(1);
  expect(
    result.row!.family !== 'public_draft' && result.row!.artifact
  ).toBeNull();
  expect(frames).toHaveLength(0);
});
test('explicit stored retry sends the identical artifact without a second signature; accepted targets are skipped', async ({
  page
}) => {
  response = 'refused';
  await page.evaluate(() =>
    window.hcp066.runInboxPreferenceOperation(window.hcp066Action)
  );
  expect(frames).toHaveLength(1);
  response = 'accepted';
  const retry = async () =>
    page.evaluate(async () => {
      const s = window.hcp066,
        f = window.hcp066Fixture;
      s.disconnectIdentity(f.identity);
      const identity = s.createIdentitySession();
      await s.connectIdentity(identity);
      await s.probeIdentityMessaging(identity, 'reviewed_self_copy');
      const prepared = await s.resumeInboxPreferenceOperation(
        window.hcp066Repo,
        identity,
        f.policy,
        window.hcp066Id,
        () => Promise.resolve(f.resolve()),
        'reviewed_stored_inbox_preference'
      );
      if (prepared.status !== 'prepared') throw new Error(prepared.status);
      const result = {
        result: (await s.runInboxPreferenceOperation(prepared.action)).status,
        signs: f.signs()
      };
      s.disconnectIdentity(identity);
      return result;
    });
  expect(await retry()).toEqual({ result: 'completed', signs: 1 });
  expect(frames).toHaveLength(2);
  expect(frames[1]).toBe(frames[0]);
  expect(await retry()).toEqual({ result: 'completed', signs: 1 });
  expect(frames).toHaveLength(2);
});
test('Stop during the actual permission wait prevents publication and preserves uncertainty', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const s = window.hcp066,
      f = window.hcp066Fixture;
    f.beforeSign(() => {
      s.stopInboxPreferenceOperation(window.hcp066Action);
      return Promise.resolve();
    });
    const result = await s.runInboxPreferenceOperation(window.hcp066Action);
    const row = await s.loadPreferenceOperation(
      window.hcp066Repo,
      window.hcp066Id
    );
    return {
      status: result.status,
      signs: f.signs(),
      revision: row.ok
        ? s.publicRecordSnapshot(row.value, f.owner, window.hcp066Id)!.revision
        : -1
    };
  });
  expect(result).toEqual({ status: 'stopped', signs: 1, revision: 1 });
  expect(frames).toHaveLength(0);
});
test('competing and incomplete preference observations block explicit stored resume', async ({
  page
}) => {
  response = 'refused';
  await page.evaluate(() =>
    window.hcp066.runInboxPreferenceOperation(window.hcp066Action)
  );
  const result = await page.evaluate(async () => {
    const s = window.hcp066,
      f = window.hcp066Fixture;
    const competing = await s.resumeInboxPreferenceOperation(
      window.hcp066Repo,
      f.identity,
      f.policy,
      window.hcp066Id,
      () => Promise.resolve(f.resolve(f.competing())),
      'reviewed_stored_inbox_preference'
    );
    const partial = await s.resumeInboxPreferenceOperation(
      window.hcp066Repo,
      f.identity,
      f.policy,
      window.hcp066Id,
      () => Promise.resolve(f.resolve(undefined, false)),
      'reviewed_stored_inbox_preference'
    );
    return [competing.status, partial.status, f.signs()];
  });
  expect(result).toEqual(['conflict', 'conflict', 1]);
  expect(frames).toHaveLength(1);
});
test('actual SDK timeouts allow only three explicit same-artifact attempts and retain each named fact', async ({
  page
}) => {
  test.setTimeout(1_800_000);
  response = 'silent';
  const result = await page.evaluate(async () => {
    const s = window.hcp066,
      f = window.hcp066Fixture;
    const result = await s.runInboxPreferenceOperation(window.hcp066Action);
    const row = await s.loadPreferenceOperation(
      window.hcp066Repo,
      window.hcp066Id
    );
    const snapshot = row.ok
      ? s.publicRecordSnapshot(row.value, f.owner, window.hcp066Id)
      : null;
    return {
      status: result.status,
      signs: f.signs(),
      receipts:
        snapshot && snapshot.family !== 'public_draft' ? snapshot.receipts : []
    };
  });
  expect(result.status).toBe('needs_action');
  expect(result.signs).toBe(1);
  expect(frames).toHaveLength(3);
  expect(new Set(frames).size).toBe(1);
  expect(
    result.receipts.map((row) => [row.origin, row.attempt, row.status])
  ).toEqual(
    [1, 2, 3].map((attempt) => [
      'wss://discovery.example.org',
      attempt,
      'timed_out'
    ])
  );
});
test('Stop during the actual policy digest prevents even a new durable signing marker', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const s = window.hcp066,
      f = window.hcp066Fixture;
    const original = Reflect.get<SubtleCrypto, 'digest'>(
      crypto.subtle,
      'digest'
    );
    crypto.subtle.digest = async function (
      ...args: Parameters<SubtleCrypto['digest']>
    ) {
      const buffer = await original.call(this, ...args);
      s.stopInboxPreferenceOperation(window.hcp066Action);
      return buffer;
    };
    try {
      const result = await s.runInboxPreferenceOperation(window.hcp066Action);
      const read = await s.loadPreferenceOperation(
        window.hcp066Repo,
        window.hcp066Id
      );
      return {
        status: result.status,
        signs: f.signs(),
        revision: read.ok
          ? s.publicRecordSnapshot(read.value, f.owner, window.hcp066Id)!
              .revision
          : -1
      };
    } finally {
      crypto.subtle.digest = original;
    }
  });
  expect(result).toEqual({ status: 'stopped', signs: 0, revision: 0 });
  expect(frames).toHaveLength(0);
});
test('a real relay refusal worded Timeout is ambiguous evidence and never automatically retried', async ({
  page
}) => {
  response = 'timeout-refusal';
  const result = await page.evaluate(async () => {
    const s = window.hcp066,
      f = window.hcp066Fixture;
    const result = await s.runInboxPreferenceOperation(window.hcp066Action);
    const read = await s.loadPreferenceOperation(
      window.hcp066Repo,
      window.hcp066Id
    );
    const row = read.ok
      ? s.publicRecordSnapshot(read.value, f.owner, window.hcp066Id)
      : null;
    return {
      status: result.status,
      signs: f.signs(),
      receipts:
        row && row.family !== 'public_draft'
          ? row.receipts.map((fact) => fact.status)
          : []
    };
  });
  expect(result).toEqual({
    status: 'needs_action',
    signs: 1,
    receipts: ['unknown']
  });
  expect(frames).toHaveLength(1);
});
test('pending real publication release failure blocks terminal cleanup until the same control is released', async ({
  page
}) => {
  response = 'silent';
  await page.evaluate(() => {
    const s = window.hcp066;
    window.hcp066ReleaseFault = s.installUnsubscribeFailure();
    window.hcp066Run = s.runInboxPreferenceOperation(window.hcp066Action);
  });
  await expect.poll(() => frames.length).toBe(1);
  const result = await page.evaluate(async () => {
    const s = window.hcp066,
      pool = s.getPublicPool(window.hcp066Fixture.policy)!;
    let firstFailed = false;
    try {
      s.closePublicPool(pool);
    } catch {
      firstFailed = true;
    }
    window.hcp066ReleaseFault!.allowRelease();
    s.closePublicPool(pool);
    s.closePublicPool(pool);
    return { firstFailed, status: (await window.hcp066Run)!.status };
  });
  expect(result.firstFailed).toBe(true);
  expect(result.status).not.toBe('completed');
  expect(frames).toHaveLength(1);
});
