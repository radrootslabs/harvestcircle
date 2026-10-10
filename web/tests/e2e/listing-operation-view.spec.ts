import { expect, test } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/listing-operation-view.ts';
declare global {
  interface Window {
    hcp114: typeof Fixture;
    hcp114Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
    hcp114Reopened: Awaited<ReturnType<typeof Fixture.reopen>>;
  }
}
const first = 'wss://discovery.example.org',
  second = 'wss://second.example.org';
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  bundle: string,
  endpoint: string;
let frames: { origin: string; message: unknown[] }[] = [];
let stored: Record<string, unknown> | undefined;
let responses = new Map<
  string,
  'accepted' | 'refused' | 'literal-timeout' | 'silent' | 'held'
>();
let holdReadback = false,
  observeExact = true;
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 262144
  });
  await once(sockets, 'listening');
  const address = sockets.address();
  if (!address || typeof address === 'string')
    throw Error('loopback unavailable');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket, request) => {
    const origin = request.url === '/b' ? second : first;
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes.toString()
        : Array.isArray(bytes)
          ? Buffer.concat(bytes).toString()
          : Buffer.from(bytes).toString();
      const message = JSON.parse(raw) as unknown[];
      frames.push({ origin, message });
      if (message[0] === 'EVENT') {
        stored = message[1] as Record<string, unknown>;
        const response = responses.get(origin) ?? 'accepted';
        if (response === 'held' || response === 'silent') return;
        socket.send(
          JSON.stringify([
            'OK',
            stored.id,
            response === 'accepted',
            response === 'accepted'
              ? ''
              : response === 'literal-timeout'
                ? 'Timeout'
                : 'blocked: controlled refusal'
          ])
        );
      } else if (message[0] === 'REQ' && !holdReadback) {
        if (stored && observeExact)
          socket.send(JSON.stringify(['EVENT', message[1], stored]));
        socket.send(JSON.stringify(['EOSE', message[1]]));
      }
    });
  });
  const built = await build({
    configFile: false,
    plugins: [svelte({ configFile: false })],
    resolve: { conditions: ['browser'] },
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/listing-operation-view.ts', import.meta.url)
        ),
        name: 'hcp114',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(built) ? built[0] : built;
  if (!('output' in output)) throw Error('bundle unavailable');
  const chunks = output.output.filter((row) => row.type === 'chunk');
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
  stored = undefined;
  responses = new Map();
  holdReadback = false;
  observeExact = true;
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      class LoopbackSocket extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const role =
            String(url) === 'wss://discovery.example.org/'
              ? '/a'
              : String(url) === 'wss://second.example.org/'
                ? '/b'
                : undefined;
          if (!role) throw Error('unfrozen controlled target');
          super(endpoint + role, protocols);
        }
      }
      window.WebSocket = LoopbackSocket;
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp114Fixture = await window.hcp114.createFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => {
      window.hcp114Reopened?.close();
      window.hcp114Fixture?.close();
    });
});

test('HCP114 unsigned prepared source is local and renders no acceptance or edit permission', async ({
  page
}) => {
  const v = await page.evaluate(() => window.hcp114Fixture.renderView());
  expect(v).toMatchObject({
    state: 'prepared',
    artifact: 'unsigned',
    acceptanceCount: 0,
    readbackCount: 0,
    localSource: 'read-only'
  });
  expect(
    await page.evaluate(() => window.hcp114Fixture.counts())
  ).toMatchObject({ signs: 0 });
  expect(frames).toEqual([]);
  await expect(page.locator('#hcp114-operation')).toContainText(
    'Saved in this browser'
  );
  await expect(page.locator('#hcp114-operation')).not.toContainText(
    'globally published'
  );
});
test('HCP114 genuine live signing differs from unsigned uncertainty and actual restart never resets it fresh', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.holdSigning());
  expect(await page.evaluate(() => window.hcp114Fixture.show())).toMatchObject({
    state: 'signing',
    artifact: 'unsigned'
  });
  expect(
    await page.evaluate(() => window.hcp114Fixture.show(false))
  ).toMatchObject({ state: 'unknown' });
  await page.evaluate(() => window.hcp114Fixture.stopSigning());
  const row = await page.evaluate(() => window.hcp114Fixture.row());
  expect(row.revision).toBe(1);
  expect(row.artifact).toBeNull();
  await page.evaluate(() => window.hcp114Fixture.close());
  await page.reload();
  await page.addScriptTag({ content: bundle });
  const result = await page.evaluate(
    async ({ owner, command }) => {
      window.hcp114Reopened = await window.hcp114.reopen(owner, command);
      return window.hcp114Reopened.result;
    },
    { owner: row.owner, command: row.id }
  );
  expect(result).toMatchObject({
    state: 'unknown',
    artifact: 'unsigned',
    revision: 1
  });
  expect(await page.evaluate(() => window.hcp114Reopened.counts())).toEqual({
    signs: 0
  });
  expect(frames).toEqual([]);
});
test('HCP114 copied disclosure cannot present original local evidence', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.signed());
  await page.evaluate(() => window.hcp114Fixture.renderView(true));
  expect(
    await page.evaluate(() => window.hcp114Fixture.copiedSnapshot())
  ).toBeUndefined();
  await expect(page.locator('#hcp114-operation')).toContainText(
    'outcome is unknown'
  );
  await expect(page.locator('#hcp114-operation')).not.toContainText(
    'Relay acceptance recorded'
  );
  expect(frames).toEqual([]);
});
test('HCP114 actual SDK acceptance by one of two targets is partial and does not invent readback', async ({
  page
}) => {
  observeExact = false;
  responses.set(second, 'refused');
  expect(await page.evaluate(() => window.hcp114Fixture.deliver())).toBe(
    'needs_action'
  );
  const v = await page.evaluate(() => window.hcp114Fixture.renderView());
  expect(v).toMatchObject({
    state: 'partial',
    acceptanceCount: 1,
    readbackCount: 0,
    needsAction: true
  });
  expect(frames.filter((x) => x.message[0] === 'EVENT')).toHaveLength(2);
  await expect(page.locator('#hcp114-operation')).toContainText(
    'Relay acceptance recorded for 1 of 2 targets.'
  );
  await expect(page.locator('#hcp114-operation')).toContainText(
    'Exact read-back recorded for 0 of 2 targets.'
  );
});
test('HCP114 actual SDK exact readback without an ACK stays separate from acceptance', async ({
  page
}) => {
  responses.set(first, 'silent');
  responses.set(second, 'refused');
  expect(await page.evaluate(() => window.hcp114Fixture.deliver())).toBe(
    'needs_action'
  );
  const v = await page.evaluate(() => window.hcp114Fixture.show());
  expect(v).toMatchObject({
    state: 'partial',
    acceptanceCount: 0,
    readbackCount: 1
  });
  expect(v && 'targets' in v && v.targets[0]).toMatchObject({
    accepted: false,
    readback: 'confirmed',
    latestOutcome: 'timed_out'
  });
});
test('HCP114 actual all-target ACKs alone are accepted with exact readback still unobserved', async ({
  page
}) => {
  observeExact = false;
  expect(await page.evaluate(() => window.hcp114Fixture.deliver())).toBe(
    'completed'
  );
  expect(await page.evaluate(() => window.hcp114Fixture.show())).toMatchObject({
    state: 'accepted',
    acceptanceCount: 2,
    readbackCount: 0
  });
});
test('HCP114 actual all-target readback is distinct from a browser-only signed artifact', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.signed());
  expect(await page.evaluate(() => window.hcp114Fixture.show())).toMatchObject({
    state: 'prepared',
    artifact: 'signed',
    acceptanceCount: 0,
    readbackCount: 0
  });
  expect(await page.evaluate(() => window.hcp114Fixture.deliver())).toBe(
    'completed'
  );
  expect(await page.evaluate(() => window.hcp114Fixture.show())).toMatchObject({
    state: 'readback',
    acceptanceCount: 2,
    readbackCount: 2
  });
});
test('HCP114 controlled later Stop and refusal retain earlier named acceptance and readback facts', async ({
  page
}) => {
  expect(await page.evaluate(() => window.hcp114Fixture.deliver())).toBe(
    'completed'
  );
  await page.evaluate(async () => {
    await window.hcp114Fixture.evidence('stopped', 0, false);
    await window.hcp114Fixture.evidence('refused', 1, false);
  });
  const v = await page.evaluate(() => window.hcp114Fixture.show());
  expect(v).toMatchObject({
    acceptanceCount: 2,
    readbackCount: 2,
    needsAction: true
  });
  expect(v && 'targets' in v && v.targets.map((t) => t.latestOutcome)).toEqual([
    'stopped',
    'refused'
  ]);
});
test('HCP114 contradictory native record fields fail closed rather than becoming prepared', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.signed());
  await page.evaluate(() =>
    window.hcp114Fixture.evidence('accepted', 0, false)
  );
  await page.evaluate(async () => {
    const row = await window.hcp114Fixture.row();
    await window.hcp114Fixture.replace({ ...row, artifact: null });
  });
  expect(await page.evaluate(() => window.hcp114Fixture.show())).toEqual({
    status: 'unavailable'
  });
  expect(frames).toEqual([]);
});
test('HCP114 retained prototype capture remains available after draft cleanup but external-head records stay read-only', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.signed());
  await page.evaluate(() => window.hcp114Fixture.removeDraft());
  expect(await page.evaluate(() => window.hcp114Fixture.show())).toMatchObject({
    localSource: 'retained-prototype'
  });
  await page.evaluate(() => window.hcp114Fixture.externalSource());
  expect(await page.evaluate(() => window.hcp114Fixture.show())).toMatchObject({
    localSource: 'read-only'
  });
  expect(frames).toEqual([]);
});
test('HCP114 Close and identity invalidation retire original presentation handles', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.signed());
  await page.evaluate(() => window.hcp114Fixture.renderView());
  await page.evaluate(() => window.hcp114Fixture.logout());
  expect(
    await page.evaluate(() => window.hcp114Fixture.snapshot())
  ).toBeUndefined();
  await expect(page.locator('#hcp114-operation')).not.toContainText(
    'Relay acceptance recorded'
  );
  await page.evaluate(() => window.hcp114Fixture.closeView());
  expect(frames).toEqual([]);
});

test('HCP114 an explicit fresh native read supersedes the earlier presentation snapshot', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.signed());
  await page.evaluate(() => window.hcp114Fixture.show());
  expect(
    await page.evaluate(() => window.hcp114Fixture.supersede())
  ).toBeUndefined();
  expect(frames).toEqual([]);
});
test('HCP114 logout after actual native transaction completion fences a held application acknowledgement', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.signed());
  expect(
    await page.evaluate(() => window.hcp114Fixture.readWhileLogout())
  ).toEqual({ result: { status: 'unavailable' }, nativeCompletions: 1 });
  expect(frames).toEqual([]);
});

test('HCP114 retained mounted signing view retires on original Stop without another native read', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.holdSigning());
  expect(
    await page.evaluate(() => window.hcp114Fixture.renderView())
  ).toMatchObject({ state: 'signing' });
  await page.evaluate(() => window.hcp114Fixture.stopSigning());
  expect(
    await page.evaluate(() => window.hcp114Fixture.snapshot())
  ).toBeUndefined();
  await expect(page.locator('#hcp114-operation')).toContainText(
    'The outcome is unknown'
  );
  await expect(page.locator('#hcp114-operation')).not.toContainText(
    'Waiting for your extension'
  );
  expect(frames).toEqual([]);
});
test('HCP114 retained mounted signing view retires on signer settlement without another native read', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.holdSigning());
  expect(
    await page.evaluate(() => window.hcp114Fixture.renderView())
  ).toMatchObject({ state: 'signing' });
  expect(await page.evaluate(() => window.hcp114Fixture.releaseSigning())).toBe(
    'prepared'
  );
  expect(
    await page.evaluate(() => window.hcp114Fixture.snapshot())
  ).toBeUndefined();
  await expect(page.locator('#hcp114-operation')).toContainText(
    'The outcome is unknown'
  );
  await expect(page.locator('#hcp114-operation')).not.toContainText(
    'Waiting for your extension'
  );
  expect(frames).toEqual([]);
});

test('HCP114 notification excludes original-token listeners subscribed during notification', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.holdSigning());
  expect(
    await page.evaluate(() => window.hcp114Fixture.observerAddition())
  ).toEqual({ first: 1, added: 0 });
  expect(frames).toEqual([]);
});
test('HCP114 notification makes original-token reentrant Stop idempotent', async ({
  page
}) => {
  await page.evaluate(() => window.hcp114Fixture.holdSigning());
  expect(
    await page.evaluate(() => window.hcp114Fixture.observerStop())
  ).toEqual({ first: 1 });
  expect(frames).toEqual([]);
});
