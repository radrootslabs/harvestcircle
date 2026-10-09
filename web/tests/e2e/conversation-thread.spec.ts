import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/conversation-thread.ts';
declare global {
  interface Window {
    hcp103: typeof Fixture;
    hcp103Fixture: Awaited<ReturnType<typeof Fixture.renderFixture>>;
    hcp103Pending?: Promise<boolean>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let events: unknown[][] = [],
  requests: unknown[][] = [];
test.beforeAll(async () => {
  server = await createStaticHarness();
  sockets = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  await once(sockets, 'listening');
  const address = sockets.address();
  if (!address || typeof address === 'string') throw Error('no loopback');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket) =>
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
          ? bytes
          : Array.isArray(bytes)
            ? Buffer.concat(bytes)
            : Buffer.from(bytes),
        frame = JSON.parse(raw.toString()) as unknown[];
      if (frame[0] === 'EVENT') {
        events.push(frame);
        const event = frame[1] as { id: string };
        socket.send(JSON.stringify(['OK', event.id, true, '']));
      }
      if (frame[0] === 'REQ') {
        requests.push(frame);
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      }
    })
  );
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
          new URL('./harness/conversation-thread.ts', import.meta.url)
        ),
        name: 'hcp103',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('no bundle');
  const chunks = output.output.filter((row) => row.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
async function load(page: import('@playwright/test').Page) {
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (
            ![
              'wss://peer.example.org',
              'wss://archive.example.org',
              'wss://discovery.example.org'
            ].includes(new URL(String(url)).origin)
          )
            throw Error('unapproved destination');
          super(endpoint, protocols);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
}
test.beforeEach(async ({ page }) => {
  events = [];
  requests = [];
  await load(page);
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(async () => {
      window.hcp103Fixture?.settle();
      await window.hcp103Pending;
      await window.hcp103Fixture?.close();
    });
});
async function ready(
  page: import('@playwright/test').Page,
  kind: Fixture.Scenario = 'inbound',
  selectedId?: string
) {
  await page.evaluate(
    async ({ kind, selectedId }) => {
      window.hcp103Fixture = await window.hcp103.renderFixture(
        kind,
        selectedId
      );
    },
    { kind, selectedId }
  );
  await page.getByRole('button', { name: 'Check current preference' }).click();
}
async function unlock(page: import('@playwright/test').Page) {
  await page
    .getByRole('button', { name: 'Unlock messages', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp103Fixture.thread().status))
    .toBe('ready');
  await expect(page.locator('[data-message-id]')).not.toHaveCount(0);
}
const missing = '12345678-1234-4234-8234-123456789abd';
function records(rows: unknown[]) {
  return rows.map(
    (row) =>
      JSON.parse((row as { wire: string }).wire) as {
        outer: string;
        read: { rumorHash: string } | null;
      }
  );
}
test('production private guest load keeps generic title noindex safe inbox link and no private effects', async ({
  page
}) => {
  await page.goto(server.url + '/messages/' + missing);
  await expect(
    page.getByRole('heading', { name: 'Connect or unlock' })
  ).toBeVisible();
  await expect(
    page.getByRole('link', { name: 'Back to messages' })
  ).toHaveAttribute('href', '/messages');
  await expect(page).toHaveTitle('HarvestCircle');
  await expect(page.locator('meta[name="robots"]')).toHaveAttribute(
    'content',
    'noindex'
  );
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('actual owner UUID and ciphertext stay unpreviewed and unread before explicit unlock', async ({
  page
}) => {
  await ready(page);
  const native = await page.evaluate(() => window.hcp103Fixture.native());
  expect(native.pairs).toHaveLength(1);
  expect(records(native.received).every((row) => row.read === null)).toBe(true);
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
  await expect(
    page.getByText('Actual encrypted thread fixture', { exact: true })
  ).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp103Fixture.delta())).toEqual({
    decrypts: 0,
    signs: 0,
    encrypts: 0
  });
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('original explicit decrypt renders authenticated sender inner time and plain body in one chronological column', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  await expect(page.locator('[data-message-id]')).toHaveCount(1);
  await expect(
    page.getByText('Actual encrypted thread fixture', { exact: true })
  ).toBeVisible();
  await expect(page.locator('[data-message-id]')).toContainText('Peer');
  await expect(page.locator('[data-message-id] time')).toHaveAttribute(
    'datetime',
    '2023-11-14T22:13:20.000Z'
  );
  expect(await page.evaluate(() => window.hcp103Fixture.delta())).toEqual({
    decrypts: 2,
    signs: 0,
    encrypts: 0
  });
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('two actual encrypted product contexts sort per message with their own citation and no current price claim', async ({
  page
}) => {
  await ready(page, 'contexts');
  await unlock(page);
  await expect(page.locator('[data-message-id]')).toHaveCount(3);
  const texts = await page.locator('[data-message-id]').allTextContents();
  expect(texts[0]).toContain('Actual encrypted thread fixture');
  expect(texts[1]).toContain('About apples');
  expect(texts[2]).toContain('About carrots');
  const refs = await page.evaluate(() => ({
    apples: window.hcp103Fixture.apples,
    carrots: window.hcp103Fixture.carrots
  }));
  await expect(
    page.locator('[data-message-id]').nth(1).getByRole('link')
  ).toHaveAttribute('href', '/products/' + refs.apples);
  await expect(
    page.locator('[data-message-id]').nth(2).getByRole('link')
  ).toHaveAttribute('href', '/products/' + refs.carrots);
  await expect(page.locator('[data-message-id]').nth(1)).toContainText(
    'unverified'
  );
  await expect(page.locator('[data-message-id]').nth(2)).toContainText(
    'not a current'
  );
  expect(requests).toEqual([]);
  expect(events).toEqual([]);
});
test('original self archive labels You without inventing a recipient ACK or remote read', async ({
  page
}) => {
  await ready(page, 'self_archive');
  await unlock(page);
  await expect(page.locator('[data-message-id]')).toContainText('You');
  await expect(page.locator('[data-message-id]')).toContainText(
    'Recipient delivery unknown'
  );
  await expect(page.locator('[data-message-id]')).not.toContainText(
    'Accepted by recipient inbox relay'
  );
  await expect(page.locator('[data-message-id]')).not.toContainText('Seen');
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('actual original loopback SDK ACK and native receipt supply relay acceptance without thread publication', async ({
  page
}) => {
  await ready(page, 'receipt');
  expect(events).toHaveLength(2);
  const native = await page.evaluate(() => window.hcp103Fixture.native());
  expect(
    (
      JSON.parse((native.sends[0] as { wire: string }).wire) as {
        receipts: { role: string; status: string }[];
      }
    ).receipts.some(
      (row: { role: string; status: string }) =>
        row.role === 'peer' && row.status === 'accepted'
    )
  ).toBe(true);
  await unlock(page);
  await expect(page.locator('[data-message-id]')).toContainText(
    'Accepted by recipient inbox relay'
  );
  await expect(page.locator('[data-message-id]')).not.toContainText('Seen');
  expect(events).toHaveLength(2);
  expect(requests).toEqual([]);
});
test('different genuine outer wraps of the same rumor render once and retain both ciphertext copies', async ({
  page
}) => {
  await ready(page, 'duplicate');
  await unlock(page);
  await expect(page.locator('[data-message-id]')).toHaveCount(1);
  expect(
    (await page.evaluate(() => window.hcp103Fixture.native())).received
  ).toHaveLength(2);
  expect(await page.evaluate(() => window.hcp103Fixture.delta())).toEqual({
    decrypts: 4,
    signs: 0,
    encrypts: 0
  });
});
test('only actual displayed original room records local read without changing ciphertext or notifying peer', async ({
  page
}) => {
  await ready(page);
  const before = await page.evaluate(() => window.hcp103Fixture.native());
  await unlock(page);
  await expect
    .poll(
      async () =>
        records(
          (await page.evaluate(() => window.hcp103Fixture.native())).received
        ).filter((row) => row.read !== null).length
    )
    .toBe(1);
  const after = await page.evaluate(() => window.hcp103Fixture.native());
  expect(records(after.received).map((row) => row.outer)).toEqual(
    records(before.received).map((row) => row.outer)
  );
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('copied original controller cannot import bodies navigation or native display effects', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp103Fixture,
      before = await f.native();
    return {
      copied: await f.copied(),
      before,
      after: await f.native(),
      delta: f.delta()
    };
  });
  expect(result.copied.snapshot).toMatchObject({
    status: 'unavailable',
    messages: []
  });
  expect(result.copied.message).toBeUndefined();
  expect(result.copied.resolve).toBe(false);
  expect(result.copied.read).toBe(false);
  expect(result.after).toEqual(result.before);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
test('unknown original UUID refuses before SDK decrypt and never substitutes an existing pair', async ({
  page
}) => {
  await ready(page, 'inbound', missing);
  const before = await page.evaluate(() => window.hcp103Fixture.native());
  expect(await page.evaluate(() => window.hcp103Fixture.unlock())).toBe(false);
  expect(await page.evaluate(() => window.hcp103Fixture.next())).toBe(false);
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp103Fixture.native())).toEqual(
    before
  );
  expect(await page.evaluate(() => window.hcp103Fixture.delta())).toEqual({
    decrypts: 0,
    signs: 0,
    encrypts: 0
  });
});
test('same-owner UUID in a separate real storage profile is unavailable without peer inference', async ({
  page,
  browser
}) => {
  await ready(page);
  const id = await page.evaluate(() => window.hcp103Fixture.conversationId),
    context = await browser.newContext();
  try {
    const other = await context.newPage();
    await load(other);
    await ready(other, 'inbound', id);
    const before = await other.evaluate(() => window.hcp103Fixture.native());
    expect(await other.evaluate(() => window.hcp103Fixture.unlock())).toBe(
      false
    );
    await expect(other.locator('[data-message-id]')).toHaveCount(0);
    expect(await other.evaluate(() => window.hcp103Fixture.native())).toEqual(
      before
    );
    expect(await other.evaluate(() => window.hcp103Fixture.delta())).toEqual({
      decrypts: 0,
      signs: 0,
      encrypts: 0
    });
    await other.evaluate(() => window.hcp103Fixture.close());
  } finally {
    await context.close();
  }
});
test('Stop and owner logout clear original thread bodies and deny stale display effects', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  await page.evaluate(() => window.hcp103Fixture.stop());
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
  const result = await page.evaluate(async () => {
    const f = window.hcp103Fixture,
      before = await f.native();
    f.logout();
    return {
      resolved: await f.resolve(f.conversationId),
      before,
      after: await f.native()
    };
  });
  expect(result.resolved).toBe(false);
  expect(result.after).toEqual(result.before);
});
test('logout during held actual extension decrypt prevents late thread disclosure or native read', async ({
  page
}) => {
  await ready(page);
  await page.evaluate(() => {
    const f = window.hcp103Fixture;
    f.mode('hold_decrypt');
    window.hcp103Pending = f.unlock();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp103Fixture.pending()))
    .toBe(true);
  const before = await page.evaluate(() => window.hcp103Fixture.native());
  await page.evaluate(() => {
    window.hcp103Fixture.logout();
    window.hcp103Fixture.settle();
  });
  expect(await page.evaluate(() => window.hcp103Pending)).toBe(false);
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp103Fixture.native())).toEqual(
    before
  );
});
test('initial backfill is not replayed through a live log and later explicit batch preserves focus and reading position', async ({
  page
}) => {
  await ready(page, 'backfill');
  await unlock(page);
  await expect(page.locator('[data-message-id]')).toHaveCount(20);
  await expect(page.locator('[data-thread-announcement]')).toHaveText('');
  await expect(
    page.locator('[data-message-id] [aria-live="polite"]')
  ).toHaveCount(0);
  await page.getByRole('button', { name: 'Reading position marker' }).focus();
  await page.evaluate(() => window.scrollTo(0, 320));
  await expect
    .poll(() => page.evaluate(() => window.scrollY))
    .toBeGreaterThan(0);
  const before = await page.evaluate(() => ({
    active: document.activeElement?.id,
    scroll: window.scrollY
  }));
  expect(await page.evaluate(() => window.hcp103Fixture.next())).toBe(true);
  await expect(page.locator('[data-message-id]')).toHaveCount(21);
  await expect(page.locator('[data-thread-announcement]')).toHaveAttribute(
    'aria-live',
    'polite'
  );
  await expect(page.locator('[data-thread-announcement]')).toHaveText(
    '1 message added.'
  );
  expect(
    await page.evaluate(() => ({
      active: document.activeElement?.id,
      scroll: window.scrollY
    }))
  ).toEqual(before);
});
test('hostile plaintext and missing parent stay inert without HTML external links or private-ID searches', async ({
  page
}) => {
  await ready(page, 'hostile');
  await unlock(page);
  await expect(page.locator('[data-message-id]')).toContainText('<img src=x>');
  await expect(page.locator('[data-message-id]')).toContainText(
    'Earlier message unavailable'
  );
  await expect(page.locator('[data-message-id] img')).toHaveCount(0);
  await expect(page.locator('[data-message-id] a')).toHaveCount(0);
  expect(requests).toEqual([]);
  expect(events).toEqual([]);
  expect(page.url()).not.toContain('f'.repeat(64));
});
test('changing to an unknown route selection revokes stale original row lookup without writes or replacement', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  const result = await page.evaluate(async (missing) => {
    const f = window.hcp103Fixture,
      before = await f.native(),
      result = await f.resolve(missing);
    return { result, old: f.thread(), before, after: await f.native() };
  }, missing);
  expect(result.result).toBe(false);
  expect(result.old).toMatchObject({ status: 'unavailable', messages: [] });
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
  expect(result.after).toEqual(result.before);
});

test('actual native owner lock contention leaves local read unclaimed and explicit retry confirms it after display', async ({
  page
}) => {
  await ready(page);
  await page.evaluate(() => window.hcp103Fixture.holdReadLock());
  await unlock(page);
  await expect(
    page.getByText(
      'Local read status could not be confirmed. Encrypted copies are preserved.',
      { exact: true }
    )
  ).toBeVisible();
  expect(
    records(
      (await page.evaluate(() => window.hcp103Fixture.native())).received
    ).every((row) => row.read === null)
  ).toBe(true);
  await page.evaluate(() => window.hcp103Fixture.releaseRead());
  await page.getByRole('button', { name: 'Retry local read status' }).click();
  await expect
    .poll(
      async () =>
        records(
          (await page.evaluate(() => window.hcp103Fixture.native())).received
        ).filter((row) => row.read !== null).length
    )
    .toBe(1);
  await expect(
    page.getByRole('button', { name: 'Retry local read status' })
  ).toHaveCount(0);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});
