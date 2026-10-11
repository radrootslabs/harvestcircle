import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { finalizeEvent, getPublicKey } from 'applesauce-core/helpers';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/operation-flags.ts';
declare global {
  interface Window {
    hcp126: typeof Fixture;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let frames: string[] = [];
const sellerSecret = new Uint8Array(32).fill(84);
const seller = getPublicKey(sellerSecret);
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  await once(sockets, 'listening');
  const address = sockets.address();
  if (!address || typeof address === 'string')
    throw Error('actual loopback missing');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket) =>
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes.toString()
        : Array.isArray(bytes)
          ? Buffer.concat(bytes).toString()
          : Buffer.from(bytes).toString();
      const frame = JSON.parse(raw) as [string, { id: string }];
      if (frame[0] === 'REQ') {
        const request = JSON.parse(raw) as [
          string,
          string,
          ...{ kinds?: number[]; authors?: string[] }[]
        ];
        const [, requestId, ...filters] = request;
        if (
          filters.some(
            (f) =>
              f.kinds?.includes(30402) &&
              (!f.authors || f.authors.includes(seller))
          )
        ) {
          const event = finalizeEvent(
            {
              kind: 30402,
              created_at: 1700000060,
              content: 'Signed carrots',
              tags: [
                ['d', 'carrots'],
                ['title', 'Verified carrots'],
                ['summary', 'Actual signed listing'],
                ['published_at', '1700000000'],
                ['location', 'Victoria'],
                ['price', '3.5', 'CAD'],
                ['radroots:price_unit', 'lb'],
                ['status', 'active']
              ]
            },
            sellerSecret
          );
          socket.send(JSON.stringify(['EVENT', requestId, event]));
        }
        socket.send(JSON.stringify(['EOSE', requestId]));
      }

      if (frame[0] === 'EVENT') {
        frames.push(raw);
        socket.send(JSON.stringify(['OK', frame[1].id, true, '']));
      }
    })
  );
  const result = await build({
    configFile: false,
    plugins: [svelte({ configFile: false })],
    resolve: { conditions: ['browser'] },
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/operation-flags.ts', import.meta.url)
        ),
        name: 'hcp126',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('actual fixture bundle missing');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  frames = [];
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (String(url) !== 'wss://discovery.example.org/')
            throw Error('unexpected original fixture destination');
          super(endpoint, protocols);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
});
test('HCP126 first-level static availability is honest and cannot promise revocation of already-open tabs', async ({
  page
}) => {
  const actual = await page.locator('body').innerText();
  expect(actual).toContain('Posting is disabled in this build.');
  await expect(
    page.getByText('Posting is disabled in this build.', { exact: true })
  ).toBeVisible();
  await expect(
    page.getByText('Messaging is disabled in this build.', { exact: true })
  ).toBeVisible();
  await page.getByText('Availability limits', { exact: true }).click();
  await expect(
    page.getByText(/Static build flags do not instantly stop already-open tabs/)
  ).toBeVisible();
  await expect(
    page.getByText(/Availability flags do not establish full qualification/)
  ).toBeVisible();
  expect(frames).toHaveLength(0);
});
test('HCP126 messaging-only genuine preference action signs once after native acknowledgment and sends actual kind10050', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const s = window.hcp126,
      f = await s.makePublisherFixture({
        postingEnabled: false,
        messagingEnabled: true
      });
    let database: Parameters<typeof s.closeBrowserDatabase>[0] | undefined;
    try {
      const policy = f.policy;
      if (!policy) throw Error('genuine messaging-only policy missing');
      const reviewed = await s.reviewInboxSetup(
        f.identity,
        f.resolve(),
        policy,
        s.setupInput(),
        () => 101
      );
      if (reviewed.status !== 'review')
        return {
          stage: reviewed.status,
          reason: reviewed.reason,
          signs: f.signs()
        };
      const opened = await s.openBrowserDatabase();
      if (opened.state !== 'ready') throw Error(opened.reason);
      database = opened.owner;
      const repo = s.createPublicQuotaRepository(database, f.owner);
      if (!repo) throw Error('genuine original repo missing');
      const id = crypto.randomUUID();
      const saved = await s.saveInboxPreferenceOperation(
        repo,
        reviewed.review,
        f.resolve(),
        id,
        'reviewed_global_inbox_replacement'
      );
      if (saved.status !== 'saved' && saved.status !== 'existing')
        throw Error(saved.status);
      const prepared = await s.beginInboxPreferenceOperation(
        repo,
        f.identity,
        policy,
        reviewed.review,
        id,
        () => Promise.resolve(f.resolve()),
        'reviewed_global_inbox_replacement'
      );
      if (prepared.status !== 'prepared')
        return { stage: prepared.status, signs: f.signs() };
      const trace = s.installEffectTrace();
      try {
        const delivered = await s.runInboxPreferenceOperation(prepared.action);
        const read = await s.loadPreferenceOperation(repo, id);
        const row = read.ok && s.publicRecordSnapshot(read.value, f.owner, id);
        return {
          stage: delivered.status,
          signs: f.signs(),
          trace: trace.events(),
          artifact:
            row && row.family === 'preference_operation'
              ? row.artifact?.eventId
              : undefined
        };
      } finally {
        trace.restore();
      }
    } finally {
      f.close();
      if (database) s.closeBrowserDatabase(database);
    }
  });
  expect(result.stage).toBe('completed');
  expect(result.signs).toBe(1);
  expect(result.trace).toEqual(
    expect.arrayContaining(['ack:1', 'ack:2', 'EVENT'])
  );
  expect(result.trace?.indexOf('ack:1')).toBeLessThan(
    result.trace?.indexOf('ack:2') ?? -1
  );
  expect(result.trace?.indexOf('ack:2')).toBeLessThan(
    result.trace?.indexOf('EVENT') ?? -1
  );
  expect(frames).toHaveLength(1);
  const event = (
    JSON.parse(frames[0]) as [string, { id: string; kind: number }]
  )[1];
  expect(event.kind).toBe(10050);
  expect(event.id).toBe(result.artifact);
});

test('HCP126 posting-off stored public recovery remains needs-action with exact native record and no signature or EVENT', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const s = window.hcp126,
      f = await s.makeStoredPublicFixture(),
      identity = s.createIdentitySession();
    let db: Parameters<typeof s.closeBrowserDatabase>[0] | undefined;
    try {
      await s.connectIdentity(identity);
      await s.probeIdentityMessaging(identity, 'reviewed_self_copy');
      const opened = await s.openBrowserDatabase();
      if (opened.state !== 'ready') throw Error(opened.reason);
      db = opened.owner;
      const repo = s.createPublicPublicationRepository(db, f.owner);
      if (!repo) throw Error('genuine stored repo absent');
      const off = s.validateRelayPolicy(
        JSON.stringify({
          ...s.readRelayPolicy(f.policy),
          postingEnabled: false,
          messagingEnabled: true
        })
      );
      if (!off) throw Error('genuine disabled policy absent');
      const before = await f.details();
      const action = await s.prepareListingResume(
        repo,
        identity,
        off,
        f.command,
        'reviewed_stored_listing_resume'
      );
      const read = await s.readListingOperationView(repo, identity, f.command);
      const snapshot =
        read.status === 'found'
          ? s.listingOperationSnapshot(read.view)
          : undefined;
      if (read.status === 'found') s.closeListingOperationView(read.view);
      const after = await f.details();
      return {
        status: action.status,
        before: before.row,
        after: after.row,
        signsBefore: before.signs,
        signsAfter: after.signs,
        snapshot
      };
    } finally {
      if (db) s.closeBrowserDatabase(db);
      s.disconnectIdentity(identity);
      f.close();
    }
  });
  expect(result.status).toBe('needs_action');
  expect(result.after).toEqual(result.before);
  expect(result.signsAfter).toBe(result.signsBefore);
  expect(result.snapshot).toBeDefined();
  expect(frames).toHaveLength(0);
});
test('HCP126 actual uncertain public signing stays visible in this disabled build and is never retried automatically', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = await window.hcp126.makeUnknownPublicFixture();
    try {
      await f.holdSigning();
      await f.stopSigning();
      const before = await f.row(),
        counts = f.counts();
      const snapshot = await f.renderView();
      const after = await f.row();
      return { before, after, counts, afterCounts: f.counts(), snapshot };
    } finally {
      f.close();
    }
  });
  expect(result.after).toEqual(result.before);
  expect(result.afterCounts).toEqual(result.counts);
  expect(result.snapshot).toMatchObject({ state: 'unknown' });
  await expect(
    page.getByText('Posting is disabled in this build.', { exact: true })
  ).toBeVisible();
  expect(frames).toHaveLength(0);
});
test('HCP126 messaging-off denies actual publication while preserving explicitly prepared stored recovery', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const s = window.hcp126,
      f = await s.makeDisabledPairFixture({ messaging: false });
    try {
      const self = await s.prepareSelfRecovery(
        f.preparation,
        'reviewed_self_recovery'
      );
      if (self.status !== 'saved') throw Error(self.status);
      const pair = await s.preparePairedDelivery(
        f.preparation,
        f.context,
        'reviewed_pair_preparation'
      );
      if (pair.status !== 'prepared') throw Error(pair.status);
      const session = await s.createPrivateSession(
        f.identity,
        'reviewed_private_session'
      );
      if (!session) throw Error('genuine retained private session missing');
      const before = await f.stored(),
        counts = f.counts();
      let disabled = false;
      try {
        const pool = s.getPrivatePool(session, f.context.policy, [
          'wss://peer.example.org',
          'wss://archive.example.org'
        ]);
        if (pool) s.closePrivatePool(pool);
      } catch (error) {
        if (
          !(error instanceof Error) ||
          error.message !== 'private_pool_disabled'
        )
          throw error;
        disabled = true;
      } finally {
        s.closePrivateSession(session);
      }
      return {
        disabled,
        before,
        after: await f.stored(),
        counts,
        afterCounts: f.counts()
      };
    } finally {
      f.close();
    }
  });
  expect(result.disabled).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(result.afterCounts).toEqual(result.counts);
  expect(frames).toHaveLength(0);
});
test('HCP126 messaging-off original product request keeps unsaved text and denies actual Send before any effect', async ({
  page
}) => {
  const result = await page.evaluate(async (seller) => {
    const s = window.hcp126,
      f = await s.makeSetupFixture(),
      context = s.createPublicRuntimeContext(),
      policy = s.fixturePolicy({
        postingEnabled: true,
        messagingEnabled: false
      });
    const runtime = s.mountPublicRuntime(context, policy);
    if (!runtime) throw Error('genuine runtime absent');
    const address = s.encodeProductReference({
      kind: 30402,
      pubkey: seller,
      identifier: 'carrots'
    });
    if (!address) throw Error('original coordinate absent');
    let settled = () => {};
    const observed = new Promise<void>((resolve) => {
      settled = resolve;
    });
    const view = s.createProductView(
      s.createPublicView(runtime),
      address,
      { nowSeconds: () => 1700000200 },
      (snapshot) => {
        if (snapshot.outcome === 'active') settled();
      }
    );
    let setup: ReturnType<typeof s.createInboxSetupView>,
      controller: ReturnType<typeof s.createProductRequestController>;
    try {
      // Await the original bounded resolver's actual completed SDK request, not a fabricated listing.
      if (s.productViewSnapshot(view).outcome !== 'active') await observed;
      const acknowledgement = s.acknowledgeProductViewUncertainty(
        view,
        'reviewed_last_known_listing'
      );
      const draft = s.captureProductEnquiry(
        view,
        'reviewed_product_enquiry',
        acknowledgement
      );
      if (!draft)
        throw Error(
          'genuine verified product enquiry absent: ' +
            s.productViewSnapshot(view).outcome
        );
      setup = s.createInboxSetupView({
        identity: f.identity,
        policy,
        lookup: () => Promise.resolve(f.own.resolver)
      });
      if (!setup) throw Error('genuine original setup absent');
      controller = s.createProductRequestController(draft, runtime);
      if (
        !controller ||
        !s.configureProductRequest(
          controller,
          f.identity,
          setup,
          'reviewed_product_request_setup'
        )
      )
        throw Error('genuine request owner absent');
      const text = 'HCP126 preserve my unsaved memory text';
      if (
        !s.updateProductRequestText(controller, text, 'reviewed_private_text')
      )
        throw Error('original text update absent');
      let encrypts = 0;
      const provider = window.nostr as {
        nip44: { encrypt: (...args: unknown[]) => unknown };
      };
      const originalEncrypt = provider.nip44.encrypt;
      provider.nip44.encrypt = (...args: unknown[]) => {
        encrypts++;
        return Reflect.apply(originalEncrypt, provider.nip44, args);
      };
      const counts = f.counts();
      const delivered = await s.sendProductRequest(
        controller,
        'reviewed_product_request_send'
      );
      return {
        status: delivered.status,
        text: s.productRequestText(controller),
        snapshot: s.productRequestSnapshot(controller),
        counts,
        afterCounts: f.counts(),
        encrypts
      };
    } finally {
      if (controller) s.closeProductRequestController(controller);
      if (setup) s.closeInboxSetupView(setup);
      s.closeProductView(view);
      s.closePublicRuntime(context);
      f.close();
    }
  }, seller);
  expect(result.status).toBe('needs_action');
  expect(result.text).toBe('HCP126 preserve my unsaved memory text');
  expect(result.snapshot).toMatchObject({ readyToSend: false, dirty: true });
  expect(result.encrypts).toBe(0);
  expect(result.afterCounts).toEqual(result.counts);
  expect(frames).toHaveLength(0);
});
