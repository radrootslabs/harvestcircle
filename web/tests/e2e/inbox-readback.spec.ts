import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-readback.ts';
import type { PublicQuotaRepository } from '../../src/lib/persistence/quota.ts';
import type { BrowserDatabase } from '../../src/lib/persistence/database.ts';
import type { PrivateSession } from '../../src/lib/runtime/private-session.ts';
import type {
  PublicRuntimeContext,
  PublicView
} from '../../src/lib/runtime/public-runtime.ts';
import type { InboxReadback } from '../../src/lib/nostr/inbox-readback.ts';
declare global {
  interface Window {
    hcp067: typeof Fixture;
    hcp067Clock: ReturnType<typeof Fixture.makeReadbackClock>;
    hcp067Fixture: Awaited<ReturnType<typeof Fixture.makePublisherFixture>>;
    hcp067Repo: PublicQuotaRepository;
    hcp067Db: BrowserDatabase;
    hcp067Id: string;
    hcp067Private: PrivateSession | undefined;
    hcp067Context: PublicRuntimeContext;
    hcp067View: PublicView;
    hcp067Readback: InboxReadback;
    hcp067Restore?: () => void;
    hcp067Trace: ReturnType<typeof Fixture.installEffectTrace>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  socketServer: WebSocketServer,
  bundle: string,
  endpoint: string;
let frames: string[] = [],
  artifact: unknown,
  competing: unknown;
let response: 'artifact' | 'missing' | 'competing' | 'partial' | 'wrong_wire' =
  'artifact';
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
        frame = JSON.parse(raw) as unknown[];
      if (frame[0] === 'EVENT') {
        frames.push(raw);
        artifact = frame[1];
        const event = artifact as { id: string };
        socket.send(JSON.stringify(['OK', event.id, true, '']));
      } else if (frame[0] === 'REQ') {
        if (response !== 'missing') {
          const event =
            response === 'wrong_wire'
              ? {
                  ...(artifact as Record<string, unknown>),
                  content: 'altered unverified'
                }
              : artifact;
          socket.send(JSON.stringify(['EVENT', frame[1], event]));
        }
        if (response === 'competing')
          socket.send(JSON.stringify(['EVENT', frame[1], competing]));
        if (response !== 'partial')
          socket.send(JSON.stringify(['EOSE', frame[1]]));
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
          new URL('./harness/inbox-readback.ts', import.meta.url)
        ),
        name: 'hcp067',
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
      fixture: 'HCP067_ACTUAL_SDK_REAL_IDB_WEBLOCK_LOOPBACK',
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
test.beforeEach(async ({ page }, testInfo) => {
  frames = [];
  response = 'artifact';
  artifact = undefined;
  competing = undefined;
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      class LoopbackSocket extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (
            ![
              'wss://discovery.example.org/',
              'wss://publisher.example.org/'
            ].includes(String(url))
          )
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
  await page.evaluate(async (disjoint) => {
    const s = window.hcp067,
      f = disjoint
        ? await s.makeDisjointReadbackFixture()
        : await s.makePublisherFixture();
    window.hcp067Fixture = f;
    const opened = await s.openBrowserDatabase();
    if (opened.state !== 'ready') throw new Error(opened.reason);
    window.hcp067Db = opened.owner;
    const repo = s.createPublicQuotaRepository(opened.owner, f.owner);
    if (!repo) throw new Error('missing repo');
    window.hcp067Repo = repo;
    const reviewed = await s.reviewInboxSetup(
      f.identity,
      f.resolve(),
      f.policy,
      s.setupInput(),
      () => 101
    );
    if (reviewed.status !== 'review') throw new Error(reviewed.reason);
    const id = crypto.randomUUID();
    window.hcp067Id = id;
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
    const published = await s.runInboxPreferenceOperation(action.action);
    if (published.status !== 'completed') throw new Error(published.status);
    window.hcp067Private = await s.createPrivateSession(
      f.identity,
      'reviewed_private_session'
    );
    const context = s.createPublicRuntimeContext(),
      clock = s.makeReadbackClock(),
      runtime = s.mountPublicRuntime(context, f.policy, clock.clock);
    window.hcp067Clock = clock;
    if (!runtime) throw new Error('missing runtime');
    window.hcp067Context = context;
    window.hcp067View = s.createPublicView(runtime);
  }, testInfo.title.includes('distinct discovery and publication origins'));
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => {
      window.hcp067Restore?.();
      window.hcp067Trace?.restore();
      if (window.hcp067Readback)
        window.hcp067.closeInboxReadback(window.hcp067Readback);
      if (window.hcp067Context)
        window.hcp067.closePublicRuntime(window.hcp067Context);
      if (window.hcp067Private)
        window.hcp067.closePrivateSession(window.hcp067Private);
      window.hcp067Fixture?.close();
      if (window.hcp067Db) window.hcp067.closeBrowserDatabase(window.hcp067Db);
    });
});

async function openReadback(page: Page, settle = true) {
  await page.evaluate(async () => {
    const s = window.hcp067,
      f = window.hcp067Fixture,
      read = await s.loadPreferenceOperation(
        window.hcp067Repo,
        window.hcp067Id
      );
    if (!read.ok) throw new Error(read.reason);
    const run = s.beginPublicViewRun(window.hcp067View);
    const reader = s.resolveInboxReadback(
      window.hcp067View,
      run,
      read.value,
      f.owner,
      window.hcp067Id,
      f.policy
    );
    if (!reader) throw new Error('missing reader');
    window.hcp067Readback = reader;
  });
  if (settle)
    await page.waitForFunction(
      () =>
        window.hcp067.inboxReadbackSnapshot(window.hcp067Readback)?.status !==
        'pending'
    );
}
async function verify(page: Page, qualified = true) {
  return await page.evaluate(async (qualified) => {
    const s = window.hcp067,
      f = window.hcp067Fixture;
    const result = await s.verifyInboxSetupOperation(
      window.hcp067Repo,
      f.identity,
      window.hcp067Private,
      f.policy,
      window.hcp067Readback,
      qualified
        ? (context) => ({
            ...context,
            receive: 'qualified_exercised',
            archive: 'qualified_exercised',
            current: () => true
          })
        : undefined
    );
    const read = await s.loadPreferenceOperation(
        window.hcp067Repo,
        window.hcp067Id
      ),
      row = read.ok
        ? s.publicRecordSnapshot(read.value, f.owner, window.hcp067Id)
        : undefined;
    if (row?.family !== 'preference_operation')
      throw new Error('missing operation');
    return { result, row, signs: f.signs() };
  }, qualified);
}
test('named ACK alone leaves setup closed; exact actual relay readback is durable before setup projection opens', async ({
  page
}) => {
  response = 'missing';
  await openReadback(page);
  let result = await verify(page);
  expect(result.result.setupComplete).toBe(false);
  expect(result.row?.receipts.every((f) => f.readbackWire === null)).toBe(true);
  response = 'artifact';
  await openReadback(page);
  result = await verify(page);
  expect(result.result.status).toBe('verified');
  expect(result.result.setupComplete).toBe(true);
  expect(result.result.newListingReady).toBe(true);
  expect(result.result.sendReady).toBe(false);
  expect(result.signs).toBe(1);
  expect(
    result.row?.receipts.filter((f) => f.readbackWire !== null)
  ).toHaveLength(1);
  expect(frames).toHaveLength(1);
});
test('exact readback without qualified exercised access does not claim setup or perpetual availability', async ({
  page
}) => {
  await openReadback(page);
  const result = await verify(page, false);
  expect(result.result.status).toBe('access_unavailable');
  expect(result.result.setupComplete).toBe(false);
  expect(result.result.newListingReady).toBe(false);
  expect(frames).toHaveLength(1);
});
test('competing newest preference remains visible while exact artifact readback is retained separately', async ({
  page
}) => {
  competing = await page.evaluate(() => window.hcp067Fixture.competing());
  response = 'competing';
  await openReadback(page);
  const result = await verify(page);
  expect(result.result.status).toBe('conflict');
  expect(result.result.setupComplete).toBe(false);
  expect(result.result.knownHead?.id).toBe((competing as { id: string }).id);
  expect(
    result.row?.receipts.filter((f) => f.readbackWire !== null)
  ).toHaveLength(1);
  expect(result.signs).toBe(1);
  expect(frames).toHaveLength(1);
});
test('real IDB readback receipt failure prevents setup completion', async ({
  page
}) => {
  await openReadback(page);
  await page.evaluate(() => {
    window.hcp067Restore = window.hcp067.failPreferenceWrite(4);
  });
  const result = await verify(page);
  expect(result.result.status).toBe('storage_failed');
  expect(result.result.setupComplete).toBe(false);
  expect(result.row?.revision).toBe(3);
  expect(result.row?.receipts.every((f) => f.readbackWire === null)).toBe(true);
  expect(frames).toHaveLength(1);
});
test('partial discovery and invalid signed readback cannot open setup gates', async ({
  page
}) => {
  response = 'partial';
  await openReadback(page, false);
  let result = await verify(page);
  expect(result.result.setupComplete).toBe(false);
  expect(result.result.newListingReady).toBe(false);
  response = 'wrong_wire';
  await openReadback(page);
  result = await verify(page);
  expect(result.result.setupComplete).toBe(false);
  expect(result.row?.receipts.every((f) => f.readbackWire === null)).toBe(true);
  expect(frames).toHaveLength(1);
});
test('fresh extension owner mismatch stops readback verification without another write or signature', async ({
  page
}) => {
  await openReadback(page);
  await page.evaluate(() => window.hcp067Fixture.changeOwner());
  const result = await verify(page);
  expect(result.result.setupComplete).toBe(false);
  expect(result.row?.revision).toBe(3);
  expect(result.signs).toBe(1);
  expect(frames).toHaveLength(1);
});
test('already compatible configuration projects readiness without a redundant preference write', async ({
  page
}) => {
  await openReadback(page);
  const result = await page.evaluate(() => {
    const s = window.hcp067,
      f = window.hcp067Fixture;
    window.hcp067Trace = s.installEffectTrace();
    const own = s.inboxReadbackEvidence(window.hcp067Readback)?.resolver;
    if (!own) throw new Error('missing actual compatible resolver');
    const result = s.existingInboxSetupSnapshot(
      f.identity,
      window.hcp067Private,
      f.policy,
      own,
      (context) => ({
        ...context,
        receive: 'qualified_exercised',
        archive: 'qualified_exercised',
        current: () => true
      })
    );
    return { result, events: window.hcp067Trace.events(), signs: f.signs() };
  });
  expect(result.result.setupComplete).toBe(true);
  expect(result.events).toEqual([]);
  expect(result.signs).toBe(1);
  expect(frames).toHaveLength(1);
});

test('final readback clock invalidating exercised access leaves setup and listing gates closed', async ({
  page
}) => {
  await openReadback(page);
  const observed = await page.evaluate(async () => {
    const s = window.hcp067,
      f = window.hcp067Fixture;
    let qualified = true,
      invalidated = false;
    const result = await s.verifyInboxSetupOperation(
      window.hcp067Repo,
      f.identity,
      window.hcp067Private,
      f.policy,
      window.hcp067Readback,
      (context) => ({
        ...context,
        receive: 'qualified_exercised',
        archive: 'qualified_exercised',
        current: () => {
          window.hcp067Clock.once(() => {
            qualified = false;
            invalidated = true;
          });
          return qualified;
        }
      })
    );
    return { result, invalidated, qualified, signs: f.signs() };
  });
  expect(observed.invalidated).toBe(true);
  expect(observed.qualified).toBe(false);
  expect(observed.result.status).toBe('access_unavailable');
  expect(observed.result.setupComplete).toBe(false);
  expect(observed.result.newListingReady).toBe(false);
  expect(observed.result.sendReady).toBe(false);
  expect(observed.signs).toBe(1);
  expect(frames).toHaveLength(1);
});

test('distinct discovery and publication origins retain separate ACK and readback provenance before setup opens', async ({
  page
}) => {
  await openReadback(page);
  const observed = await verify(page);
  expect(observed.result.status).toBe('verified');
  expect(observed.result.setupComplete).toBe(true);
  expect(observed.result.newListingReady).toBe(true);
  expect(observed.result.sendReady).toBe(false);
  expect(observed.signs).toBe(1);
  expect(frames).toHaveLength(1);
  expect(
    observed.row.receipts.some(
      (fact) =>
        fact.origin === 'wss://publisher.example.org' &&
        fact.status === 'accepted' &&
        fact.readbackWire === null
    )
  ).toBe(true);
  const reads = observed.row.receipts.filter(
    (fact) => fact.readbackWire !== null
  );
  expect(reads).toHaveLength(1);
  expect(reads[0]).toMatchObject({
    origin: 'wss://publisher.example.org',
    readbackOrigin: 'wss://discovery.example.org',
    status: 'accepted'
  });
  expect(
    observed.row.receipts.every(
      (fact) => fact.origin === 'wss://publisher.example.org'
    )
  ).toBe(true);
});
