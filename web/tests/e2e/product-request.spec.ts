import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { finalizeEvent, getPublicKey } from 'applesauce-core/helpers';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/product-request.ts';

declare global {
  interface Window {
    hcp107: typeof Fixture;
    hcp107Fixture: Awaited<ReturnType<typeof Fixture.renderFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>;
let sockets: WebSocketServer, endpoint: string, bundle: string;
let missingOwn = false;
let holdOwnEose = false,
  holdPeerEose = false;
let waitingEose: (() => void)[] = [];
let events: {
  origin: string;
  event: {
    kind: number;
    id: string;
    pubkey: string;
    content: string;
    tags: string[][];
  };
}[] = [];
const sellerSecret = new Uint8Array(32).fill(84);
const ownerSecret = new Uint8Array(32).fill(83);
const peer = getPublicKey(sellerSecret),
  owner = getPublicKey(ownerSecret);
let ownPreference: ReturnType<typeof finalizeEvent> | undefined;
function preference(secret: Uint8Array, origin: string) {
  return finalizeEvent(
    {
      kind: 10050,
      created_at: 1700000060,
      tags: [['relay', origin]],
      content: ''
    },
    secret
  );
}
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
  sockets.on('connection', (socket, request) => {
    const origin =
      new URL(request.url ?? '/', endpoint).searchParams.get('source') ?? '';
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
      if (frame[0] === 'EVENT') {
        const event = frame[1] as (typeof events)[number]['event'];
        events.push({ origin, event });
        if (event.kind === 10050)
          ownPreference = event as ReturnType<typeof finalizeEvent>;
        socket.send(
          JSON.stringify(['OK', event.id, true, 'controlled acceptance'])
        );
        return;
      }
      if (frame[0] !== 'REQ') return;
      const filters = frame.slice(2) as {
        kinds?: number[];
        authors?: string[];
      }[];
      for (const filter of filters) {
        if (filter.kinds?.includes(30402)) {
          const event = finalizeEvent(
            {
              kind: 30402,
              created_at: 1700000060,
              content: 'Independently signed carrots',
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
          socket.send(JSON.stringify(['EVENT', frame[1], event]));
        }
        if (filter.kinds?.includes(10050)) {
          if (filter.authors?.includes(peer))
            socket.send(
              JSON.stringify([
                'EVENT',
                frame[1],
                preference(sellerSecret, 'wss://peer.example.org')
              ])
            );
          if (filter.authors?.includes(owner) && (!missingOwn || ownPreference))
            socket.send(
              JSON.stringify([
                'EVENT',
                frame[1],
                ownPreference ??
                  preference(ownerSecret, 'wss://archive.example.org')
              ])
            );
        }
      }
      if (
        filters.some(
          (filter) =>
            filter.kinds?.includes(10050) &&
            ((holdOwnEose && filter.authors?.includes(owner)) ||
              (holdPeerEose && filter.authors?.includes(peer)))
        )
      )
        waitingEose.push(() => {
          if (socket.readyState === 1)
            socket.send(JSON.stringify(['EOSE', frame[1]]));
        });
      else socket.send(JSON.stringify(['EOSE', frame[1]]));
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
          new URL('./harness/product-request.ts', import.meta.url)
        )
      }
    },
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/product-request.ts', import.meta.url)
        ),
        name: 'hcp107',
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
test.beforeEach(async ({ page }) => {
  events = [];
  missingOwn = false;
  ownPreference = undefined;
  holdOwnEose = false;
  holdPeerEose = false;
  waitingEose = [];
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const origin = new URL(String(url)).origin;
          if (
            ![
              'wss://discovery.example.org',
              'wss://archive.example.org',
              'wss://peer.example.org'
            ].includes(origin)
          )
            throw Error('unapproved destination');
          super(endpoint + '/?source=' + encodeURIComponent(origin), protocols);
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
    await page.evaluate(() => window.hcp107Fixture?.close());
});
const panel = (page: Page) => page.locator('[data-request-fixture]');
const editor = (page: Page) =>
  panel(page).getByRole('textbox', { name: 'Your message', exact: true });
async function ready(page: Page, qualified = true) {
  await page.evaluate(async (qualified) => {
    window.hcp107Fixture = await window.hcp107.renderFixture(qualified);
  }, qualified);
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp107Fixture.viewSnapshot().outcome)
    )
    .toBe('active');
  await panel(page)
    .getByRole('button', { name: 'Message seller', exact: true })
    .click();
  await expect(editor(page)).toBeVisible();
}
async function connect(page: Page) {
  await panel(page)
    .getByRole('button', { name: 'Connect extension', exact: true })
    .click();
  await expect(
    panel(page).getByRole('button', {
      name: 'Check messaging support',
      exact: true
    })
  ).toBeVisible();
  await panel(page)
    .getByRole('button', { name: 'Check messaging support', exact: true })
    .click();
  await expect(panel(page)).toContainText('Messaging support checked.');
}
async function unlock(page: Page) {
  await connect(page);
  await panel(page)
    .getByRole('button', { name: 'Check current preference', exact: true })
    .click();
  await panel(page)
    .getByRole('button', { name: 'Unlock inbox', exact: true })
    .click();
  await expect(
    panel(page).getByRole('button', { name: 'Send request', exact: true })
  ).toBeEnabled();
}
async function noPrivate(page: Page, marker?: string) {
  expect(events.filter((row) => row.event.kind === 1059)).toEqual([]);
  expect(
    await page.evaluate(async () => (await window.hcp107Fixture.native()).sends)
  ).toEqual([]);
  if (marker)
    expect(
      await page.evaluate(() => window.hcp107Fixture.persistentText())
    ).not.toContain(marker);
}
test('guest text is memory only, with no automatic identity or private intent', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('guest-private-marker');
  expect(await page.evaluate(() => window.hcp107Fixture.counts())).toEqual({
    keys: 0,
    encrypts: 0,
    decrypts: 0,
    signs: 0
  });
  await noPrivate(page, 'guest-private-marker');
});
test('same-page Connect preserves edited text and never sends', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('connect-private-marker');
  await panel(page)
    .getByRole('button', { name: 'Connect extension', exact: true })
    .click();
  await expect(editor(page)).toHaveValue('connect-private-marker');
  await noPrivate(page, 'connect-private-marker');
});
test('explicit support probe preserves text and creates no private send', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('probe-private-marker');
  await connect(page);
  await expect(editor(page)).toHaveValue('probe-private-marker');
  await noPrivate(page, 'probe-private-marker');
});
test('existing preference and unlock are separate from final Send', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  expect(events).toEqual([]);
  await noPrivate(page);
});
test('public preference enablement is explicit and never autosends the request', async ({
  page
}) => {
  missingOwn = true;
  await ready(page);
  await editor(page).fill('setup-private-marker');
  await connect(page);
  await panel(page)
    .getByRole('button', { name: 'Check current preference', exact: true })
    .click();
  await panel(page)
    .getByRole('checkbox', { name: 'wss://archive.example.org', exact: true })
    .check();
  await panel(page)
    .getByRole('button', { name: 'Review inbox change', exact: true })
    .click();
  expect(events).toEqual([]);
  await panel(page)
    .getByRole('button', { name: 'Enable inbox', exact: true })
    .click();
  await expect
    .poll(() => events.filter((row) => row.event.kind === 10050).length)
    .toBeGreaterThan(0);
  await expect(editor(page)).toHaveValue('setup-private-marker');
  await noPrivate(page, 'setup-private-marker');
});
test('unqualified access cannot enable final Send', async ({ page }) => {
  await ready(page, false);
  await connect(page);
  await panel(page)
    .getByRole('button', { name: 'Check current preference', exact: true })
    .click();
  await panel(page)
    .getByRole('button', { name: 'Unlock inbox', exact: true })
    .click();
  await expect(panel(page)).toContainText(
    'Inbox access has not been qualified'
  );
  await expect(
    panel(page).getByRole('button', { name: 'Send request', exact: true })
  ).toBeDisabled();
  await noPrivate(page);
});
test('only the current task has one enabled primary action', async ({
  page
}) => {
  await ready(page);
  await expect(
    panel(page).locator('button.button--primary:enabled')
  ).toHaveCount(1);
  await connect(page);
  await expect(
    panel(page).locator('button.button--primary:enabled')
  ).toHaveCount(1);
  await panel(page)
    .getByRole('button', { name: 'Check current preference', exact: true })
    .click();
  await expect(
    panel(page).locator('button.button--primary:enabled')
  ).toHaveCount(1);
});
test('refused Connect keeps unsaved text and performs no send', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('refused-private-marker');
  await page.evaluate(() => window.hcp107Fixture.mode('refused_key'));
  await panel(page)
    .getByRole('button', { name: 'Connect extension', exact: true })
    .click();
  await expect(panel(page)).toContainText('Connection was not approved');
  await expect(editor(page)).toHaveValue('refused-private-marker');
  await noPrivate(page);
});
test('held Connect can be cancelled without losing text or autosending later', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('held-connect-marker');
  await page.evaluate(() => window.hcp107Fixture.mode('hold_key'));
  await panel(page)
    .getByRole('button', { name: 'Connect extension', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp107Fixture.pending()))
    .toBe(true);
  await panel(page)
    .getByRole('button', { name: 'Cancel', exact: true })
    .click();
  await panel(page)
    .getByRole('button', { name: 'Keep editing', exact: true })
    .click();
  await page.evaluate(() => window.hcp107Fixture.settle());
  await expect(editor(page)).toHaveValue('held-connect-marker');
  await noPrivate(page);
});
test('copied owners and unreviewed Send are rejected', async ({ page }) => {
  await ready(page);
  expect(await page.evaluate(() => window.hcp107Fixture.copied())).toEqual({
    create: false,
    snapshot: undefined,
    configure: false,
    send: { status: 'invalid' }
  });
  expect(await page.evaluate(() => window.hcp107Fixture.unreviewed())).toEqual({
    status: 'invalid'
  });
  await noPrivate(page);
});
test('final explicit Send publishes only ciphertext to the intended named destinations', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  const expected = await page.evaluate(() => window.hcp107Fixture.message());
  expect(expected).toBeDefined();
  await panel(page)
    .getByRole('button', { name: 'Send request', exact: true })
    .click();
  await expect
    .poll(() => events.filter((row) => row.event.kind === 1059).length)
    .toBeGreaterThanOrEqual(2);
  expect(events.every((row) => row.event.kind === 1059)).toBe(true);
  expect(new Set(events.map((row) => row.origin))).toEqual(
    new Set(['wss://archive.example.org', 'wss://peer.example.org'])
  );
  expect(
    events.every((row) => !row.event.content.includes('Verified carrots'))
  ).toBe(true);
  await expect(panel(page)).not.toContainText('Delivered');
  await expect
    .poll(() => page.evaluate(() => window.hcp107Fixture.snapshot()?.state))
    .toBe('completed');
  await expect(panel(page)).not.toContainText('No message has been sent.');
  expect(events).toHaveLength(2);
  const { nip44 } = await import('applesauce-core/helpers/encryption');
  const { getEventHash } = await import('applesauce-core/helpers');
  const rumors = events.map(({ event: outer }) => {
    const recipient = outer.tags.find((tag) => tag[0] === 'p')?.[1];
    expect([owner, peer]).toContain(recipient);
    const key = new Uint8Array(32).fill(recipient === owner ? 83 : 84);
    try {
      const seal = JSON.parse(
        nip44.v2.decrypt(
          outer.content,
          nip44.v2.utils.getConversationKey(key, outer.pubkey)
        )
      ) as { pubkey: string; content: string; kind: number; tags: string[][] };
      expect(seal.kind).toBe(13);
      expect(seal.pubkey).toBe(owner);
      expect(seal.tags).toEqual([]);
      const rumor = JSON.parse(
        nip44.v2.decrypt(
          seal.content,
          nip44.v2.utils.getConversationKey(key, seal.pubkey)
        )
      ) as {
        id: string;
        kind: number;
        pubkey: string;
        content: string;
        tags: string[][];
        created_at: number;
      };
      expect(rumor.kind).toBe(14);
      expect(rumor.pubkey).toBe(owner);
      expect(rumor.tags).toEqual([['p', peer]]);
      expect(rumor.content).toBe(expected?.content);
      expect(rumor.content).toContain('Viewed listing: nostr:nevent1');
      expect(rumor.content).toContain('Product: nostr:naddr1');
      expect(getEventHash(rumor)).toBe(rumor.id);
      return rumor;
    } finally {
      key.fill(0);
    }
  });
  expect(rumors[0]).toEqual(rumors[1]);
});
test('held self seal reports multiple approvals and does not publish ahead of persistence', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  await page.evaluate(() => window.hcp107Fixture.mode('hold_seal'));
  await panel(page)
    .getByRole('button', { name: 'Send request', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp107Fixture.pending()))
    .toBe(true);
  await expect(panel(page)).toContainText('approval');
  expect(events).toEqual([]);
  await page.evaluate(() => window.hcp107Fixture.settle());
});
test('Cancel retires a held private Send while Keep preserves unsaved text', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  await editor(page).fill('cancel-held-seal-marker');
  await page.evaluate(() => window.hcp107Fixture.mode('hold_seal'));
  await panel(page)
    .getByRole('button', { name: 'Send request', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp107Fixture.pending()))
    .toBe(true);
  const before = await page.evaluate(() => window.hcp107Fixture.native());
  const calls = await page.evaluate(() => window.hcp107Fixture.counts());
  await panel(page)
    .getByRole('button', { name: 'Cancel', exact: true })
    .click();
  await panel(page)
    .getByRole('button', { name: 'Keep editing', exact: true })
    .click();
  await expect(editor(page)).toHaveValue('cancel-held-seal-marker');
  expect(await page.evaluate(() => window.hcp107Fixture.pending())).toBe(true);
  await page.evaluate(() => window.hcp107Fixture.settle());
  await expect
    .poll(() => page.evaluate(() => window.hcp107Fixture.snapshot()?.busy))
    .toBe(false);
  expect(await page.evaluate(() => window.hcp107Fixture.native())).toEqual(
    before
  );
  expect(await page.evaluate(() => window.hcp107Fixture.counts())).toEqual(
    calls
  );
  expect(events).toEqual([]);
});
test('declined encryption retains unsaved text without publication', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  await editor(page).fill('declined-encryption-marker');
  await page.evaluate(() => window.hcp107Fixture.mode('declined_encrypt'));
  await panel(page)
    .getByRole('button', { name: 'Send request', exact: true })
    .click();
  await expect(editor(page)).toHaveValue('declined-encryption-marker');
  expect(events).toEqual([]);
});
test('disconnect during held crypto prevents late publication', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  await page.evaluate(() => window.hcp107Fixture.mode('hold_encrypt'));
  await panel(page)
    .getByRole('button', { name: 'Send request', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp107Fixture.pending()))
    .toBe(true);
  await page.evaluate(() => {
    window.hcp107Fixture.logout();
    window.hcp107Fixture.settle();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp107Fixture.pending()))
    .toBe(false);
  expect(events).toEqual([]);
});
test('dirty Cancel defaults to Keep editing with focus restored', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('dirty-cancel-marker');
  await panel(page)
    .getByRole('button', { name: 'Cancel', exact: true })
    .click();
  await expect(
    panel(page).getByRole('button', { name: 'Keep editing', exact: true })
  ).toBeFocused();
  await panel(page)
    .getByRole('button', { name: 'Keep editing', exact: true })
    .click();
  await expect(editor(page)).toHaveValue('dirty-cancel-marker');
  await expect(editor(page)).toBeFocused();
  await noPrivate(page);
});
test('dirty navigation is cancelled and Keep leaves the URL unchanged', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('navigation-private-marker');
  const url = page.url();
  expect(
    (await page.evaluate(() => window.hcp107Fixture.navigate())).cancelled
  ).toBe(true);
  await panel(page)
    .getByRole('button', { name: 'Keep editing', exact: true })
    .click();
  expect(page.url()).toBe(url);
  await expect(editor(page)).toHaveValue('navigation-private-marker');
  await noPrivate(page, 'navigation-private-marker');
});
test('editing after confirmation invalidates stale discard permission', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('revision-one');
  await page.evaluate(() => window.hcp107Fixture.request());
  await page.evaluate(() => window.hcp107Fixture.update('revision-two'));
  expect(await page.evaluate(() => window.hcp107Fixture.discard())).not.toBe(
    'allowed'
  );
  await expect(editor(page)).toHaveValue('revision-two');
  await noPrivate(page);
});
test('explicit discard clears unsaved text without saving or sending it', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('discard-private-marker');
  await panel(page)
    .getByRole('button', { name: 'Cancel', exact: true })
    .click();
  await panel(page)
    .getByRole('button', { name: 'Discard draft', exact: true })
    .click();
  await expect(editor(page)).toHaveCount(0);
  await noPrivate(page, 'discard-private-marker');
});
test('a genuine second draft writer invalidates stale discard permission', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('original-writer-one');
  await page.evaluate(() => window.hcp107Fixture.request());
  expect(
    await page.evaluate(() =>
      window.hcp107Fixture.directUpdate('original-writer-two')
    )
  ).toBe(true);
  expect(await page.evaluate(() => window.hcp107Fixture.discard())).not.toBe(
    'allowed'
  );
  await expect(editor(page)).toHaveValue('original-writer-two');
  await noPrivate(page);
});
for (const revoke of ['close', 'expiry'] as const) {
  test(
    'headless original ' +
      revoke +
      ' prevents later internal crypto and native self commit',
    async ({ page }) => {
      await ready(page);
      await unlock(page);
      await page.evaluate(() => window.hcp107Fixture.mode('hold_seal'));
      await page.evaluate(() => {
        void window.hcp107Fixture.beginHeadless();
      });
      await expect
        .poll(() => page.evaluate(() => window.hcp107Fixture.pending()))
        .toBe(true);
      const before = await page.evaluate(() => window.hcp107Fixture.native());
      const calls = await page.evaluate(() => window.hcp107Fixture.counts());
      await page.evaluate((revoke) => {
        if (revoke === 'close') window.hcp107Fixture.revokeView();
        else window.hcp107Fixture.advance();
        window.hcp107Fixture.settle();
      }, revoke);
      await expect
        .poll(() =>
          page.evaluate(
            () => window.hcp107Fixture.headlessSnapshot()?.busy ?? false
          )
        )
        .toBe(false);
      expect(await page.evaluate(() => window.hcp107Fixture.native())).toEqual(
        before
      );
      expect(await page.evaluate(() => window.hcp107Fixture.counts())).toEqual(
        calls
      );
      expect(events).toEqual([]);
      if (revoke === 'expiry')
        expect(
          await page.evaluate(() => window.hcp107Fixture.headlessText())
        ).toContain('Verified carrots');
    }
  );
}
test('original view closure revokes the mounted draft and later Send', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('close-private-marker');
  await page.evaluate(() => window.hcp107Fixture.closeView());
  await expect(editor(page)).toHaveCount(0);
  await noPrivate(page, 'close-private-marker');
});
test('protocol freshness expiry preserves text but blocks final Send', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  await editor(page).fill('stale-private-marker');
  await page.evaluate(() => window.hcp107Fixture.advance());
  await expect(editor(page)).toHaveValue('stale-private-marker');
  await expect(
    panel(page).getByRole('button', { name: 'Send request', exact: true })
  ).toBeDisabled();
  await noPrivate(page);
});
test('inbox Check waits for actual bounded EOSE without sending', async ({
  page
}) => {
  await ready(page);
  await connect(page);
  holdOwnEose = true;
  await panel(page)
    .getByRole('button', { name: 'Check current preference', exact: true })
    .click();
  await expect.poll(() => waitingEose.length).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.hcp107Fixture.setup().status)).toBe(
    'checking'
  );
  await noPrivate(page);
  holdOwnEose = false;
  for (const release of waitingEose) release();
  waitingEose = [];
  await expect(
    panel(page).getByRole('button', { name: 'Unlock inbox', exact: true })
  ).toBeVisible();
  await noPrivate(page);
});
test('Cancel during actual inbox lookup preserves text and rejects late EOSE', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('cancel-inbox-lookup-marker');
  await connect(page);
  holdOwnEose = true;
  const calls = await page.evaluate(() => window.hcp107Fixture.counts());
  await panel(page)
    .getByRole('button', { name: 'Check current preference', exact: true })
    .click();
  await expect.poll(() => waitingEose.length).toBeGreaterThan(0);
  await panel(page)
    .getByRole('button', { name: 'Cancel', exact: true })
    .click();
  await panel(page)
    .getByRole('button', { name: 'Keep editing', exact: true })
    .click();
  holdOwnEose = false;
  for (const release of waitingEose) release();
  waitingEose = [];
  await expect(editor(page)).toHaveValue('cancel-inbox-lookup-marker');
  await expect
    .poll(() => page.evaluate(() => window.hcp107Fixture.setup().busy))
    .toBe(false);
  expect(await page.evaluate(() => window.hcp107Fixture.counts())).toEqual(
    calls
  );
  await noPrivate(page, 'cancel-inbox-lookup-marker');
});
test('original deadline ends a held incomplete inbox lookup without inventing readiness', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('expired-inbox-lookup-marker');
  await connect(page);
  holdOwnEose = true;
  await panel(page)
    .getByRole('button', { name: 'Check current preference', exact: true })
    .click();
  await expect.poll(() => waitingEose.length).toBeGreaterThan(0);
  await page.evaluate(() => window.hcp107Fixture.advance());
  await expect
    .poll(() => page.evaluate(() => window.hcp107Fixture.setup().busy))
    .toBe(false);
  await expect(panel(page)).toContainText('lookup is incomplete');
  await expect(editor(page)).toHaveValue('expired-inbox-lookup-marker');
  await expect(
    panel(page).getByRole('button', { name: 'Send request', exact: true })
  ).toBeDisabled();
  await noPrivate(page);
});
test('new genuine text during held peer lookup stops the original frozen Send', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  await editor(page).fill('peer-lookup-original');
  holdPeerEose = true;
  const calls = await page.evaluate(() => window.hcp107Fixture.counts());
  await panel(page)
    .getByRole('button', { name: 'Send request', exact: true })
    .click();
  await expect.poll(() => waitingEose.length).toBeGreaterThan(0);
  expect(
    await page.evaluate(() =>
      window.hcp107Fixture.directUpdate('peer-lookup-newer')
    )
  ).toBe(true);
  holdPeerEose = false;
  for (const release of waitingEose) release();
  waitingEose = [];
  await expect
    .poll(() => page.evaluate(() => window.hcp107Fixture.snapshot()?.busy))
    .toBe(false);
  await expect(editor(page)).toHaveValue('peer-lookup-newer');
  expect(await page.evaluate(() => window.hcp107Fixture.counts())).toEqual(
    calls
  );
  await noPrivate(page);
});
test('closing the original view during held peer lookup refuses late Send', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  holdPeerEose = true;
  await panel(page)
    .getByRole('button', { name: 'Send request', exact: true })
    .click();
  await expect.poll(() => waitingEose.length).toBeGreaterThan(0);
  await page.evaluate(() => window.hcp107Fixture.closeView());
  holdPeerEose = false;
  for (const release of waitingEose) release();
  waitingEose = [];
  await expect(editor(page)).toHaveCount(0);
  await noPrivate(page);
});
test('native unload warns for unsaved private text without saving', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill('unload-private-marker');
  expect(
    await page.evaluate(() => {
      const event = new Event('beforeunload', { cancelable: true });
      window.dispatchEvent(event);
      return event.defaultPrevented;
    })
  ).toBe(true);
  await noPrivate(page, 'unload-private-marker');
});
