import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/account-isolation.ts';
declare global {
  interface Window {
    hcp137: typeof Fixture;
    hcp137Thread?: Awaited<ReturnType<typeof Fixture.thread.renderFixture>>;
    hcp137Auth?: Awaited<ReturnType<typeof Fixture.auth.makeAuthFixture>>;
    hcp137OldAuth?: Awaited<ReturnType<typeof Fixture.auth.makeAuthFixture>>;
    hcp137Pending?: Promise<boolean>;
    hcp137Action?: NonNullable<
      Awaited<ReturnType<typeof Fixture.auth.beginInboxAuthentication>>
    >;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let authOwners: string[] = [],
  closedOwners: string[] = [];
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  await once(sockets, 'listening');
  const address = sockets.address();
  if (!address || typeof address === 'string') throw Error('missing loopback');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket) => {
    let authenticatedOwner: string | undefined;
    socket.send(JSON.stringify(['AUTH', 'hcp137-current-connection']));
    socket.on('close', () => {
      if (authenticatedOwner) closedOwners.push(authenticatedOwner);
    });
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
      if (frame[0] === 'AUTH') {
        const event = frame[1] as { id: string; pubkey: string };
        authenticatedOwner = event.pubkey;
        authOwners.push(event.pubkey);
        socket.send(JSON.stringify(['OK', event.id, true, '']));
      }
      if (frame[0] === 'REQ') socket.send(JSON.stringify(['EOSE', frame[1]]));
      if (frame[0] === 'EVENT')
        socket.send(
          JSON.stringify(['OK', (frame[1] as { id: string }).id, true, ''])
        );
    });
  });
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    plugins: [svelte({ configFile: false })],
    resolve: {
      conditions: ['browser'],
      alias: {
        '$app/navigation': fileURLToPath(
          new URL('./harness/conversation-thread.ts', import.meta.url)
        )
      }
    },
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/account-isolation.ts', import.meta.url)
        ),
        name: 'hcp137',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('missing actual bundle');
  const chunks = output.output.filter((item) => item.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP137_ORIGINAL_ACCOUNT_CONSUMERS',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'Actual Chromium/native storage/SDK signed AUTH on controlled loopback; dummy AUTH self probe and original genuine thread decryption are distinct; no installed extension/deployed host Q.'
    })
  );
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  authOwners = [];
  closedOwners = [];
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (
            ![
              'wss://peer.example.org',
              'wss://archive.example.org',
              'wss://discovery.example.org',
              'wss://inbox.example.org'
            ].includes(new URL(String(url)).origin)
          )
            throw Error('unapproved fixture destination');
          super(endpoint, protocols);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(async () => {
      window.hcp137Thread?.settle();
      await window.hcp137Pending;
      window.hcp137Auth?.close();
      window.hcp137OldAuth?.close();
      await window.hcp137Thread?.close();
    });
});
async function successor(page: Page) {
  await page.evaluate(async () => {
    const s = window.hcp137.auth,
      f = window.hcp137Auth;
    const current = f ?? (await s.makeAuthFixture());
    window.hcp137Auth = current;
    const action = await s.beginInboxAuthentication(
      current.pool,
      s.inbox,
      'reviewed_connection_auth'
    );
    if (!action) throw Error('missing successor AUTH');
    window.hcp137Action = action;
  });
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.hcp137Action &&
          window.hcp137.auth.inboxAuthSnapshot(window.hcp137Action)?.state
      )
    )
    .toBe('challenge');
  const action = await page.evaluate(async () => {
    const s = window.hcp137.auth,
      f = window.hcp137Auth,
      action = window.hcp137Action;
    if (!f || !action) throw Error('missing actual successor action');
    return {
      status: (await s.respondInboxAuthentication(action)).status,
      owner: f.owner,
      ownership: f.ownership()
    };
  });
  expect(action.status).toBe('accepted');
  expect(action.ownership.private).toBe(true);
  expect(action.ownership.identity).toBe(true);
  return action;
}
for (const held of [false, true])
  test(`${held ? 'held late decrypt' : 'rendered thread'} cannot disclose or write into a different accepted account`, async ({
    page
  }) => {
    await page.evaluate(async () => {
      window.hcp137Thread = await window.hcp137.thread.renderFixture();
    });
    await page
      .getByRole('button', { name: 'Check current preference' })
      .click();
    if (held) {
      await page.evaluate(() => {
        const f = window.hcp137Thread;
        if (!f) throw Error('missing original thread');
        f.mode('hold_decrypt');
        window.hcp137Pending = f.unlock();
      });
      await expect
        .poll(() => page.evaluate(() => window.hcp137Thread?.pending()))
        .toBe(true);
    } else {
      await page
        .getByRole('button', { name: 'Unlock messages', exact: true })
        .click();
      await expect(
        page.getByText('Actual encrypted thread fixture', { exact: true })
      ).toBeVisible();
    }
    const original = await page.evaluate(async () => {
      const f = window.hcp137Thread;
      if (!f) throw Error('missing original thread');
      const native = await f.native();
      f.logout();
      return { owner: f.owner, native, id: f.conversationId };
    });
    await expect(page.locator('[data-message-id]')).toHaveCount(0);
    if (held) {
      // The page-local provider slot stays occupied until the original prompt settles.
      expect(await page.evaluate(() => window.hcp137Thread?.pending())).toBe(
        true
      );
      await page.evaluate(() => window.hcp137Thread?.settle());
      expect(await page.evaluate(() => window.hcp137Pending)).toBe(false);
    }
    const next = await successor(page);
    expect(next.owner).not.toBe(original.owner);
    expect(authOwners).toEqual([next.owner]);
    const after = await page.evaluate(async () => {
      const f = window.hcp137Thread;
      if (!f) throw Error('missing stale consumer');
      return {
        resolve: await f.resolve(f.conversationId),
        thread: f.thread(),
        native: await f.native(),
        current: window.hcp137Auth?.ownership()
      };
    });
    expect(after.resolve).toBe(false);
    expect(after.thread.status).not.toBe('ready');
    expect(after.native).toEqual(original.native);
    expect(after.current?.identity).toBe(true);
    expect(after.current?.private).toBe(true);
    await expect(page.locator('[data-message-id]')).toHaveCount(0);
    await expect(
      page.getByText('Actual encrypted thread fixture', { exact: true })
    ).toHaveCount(0);
    expect(page.url()).not.toContain(original.owner);
    expect(page.url()).not.toContain(next.owner);
    await expect(page).toHaveTitle('HarvestCircle');
  });
test('logout closes original private AUTH socket and clears cache before different-account AUTH', async ({
  page
}) => {
  const first = await successor(page);
  await page.evaluate(() => {
    window.hcp137OldAuth = window.hcp137Auth;
    window.hcp137Auth = undefined;
    window.hcp137OldAuth?.logout();
  });
  await expect.poll(() => closedOwners).toContain(first.owner);
  const old = await page.evaluate(() => window.hcp137OldAuth?.ownership());
  expect(old?.identity).toBe(false);
  expect(old?.private).not.toBe(true);
  expect(old?.cache).toEqual({ closed: true, count: 0, bytes: 0 });
  const next = await successor(page);
  expect(next.owner).not.toBe(first.owner);
  expect(authOwners).toEqual([first.owner, next.owner]);
  const stable = await page.evaluate(() => ({
    old: window.hcp137OldAuth?.ownership(),
    current: window.hcp137Auth?.ownership()
  }));
  expect(stable.old).toEqual(old);
  expect(stable.current?.private).toBe(true);
  expect(stable.current?.identity).toBe(true);
});
