import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-auth.ts';
declare global {
  interface Window {
    hcp068: typeof Fixture;
    hcp068Fixture: Awaited<ReturnType<typeof Fixture.makeAuthFixture>>;
    hcp068Action: NonNullable<
      Awaited<ReturnType<typeof Fixture.beginInboxAuthentication>>
    >;
    hcp068Run?: Promise<{ status: string }>;
    hcp068Settle?: () => void;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let frames: unknown[][] = [],
  accept = true,
  holdPage = false;
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
    socket.send(JSON.stringify(['AUTH', 'challenge-one']));
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
      frames.push(frame);
      if (frame[0] === 'AUTH')
        socket.send(
          JSON.stringify([
            'OK',
            (frame[1] as { id: string }).id,
            accept,
            accept ? '' : 'blocked: refusal'
          ])
        );
      if (frame[0] === 'REQ' && !holdPage)
        socket.send(JSON.stringify(['EOSE', frame[1]]));
    });
  });
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/inbox-auth.ts', import.meta.url)
        ),
        name: 'hcp068',
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
  frames = [];
  accept = true;
  holdPage = false;
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (
            ![
              'wss://inbox.example.org/',
              'wss://discovery.example.org/'
            ].includes(String(url))
          )
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
    window.hcp068Fixture = await window.hcp068.makeAuthFixture();
    const action = await window.hcp068.beginInboxAuthentication(
      window.hcp068Fixture.pool,
      window.hcp068.inbox,
      'reviewed_connection_auth'
    );
    if (!action) throw new Error('missing AUTH action');
    window.hcp068Action = action;
  });
  await expect
    .poll(() =>
      page.evaluate(
        () => window.hcp068.inboxAuthSnapshot(window.hcp068Action)?.state
      )
    )
    .toBe('challenge');
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => {
      window.hcp068Settle?.();
      window.hcp068Fixture.close();
    });
});
test('genuine SDK connection waits without REQ or EVENT and exact signed22242 uses AUTH only', async ({
  page
}) => {
  expect(frames).toHaveLength(0);
  const result = await page.evaluate(async () => ({
    result: await window.hcp068.respondInboxAuthentication(window.hcp068Action),
    owner: window.hcp068Fixture.owner,
    signs: window.hcp068Fixture.signs()
  }));
  expect(result.result.status).toBe('accepted');
  expect(result.signs).toBe(1);
  expect(frames).toHaveLength(1);
  expect(frames[0][0]).toBe('AUTH');
  expect(frames[0][1]).toMatchObject({
    kind: 22242,
    pubkey: result.owner,
    content: '',
    tags: [
      ['relay', 'wss://inbox.example.org/'],
      ['challenge', 'challenge-one']
    ]
  });
});
for (const mode of [
  'relay',
  'challenge',
  'author',
  'cached',
  'denied'
] as const)
  test(`actual provider ${mode} response never reaches AUTH`, async ({
    page
  }) => {
    const result = await page.evaluate(async (mode) => {
      window.hcp068Fixture.mode(mode);
      return window.hcp068.respondInboxAuthentication(window.hcp068Action);
    }, mode);
    expect(result.status).not.toBe('accepted');
    expect(frames.filter((x) => x[0] === 'AUTH')).toHaveLength(0);
  });
test('real refusal pauses without retry', async ({ page }) => {
  accept = false;
  expect(
    await page.evaluate(() =>
      window.hcp068.respondInboxAuthentication(window.hcp068Action)
    )
  ).toEqual({ status: 'refused' });
  await page.evaluate(() =>
    window.hcp068.respondInboxAuthentication(window.hcp068Action)
  );
  expect(frames.filter((x) => x[0] === 'AUTH')).toHaveLength(1);
});
test('two genuine changing challenges admit two responses and the third closes the scope', async ({
  page
}) => {
  await page.evaluate(() =>
    window.hcp068.respondInboxAuthentication(window.hcp068Action)
  );
  for (const socket of sockets.clients)
    socket.send(JSON.stringify(['AUTH', 'challenge-two']));
  await expect
    .poll(() =>
      page.evaluate(
        () => window.hcp068.inboxAuthSnapshot(window.hcp068Action)?.state
      )
    )
    .toBe('challenge');
  await page.evaluate(() =>
    window.hcp068.respondInboxAuthentication(window.hcp068Action)
  );
  for (const socket of sockets.clients)
    socket.send(JSON.stringify(['AUTH', 'challenge-three']));
  await expect
    .poll(() =>
      page.evaluate(
        () => window.hcp068.inboxAuthSnapshot(window.hcp068Action)?.state
      )
    )
    .toBe('stopped');
  await page.evaluate(() =>
    window.hcp068.respondInboxAuthentication(window.hcp068Action)
  );
  expect(frames.filter((x) => x[0] === 'AUTH')).toHaveLength(2);
  expect(await page.evaluate(() => window.hcp068Fixture.signs())).toBe(2);
});
for (const stop of ['stop', 'logout', 'socket'] as const)
  test(`actual pending signer retains shared slot through ${stop} and cannot send late AUTH`, async ({
    page
  }) => {
    await page.evaluate(() => {
      window.hcp068Fixture.beforeSign(
        () =>
          new Promise<void>((resolve) => {
            window.hcp068Settle = resolve;
          })
      );
      window.hcp068Run = window.hcp068.respondInboxAuthentication(
        window.hcp068Action
      );
    });
    await expect
      .poll(() => page.evaluate(() => window.hcp068Fixture.signs()))
      .toBe(1);
    if (stop === 'socket') for (const socket of sockets.clients) socket.close();
    else
      await page.evaluate((stop) => {
        if (stop === 'logout') window.hcp068Fixture.logout();
        else window.hcp068.closeInboxAuthentication(window.hcp068Action);
      }, stop);
    await expect
      .poll(() =>
        page.evaluate(
          () => window.hcp068.inboxAuthSnapshot(window.hcp068Action)?.state
        )
      )
      .toBe('stopped');
    expect(
      await page.evaluate(
        () =>
          window.hcp068.extensionSchedulerSnapshot(
            window.hcp068.browserExtensionScheduler()!
          ).state
      )
    ).toBe('active');
    const result = await page.evaluate(async () => {
      window.hcp068Settle!();
      return window.hcp068Run;
    });
    expect(result?.status).not.toBe('accepted');
    expect(frames.filter((x) => x[0] === 'AUTH')).toHaveLength(0);
    expect(
      await page.evaluate(
        () =>
          window.hcp068.extensionSchedulerSnapshot(
            window.hcp068.browserExtensionScheduler()!
          ).state
      )
    ).toBe('idle');
  });
test('unsolicited public challenges never sign and public reads survive private Stop', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const s = window.hcp068,
      f = window.hcp068Fixture;
    s.closeInboxAuthentication(window.hcp068Action);
    const pool = s.getPublicPool(f.policy)!;
    let release = () => {};
    await new Promise<void>((resolve) => {
      release = s.subscribePublicPool(
        pool,
        [{ kinds: [30402], limit: 1 }],
        (message) => {
          if (message.type === 'EOSE') resolve();
        }
      );
    });
    release();
    s.closePublicPool(pool);
    return f.signs();
  });
  expect(result).toBe(0);
  expect(frames.some((x) => x[0] === 'REQ')).toBe(true);
  expect(frames.some((x) => x[0] === 'AUTH' || x[0] === 'EVENT')).toBe(false);
});
test('replacement during an outstanding SDK signer stops without rebinding the response', async ({
  page
}) => {
  await page.evaluate(() => {
    window.hcp068Fixture.beforeSign(
      () =>
        new Promise<void>((resolve) => {
          window.hcp068Settle = resolve;
        })
    );
    window.hcp068Run = window.hcp068.respondInboxAuthentication(
      window.hcp068Action
    );
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp068Fixture.signs()))
    .toBe(1);
  for (const socket of sockets.clients)
    socket.send(JSON.stringify(['AUTH', 'replacement']));
  await expect
    .poll(() =>
      page.evaluate(
        () => window.hcp068.inboxAuthSnapshot(window.hcp068Action)?.state
      )
    )
    .toBe('stopped');
  const result = await page.evaluate(async () => {
    window.hcp068Settle!();
    return window.hcp068Run;
  });
  expect(result?.status).not.toBe('accepted');
  expect(frames.filter((x) => x[0] === 'AUTH')).toHaveLength(0);
});
test('fresh SDK owner mismatch stops before the signer', async ({ page }) => {
  const result = await page.evaluate(async () => {
    window.hcp068Fixture.changeOwner();
    const result = await window.hcp068.respondInboxAuthentication(
      window.hcp068Action
    );
    return { result, signs: window.hcp068Fixture.signs() };
  });
  expect(result.result.status).not.toBe('accepted');
  expect(result.signs).toBe(0);
  expect(frames.filter((x) => x[0] === 'AUTH')).toHaveLength(0);
});
test('oversized genuine challenge stops bounded post-parse work without prompting', async ({
  page
}) => {
  for (const socket of sockets.clients)
    socket.send(JSON.stringify(['AUTH', 'x'.repeat(4097)]));
  await expect
    .poll(() =>
      page.evaluate(
        () => window.hcp068.inboxAuthSnapshot(window.hcp068Action)?.state
      )
    )
    .toBe('stopped');
  expect(await page.evaluate(() => window.hcp068Fixture.signs())).toBe(0);
});
test('new physical connection and explicit action do not inherit accepted scope', async ({
  page
}) => {
  await page.evaluate(() =>
    window.hcp068.respondInboxAuthentication(window.hcp068Action)
  );
  await page.evaluate(async () => {
    window.hcp068Fixture.close();
    window.hcp068Fixture = await window.hcp068.makeAuthFixture();
    const action = await window.hcp068.beginInboxAuthentication(
      window.hcp068Fixture.pool,
      window.hcp068.inbox,
      'reviewed_connection_auth'
    );
    if (!action) throw new Error('missing fresh action');
    window.hcp068Action = action;
  });
  await expect
    .poll(() =>
      page.evaluate(
        () => window.hcp068.inboxAuthSnapshot(window.hcp068Action)?.state
      )
    )
    .toBe('challenge');
  expect(
    await page.evaluate(
      () => window.hcp068.inboxAuthSnapshot(window.hcp068Action)?.responses
    )
  ).toBe(0);
  await page.evaluate(() =>
    window.hcp068.respondInboxAuthentication(window.hcp068Action)
  );
  expect(frames.filter((x) => x[0] === 'AUTH')).toHaveLength(2);
  expect(await page.evaluate(() => window.hcp068Fixture.signs())).toBe(1);
});
test('failed physical close retains unavailable ownership until the same cleanup retries', async ({
  page
}) => {
  const result = await page.evaluate(() => {
    const s = window.hcp068,
      fault = s.installCloseFailure();
    let closeFailed = false,
      reopenFailed = false;
    try {
      try {
        s.closeInboxAuthentication(window.hcp068Action);
      } catch {
        closeFailed = true;
      }
      try {
        window.hcp068Fixture.reopen();
      } catch {
        reopenFailed = true;
      }
      fault.allow();
      s.closeInboxAuthentication(window.hcp068Action);
      return { closeFailed, reopenFailed, signs: window.hcp068Fixture.signs() };
    } finally {
      fault.restore();
    }
  });
  expect(result).toEqual({ closeFailed: true, reopenFailed: true, signs: 0 });
});
test('raw private AUTH ports cannot bypass explicit review and guarded signing', async ({
  page
}) => {
  const status = await page.evaluate(async () => {
    window.hcp068Fixture.close();
    window.hcp068Fixture = await window.hcp068.makeAuthFixture();
    return window.hcp068Fixture.rawPortBypass();
  });
  expect(status).not.toBe('accepted');
  expect(frames.filter((x) => x[0] === 'AUTH')).toHaveLength(0);
});
test('late extension owner change during signing prevents the AUTH effect', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    window.hcp068Fixture.beforeSign(() => {
      window.hcp068Fixture.changeOwner();
      return Promise.resolve();
    });
    return window.hcp068.respondInboxAuthentication(window.hcp068Action);
  });
  expect(result.status).not.toBe('accepted');
  expect(frames.filter((x) => x[0] === 'AUTH')).toHaveLength(0);
});
test('reviewed AUTH uses the same genuine already-open private page connection', async ({
  page
}) => {
  holdPage = true;
  await page.evaluate(async () => {
    window.hcp068Fixture.close();
    window.hcp068Fixture = await window.hcp068.makeAuthFixture();
    window.hcp068Fixture.startPage();
  });
  await expect.poll(() => frames.filter((x) => x[0] === 'REQ').length).toBe(1);
  const result = await page.evaluate(async () => {
    const action = await window.hcp068.beginInboxAuthentication(
      window.hcp068Fixture.pool,
      window.hcp068.inbox,
      'reviewed_connection_auth'
    );
    if (!action) return { status: 'missing' };
    window.hcp068Action = action;
    return window.hcp068.respondInboxAuthentication(action);
  });
  expect(result.status).toBe('accepted');
  expect(frames.filter((x) => x[0] === 'REQ')).toHaveLength(1);
  expect(frames.filter((x) => x[0] === 'AUTH')).toHaveLength(1);
});
