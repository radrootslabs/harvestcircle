import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-access-controller.ts';
declare global {
  interface Window {
    cr007: typeof Fixture;
    cr007Fixture: Awaited<ReturnType<typeof Fixture.makeControllerFixture>>;
    cr007Controller: NonNullable<
      Awaited<ReturnType<Window['cr007Fixture']['capture']>>
    >;
    cr007Pending?: Promise<string>;
    cr007Release?: () => void;
    cr007Waiting?: boolean;
    cr007Open?: boolean;
    cr007StopLive?: () => void;
    cr007StopPage?: () => void;
    cr007PageEnd?: string;
    cr007LiveEvents?: number;
    cr007DelayClose?: boolean;
    cr007HeldClose?: (() => void)[];
    cr007ThrowConstructor?: string;
    cr007Reconnected?: boolean;
    cr007Fault?: ReturnType<typeof Fixture.installCloseFailure>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let head = '',
  accept = true;
const frames: { origin: string; frame: unknown[] }[] = [];
const connections = new Map<WebSocket, string>();
let peakPrivateConnections = 0,
  constructedPrivateConnections = 0;
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  await once(sockets, 'listening');
  const address = sockets.address();
  if (!address || typeof address === 'string') throw Error('owned loopback');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket, request) => {
    const origin =
      new URL(request.url ?? '/', endpoint).searchParams.get('origin') ?? '';
    connections.set(socket, origin);
    if (origin !== 'wss://discovery.example.org') {
      constructedPrivateConnections++;
      peakPrivateConnections = Math.max(
        peakPrivateConnections,
        [...connections.values()].filter(
          (from) => from !== 'wss://discovery.example.org'
        ).length
      );
    }
    socket.on('close', () => connections.delete(socket));
    socket.send(JSON.stringify(['AUTH', 'cr007-controlled-challenge']));
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
      frames.push({ origin, frame });
      if (frame[0] === 'AUTH')
        socket.send(
          JSON.stringify([
            'OK',
            (frame[1] as { id: string }).id,
            accept,
            accept ? '' : 'blocked: controlled refusal'
          ])
        );
      if (frame[0] === 'REQ' && origin === 'wss://discovery.example.org') {
        const filter = frame[2] as { kinds?: number[] };
        if (filter.kinds?.includes(10050) && head)
          socket.send(JSON.stringify(['EVENT', frame[1], JSON.parse(head)]));
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      }
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
          new URL('./harness/inbox-access-controller.ts', import.meta.url)
        ),
        name: 'cr007',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('actual fixture bundle');
  const chunks = output.output.filter((chunk) => chunk.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  await expect.poll(() => connections.size).toBe(0);
  peakPrivateConnections = 0;
  constructedPrivateConnections = 0;
  head = '';
  accept = true;
  frames.length = 0;
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const origin = new URL(String(url)).origin;
          if (
            ![
              'wss://discovery.example.org',
              'wss://inbox.example.org',
              'wss://peer.example.org',
              'wss://archive.example.org'
            ].includes(origin)
          )
            throw Error('unexpected fixture egress');
          if (window.cr007ThrowConstructor === origin) {
            window.cr007ThrowConstructor = undefined;
            throw Error('controlled native constructor failure before super');
          }
          super(endpoint + '/?origin=' + encodeURIComponent(origin), protocols);
        }
        override close(code?: number, reason?: string) {
          if (
            window.cr007DelayClose &&
            !this.url.includes(
              encodeURIComponent('wss://discovery.example.org')
            )
          ) {
            (window.cr007HeldClose ??= []).push(() =>
              super.close(code, reason)
            );
          } else super.close(code, reason);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(async () => {
    window.cr007Fixture = await window.cr007.makeControllerFixture();
    return window.cr007Fixture.preferenceWire([window.cr007.origins[0]]);
  });
  await page.evaluate(async () => {
    window.cr007Controller = await window.cr007Fixture.capture();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => {
      window.cr007Fault?.restore();
      window.cr007Release?.();
      window.cr007StopPage?.();
      window.cr007StopLive?.();
      window.cr007DelayClose = false;
      for (const release of window.cr007HeldClose ?? []) release();
      window.cr007Fixture.close();
    });
});
async function challenge(page: Page) {
  await page.evaluate(() =>
    window.cr007.authenticateInboxAccess(
      window.cr007Controller,
      window.cr007.origins[0],
      'reviewed_inbox_connection_auth'
    )
  );
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.cr007.inboxAccessControllerSnapshot(window.cr007Controller)
            ?.auth[0]?.state
      )
    )
    .toBe('challenge');
}
test('genuine controller capture has no private connection or sign and never claims readiness', async ({
  page
}) => {
  expect(
    [...connections.values()].filter(
      (origin) => origin !== 'wss://discovery.example.org'
    )
  ).toHaveLength(0);
  const result = await page.evaluate(async () => ({
    snapshot: window.cr007.inboxAccessControllerSnapshot(
      window.cr007Controller
    ),
    signs: window.cr007Fixture.signs(),
    wrong: await window.cr007.authenticateInboxAccess(
      window.cr007Controller,
      window.cr007.origins[0],
      'wrong-review'
    )
  }));
  expect(result.signs).toBe(0);
  expect(result.wrong).toBe('unavailable');
  expect(result.snapshot?.readiness).toBe('unavailable');
  expect(
    frames.some(({ frame }) => ['AUTH', 'EVENT'].includes(String(frame[0])))
  ).toBe(false);
});
test('explicit current owner AUTH sends real signed22242 only and duplicate initiation cannot reset it', async ({
  page
}) => {
  await challenge(page);
  expect(
    await page.evaluate(() =>
      window.cr007.respondInboxAccessAuthentication(
        window.cr007Controller,
        window.cr007.origins[0],
        'reviewed_inbox_auth_response'
      )
    )
  ).toBe('accepted');
  expect(
    await page.evaluate(() =>
      window.cr007.authenticateInboxAccess(
        window.cr007Controller,
        window.cr007.origins[0],
        'reviewed_inbox_connection_auth'
      )
    )
  ).toBe('accepted');
  const sent = frames.filter(({ frame }) => frame[0] === 'AUTH');
  expect(sent).toHaveLength(1);
  expect(sent[0].frame[1]).toMatchObject({
    kind: 22242,
    pubkey: await page.evaluate(() => window.cr007Fixture.owner),
    tags: [
      ['relay', 'wss://inbox.example.org/'],
      ['challenge', 'cr007-controlled-challenge']
    ]
  });
  expect(frames.some(({ frame }) => frame[0] === 'EVENT')).toBe(false);
  expect(
    await page.evaluate(
      () =>
        window.cr007.inboxAccessControllerSnapshot(window.cr007Controller)
          ?.readiness
    )
  ).toBe('unavailable');
});
test('actual discovery generation churn during pending signing cannot emit late AUTH', async ({
  page
}) => {
  await challenge(page);
  await page.evaluate(() => {
    window.cr007Fixture.beforeSign(
      () =>
        new Promise<void>((resolve) => {
          window.cr007Waiting = true;
          window.cr007Release = resolve;
        })
    );
    window.cr007Pending = window.cr007.respondInboxAccessAuthentication(
      window.cr007Controller,
      window.cr007.origins[0],
      'reviewed_inbox_auth_response'
    );
  });
  await expect.poll(() => page.evaluate(() => window.cr007Waiting)).toBe(true);
  await page.evaluate(() => {
    window.cr007Fixture.churnHead();
    window.cr007Release?.();
  });
  expect(await page.evaluate(() => window.cr007Pending)).toBe('stopped');
  expect(frames.filter(({ frame }) => frame[0] === 'AUTH')).toHaveLength(0);
});
test('actual discovery churn during pending fresh key cannot open an AUTH socket', async ({
  page
}) => {
  await page.evaluate(() => {
    window.cr007Fixture.beforeKey(
      () =>
        new Promise<void>((resolve) => {
          window.cr007Waiting = true;
          window.cr007Release = resolve;
        })
    );
    window.cr007Pending = window.cr007.authenticateInboxAccess(
      window.cr007Controller,
      window.cr007.origins[0],
      'reviewed_inbox_connection_auth'
    );
  });
  await expect.poll(() => page.evaluate(() => window.cr007Waiting)).toBe(true);
  await page.evaluate(() => {
    window.cr007Fixture.churnHead();
    window.cr007Release?.();
  });
  expect(await page.evaluate(() => window.cr007Pending)).not.toBe('accepted');
  expect(
    [...connections.values()].filter(
      (origin) => origin !== 'wss://discovery.example.org'
    )
  ).toHaveLength(0);
  expect(frames.filter(({ frame }) => frame[0] === 'AUTH')).toHaveLength(0);
});
test('selected live receive scope reaches OPEN while a delivery scope cannot borrow reads', async ({
  page
}) => {
  const result = await page.evaluate(() => {
    const receive = window.cr007Fixture.scope(
      [window.cr007.origins[2]],
      'receive'
    );
    window.cr007StopLive = window.cr007Fixture.live(receive, () => {
      window.cr007Open = true;
    });
    const delivery = window.cr007Fixture.scope(
      [window.cr007.origins[1]],
      'delivery'
    );
    let refused = false;
    try {
      window.cr007Fixture.page(delivery);
    } catch {
      refused = true;
    }
    return refused;
  });
  expect(result).toBe(true);
  await expect.poll(() => page.evaluate(() => window.cr007Open)).toBe(true);
  expect(
    frames.filter(
      ({ origin, frame }) =>
        origin === 'wss://peer.example.org' && frame[0] === 'REQ'
    )
  ).toHaveLength(0);
});
test('closing AUTH controller leaves independent other-origin receive and anonymous discovery intact', async ({
  page
}) => {
  await challenge(page);
  await page.evaluate(() => {
    const receive = window.cr007Fixture.scope(
      [window.cr007.origins[2]],
      'receive'
    );
    window.cr007StopLive = window.cr007Fixture.live(receive, () => {
      window.cr007Open = true;
    });
  });
  await expect.poll(() => page.evaluate(() => window.cr007Open)).toBe(true);
  await expect
    .poll(
      () =>
        [...connections.values()].filter(
          (origin) => origin === 'wss://archive.example.org'
        ).length
    )
    .toBe(1);
  await page.evaluate(() =>
    window.cr007.closeInboxAccessController(window.cr007Controller)
  );
  await expect
    .poll(
      () =>
        [...connections.values()].filter(
          (origin) => origin === 'wss://inbox.example.org'
        ).length
    )
    .toBe(0);
  expect(
    [...connections.values()].filter(
      (origin) => origin === 'wss://archive.example.org'
    )
  ).toHaveLength(1);
  expect(await page.evaluate(() => window.cr007Open)).toBe(true);
});

test('scope-only current-head page invalidation leaves another origin live', async ({
  page
}) => {
  await page.evaluate(() => {
    const receive = window.cr007Fixture.scope(
      [window.cr007.origins[2]],
      'receive'
    );
    window.cr007LiveEvents = 0;
    window.cr007StopLive = window.cr007Fixture.live(
      receive,
      () => {
        window.cr007Open = true;
      },
      () => {
        window.cr007LiveEvents = (window.cr007LiveEvents ?? 0) + 1;
      }
    );
  });
  await expect.poll(() => page.evaluate(() => window.cr007Open)).toBe(true);
  await expect
    .poll(
      () =>
        [...connections.values()].filter(
          (origin) => origin === 'wss://archive.example.org'
        ).length
    )
    .toBe(1);
  await page.evaluate(() => {
    const pageScope = window.cr007Fixture.scope(
      [window.cr007.origins[0]],
      'receive',
      true
    );
    window.cr007StopPage = window.cr007Fixture.page(pageScope, (reason) => {
      window.cr007PageEnd = reason;
    });
  });
  await expect
    .poll(
      () =>
        frames.filter(
          ({ origin, frame }) =>
            origin === 'wss://inbox.example.org' && frame[0] === 'REQ'
        ).length
    )
    .toBe(1);
  await page.evaluate(() => window.cr007Fixture.churnHead());
  const request = frames.find(
    ({ origin, frame }) =>
      origin === 'wss://inbox.example.org' && frame[0] === 'REQ'
  );
  expect(request).toBeDefined();
  for (const [socket, origin] of connections) {
    if (origin === 'wss://inbox.example.org')
      socket.send(JSON.stringify(['EOSE', request!.frame[1]]));
    if (origin === 'wss://archive.example.org') {
      const live = frames.find(
        ({ origin: from, frame }) => from === origin && frame[0] === 'REQ'
      );
      socket.send(
        JSON.stringify([
          'EVENT',
          live!.frame[1],
          {
            id: '1'.repeat(64),
            pubkey: '2'.repeat(64),
            kind: 1059,
            created_at: 1,
            tags: [],
            content: '',
            sig: '3'.repeat(128)
          }
        ])
      );
    }
  }
  await expect
    .poll(() => page.evaluate(() => window.cr007PageEnd))
    .toBe('stale');
  await expect.poll(() => page.evaluate(() => window.cr007LiveEvents)).toBe(1);
  expect(
    [...connections.values()].filter(
      (origin) => origin === 'wss://archive.example.org'
    )
  ).toHaveLength(1);
});
test('pending receive fresh-key wait is fenced by genuine discovery-head churn', async ({
  page
}) => {
  await page.evaluate(() => {
    window.cr007Fixture.beforeKey(
      () =>
        new Promise<void>((resolve) => {
          window.cr007Waiting = true;
          window.cr007Release = resolve;
        })
    );
    const receive = window.cr007Fixture.scope(
      [window.cr007.origins[0]],
      'receive',
      true
    );
    window.cr007StopPage = window.cr007Fixture.page(receive, (reason) => {
      window.cr007PageEnd = reason;
    });
  });
  await expect.poll(() => page.evaluate(() => window.cr007Waiting)).toBe(true);
  await page.evaluate(() => {
    window.cr007Fixture.churnHead();
    window.cr007Release?.();
  });
  await expect
    .poll(() => page.evaluate(() => window.cr007PageEnd))
    .toBe('stale');
  expect(
    frames.filter(
      ({ origin, frame }) =>
        origin === 'wss://inbox.example.org' && frame[0] === 'REQ'
    )
  ).toHaveLength(0);
  expect(
    [...connections.values()].filter(
      (origin) => origin !== 'wss://discovery.example.org'
    )
  ).toHaveLength(0);
});
test('AUTH retirement blocks a pending same-origin receive until actual physical close', async ({
  page
}) => {
  await challenge(page);
  await page.evaluate(() => {
    window.cr007DelayClose = true;
    window.cr007Fixture.beforeKey(
      () =>
        new Promise<void>((resolve) => {
          window.cr007Waiting = true;
          window.cr007Release = resolve;
        })
    );
    const receive = window.cr007Fixture.scope(
      [window.cr007.origins[0]],
      'receive'
    );
    window.cr007StopPage = window.cr007Fixture.page(receive, (reason) => {
      window.cr007PageEnd = reason;
    });
  });
  await expect.poll(() => page.evaluate(() => window.cr007Waiting)).toBe(true);
  await page.evaluate(() => {
    window.cr007.closeInboxAccessController(window.cr007Controller);
    window.cr007Release?.();
  });
  await expect
    .poll(() => page.evaluate(() => window.cr007PageEnd))
    .toBe('stale');
  expect(
    frames.filter(
      ({ origin, frame }) =>
        origin === 'wss://inbox.example.org' && frame[0] === 'REQ'
    )
  ).toHaveLength(0);
  expect(
    [...connections.values()].filter(
      (origin) => origin === 'wss://inbox.example.org'
    )
  ).toHaveLength(1);
  expect(
    await page.evaluate(() => {
      try {
        window.cr007Fixture.scope([window.cr007.origins[0]], 'receive');
        return false;
      } catch {
        return true;
      }
    })
  ).toBe(true);
  await page.evaluate(() => {
    window.cr007DelayClose = false;
    for (const release of window.cr007HeldClose ?? []) release();
  });
  await expect
    .poll(
      () =>
        [...connections.values()].filter(
          (origin) => origin === 'wss://inbox.example.org'
        ).length
    )
    .toBe(0);
});
test('ordinary three-socket session teardown blocks replacement until actual close', async ({
  page
}) => {
  await page.evaluate(() => {
    const receive = window.cr007Fixture.scope(window.cr007.origins, 'receive');
    window.cr007StopLive = window.cr007Fixture.live(receive, () => {
      window.cr007Open = true;
    });
  });
  await expect.poll(() => page.evaluate(() => window.cr007Open)).toBe(true);
  await expect
    .poll(
      () =>
        [...connections.values()].filter(
          (origin) => origin !== 'wss://discovery.example.org'
        ).length
    )
    .toBe(3);
  expect(
    [...connections.values()].filter(
      (origin) => origin !== 'wss://discovery.example.org'
    )
  ).toHaveLength(3);
  const originalSockets = [...connections.keys()].filter(
    (socket) => connections.get(socket) !== 'wss://discovery.example.org'
  );
  const result = await page.evaluate(async () => {
    window.cr007DelayClose = true;
    const closed = window.cr007Fixture.closeSession();
    try {
      await window.cr007Fixture.successorScope();
      return { closed, blocked: false };
    } catch {
      return { closed, blocked: true };
    }
  });
  expect(result).toEqual({ closed: false, blocked: true });
  expect(
    [...connections.keys()].filter(
      (socket) => connections.get(socket) !== 'wss://discovery.example.org'
    )
  ).toEqual(originalSockets);
  expect(constructedPrivateConnections).toBe(3);
  expect(peakPrivateConnections).toBe(3);
  expect(
    [...connections.values()].filter(
      (origin) => origin !== 'wss://discovery.example.org'
    )
  ).toHaveLength(3);
  await page.evaluate(() => {
    window.cr007DelayClose = false;
    for (const release of window.cr007HeldClose ?? []) release();
  });
  await expect
    .poll(
      () =>
        [...connections.values()].filter(
          (origin) => origin !== 'wss://discovery.example.org'
        ).length
    )
    .toBe(0);
  await page.evaluate(async () => {
    const receive = await window.cr007Fixture.successorScope();
    window.cr007StopLive = window.cr007Fixture.live(receive, () => {
      window.cr007Reconnected = true;
    });
  });
  await expect
    .poll(() => page.evaluate(() => window.cr007Reconnected))
    .toBe(true);
  await expect
    .poll(
      () =>
        [...connections.values()].filter(
          (origin) => origin !== 'wss://discovery.example.org'
        ).length
    )
    .toBe(3);
  expect(
    [...connections.values()].filter(
      (origin) => origin !== 'wss://discovery.example.org'
    )
  ).toHaveLength(3);
  await expect.poll(() => constructedPrivateConnections).toBe(6);
  expect(peakPrivateConnections).toBe(3);
});
test('synchronous native constructor failure retires the original metadata and permits actual retry', async ({
  page
}) => {
  await page.evaluate(() => {
    window.cr007ThrowConstructor = window.cr007.origins[0];
    const receive = window.cr007Fixture.scope(
      [window.cr007.origins[0]],
      'receive'
    );
    window.cr007StopPage = window.cr007Fixture.page(receive, (reason) => {
      window.cr007PageEnd = reason;
    });
  });
  await expect
    .poll(() => page.evaluate(() => window.cr007PageEnd))
    .toBe('error');
  expect(
    [...connections.values()].filter(
      (origin) => origin !== 'wss://discovery.example.org'
    )
  ).toHaveLength(0);
  await page.evaluate(() => {
    const receive = window.cr007Fixture.scope(
      [window.cr007.origins[0]],
      'receive'
    );
    window.cr007StopLive = window.cr007Fixture.live(receive, () => {
      window.cr007Open = true;
    });
  });
  await expect.poll(() => page.evaluate(() => window.cr007Open)).toBe(true);
  await expect
    .poll(
      () =>
        [...connections.values()].filter(
          (origin) => origin !== 'wss://discovery.example.org'
        ).length
    )
    .toBe(1);
  expect(
    [...connections.values()].filter(
      (origin) => origin !== 'wss://discovery.example.org'
    )
  ).toHaveLength(1);
});

test('AUTH startup native constructor failure releases the exact origin before explicit retry', async ({
  page
}) => {
  const first = await page.evaluate(async () => {
    window.cr007ThrowConstructor = window.cr007.origins[0];
    return window.cr007.authenticateInboxAccess(
      window.cr007Controller,
      window.cr007.origins[0],
      'reviewed_inbox_connection_auth'
    );
  });
  expect(first).toBe('unavailable');
  expect(
    [...connections.values()].filter(
      (origin) => origin !== 'wss://discovery.example.org'
    )
  ).toHaveLength(0);
  await challenge(page);
  expect(
    await page.evaluate(() =>
      window.cr007.respondInboxAccessAuthentication(
        window.cr007Controller,
        window.cr007.origins[0],
        'reviewed_inbox_auth_response'
      )
    )
  ).toBe('accepted');
  expect(frames.filter(({ frame }) => frame[0] === 'AUTH')).toHaveLength(1);
});

test('failed AUTH startup cleanup stays with its original scope and retries after removal recovers', async ({
  page
}) => {
  const first = await page.evaluate(async () => {
    window.cr007ThrowConstructor = window.cr007.origins[0];
    window.cr007Fault = window.cr007.installCloseFailure();
    return window.cr007.authenticateInboxAccess(
      window.cr007Controller,
      window.cr007.origins[0],
      'reviewed_inbox_connection_auth'
    );
  });
  expect(first).toBe('unavailable');
  expect(
    await page.evaluate(() =>
      window.cr007.closeInboxAccessController(window.cr007Controller)
    )
  ).toBe(false);
  await page.evaluate(() => window.cr007Fault?.allow());
  expect(
    await page.evaluate(() =>
      window.cr007.closeInboxAccessController(window.cr007Controller)
    )
  ).toBe(true);
  await page.evaluate(async () => {
    window.cr007Fault?.restore();
    window.cr007Controller = await window.cr007Fixture.capture();
  });
  await challenge(page);
  expect(
    await page.evaluate(() =>
      window.cr007.respondInboxAccessAuthentication(
        window.cr007Controller,
        window.cr007.origins[0],
        'reviewed_inbox_auth_response'
      )
    )
  ).toBe('accepted');
  expect(frames.filter(({ frame }) => frame[0] === 'AUTH')).toHaveLength(1);
});
