import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer, WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import { createInboxPolicy } from '../integration/harness/inbox-policy.ts';
import type * as Fixture from './harness/inbox-policy.ts';
declare global {
  interface Window {
    hcp069: typeof Fixture;
    hcp069Fixture: Awaited<ReturnType<typeof Fixture.makePolicyFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let policy: ReturnType<typeof createInboxPolicy>;
let verbs: string[] = [];
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
    throw new Error('missing loopback');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket) => {
    const connection = policy.connect('loopback-challenge');
    socket.send(JSON.stringify(['AUTH', 'loopback-challenge']));
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const text = raw.toString();
      try {
        const frame: unknown = JSON.parse(text);
        if (Array.isArray(frame) && typeof frame[0] === 'string')
          verbs.push(frame[0]);
      } catch {
        // Malformed fixture peers still reach the bounded policy NOTICE below.
      }
      for (const response of connection.frame(text))
        socket.send(JSON.stringify(response));
    });
    socket.on('close', () => connection.close());
  });
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/inbox-policy.ts', import.meta.url)
        ),
        name: 'hcp069',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing bundle');
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
  policy = createInboxPolicy({
    now: Math.floor(Date.now() / 1000),
    retentionSeconds: 30 * 86400
  });
  verbs = [];
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (String(url) !== 'wss://inbox.example.org/')
            throw new Error('unexpected fixture destination');
          super(endpoint, protocols);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp069Fixture = await window.hcp069.makePolicyFixture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed()) await page.evaluate(() => window.hcp069Fixture.close());
});
test('actual SDK accepts separately signed recipient/self wraps and recipient-only backdated reads', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcp069Fixture,
      sender = await f.connect('sender'),
      recipient = await f.connect('recipient');
    return {
      owner: f.owner,
      peer: f.peer,
      wraps: f.wraps,
      toPeer: await f.write(sender, f.wraps.recipient),
      toSelf: await f.write(sender, f.wraps.archive),
      wrong: await f.read(sender, f.peer),
      archive: await f.read(sender, f.owner),
      inbox: await f.read(recipient, f.peer)
    };
  });
  expect(result.wraps.recipient.pubkey).not.toBe(result.owner);
  expect(result.wraps.archive.pubkey).not.toBe(result.owner);
  expect(result.wraps.archive.pubkey).not.toBe(result.wraps.recipient.pubkey);
  expect(result.toPeer.accepted).toBe(true);
  expect(result.toSelf.accepted).toBe(true);
  expect(result.wrong.status).toBe('refused');
  expect(result.wrong.ids).toEqual([]);
  expect(result.archive.ids).toEqual([result.wraps.archive.id]);
  expect(result.inbox.ids).toEqual([result.wraps.recipient.id]);
  expect(result.wraps.recipient.created_at).toBeLessThanOrEqual(
    Math.floor(Date.now() / 1000) - 48 * 3600 - 300
  );
  expect(policy.size()).toBe(2);
  expect(verbs.filter((v) => v === 'AUTH')).toHaveLength(2);
  expect(verbs.filter((v) => v === 'EVENT')).toHaveLength(2);
});
test('anonymous p-target request is refused rather than an empty successful inbox', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcp069Fixture,
      c = await f.connect('recipient', false);
    return f.read(c, f.peer);
  });
  expect(result).toEqual({ status: 'refused', ids: [] });
  expect(verbs.filter((v) => v === 'AUTH')).toHaveLength(0);
  expect(verbs.filter((v) => v === 'REQ')).toHaveLength(1);
});
test('actual SDK surfaces incompatible outer-author write policy with no alternate publication', async ({
  page
}) => {
  policy = createInboxPolicy({
    now: Math.floor(Date.now() / 1000),
    retentionSeconds: 30 * 86400,
    requireOuterAuthor: true
  });
  const result = await page.evaluate(async () => {
    const f = window.hcp069Fixture,
      c = await f.connect('sender');
    return f.write(c, f.wraps.recipient);
  });
  expect(result.accepted).toBe(false);
  expect(policy.size()).toBe(0);
  expect(verbs.filter((v) => v === 'EVENT')).toHaveLength(1);
});

test('malformed loopback peer frame is isolated before a normal SDK write', async ({
  page
}) => {
  const peer = new WebSocket(endpoint);
  try {
    await once(peer, 'open');
    const reply = new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error('fixture NOTICE missing')),
        2000
      );
      peer.on('message', (bytes) => {
        const raw = Buffer.isBuffer(bytes)
          ? bytes
          : Array.isArray(bytes)
            ? Buffer.concat(bytes)
            : Buffer.from(bytes);
        const value: unknown = JSON.parse(raw.toString());
        if (Array.isArray(value) && value[0] === 'NOTICE') {
          clearTimeout(timer);
          resolve(value);
        }
      });
    });
    peer.send('{');
    expect(await reply).toEqual(['NOTICE', 'invalid: fixture frame']);
  } finally {
    peer.terminate();
  }
  const result = await page.evaluate(async () => {
    const f = window.hcp069Fixture,
      c = await f.connect('sender');
    return f.write(c, f.wraps.recipient);
  });
  expect(result.accepted).toBe(true);
  expect(policy.size()).toBe(1);
});
