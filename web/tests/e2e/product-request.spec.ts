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
    cr007PendingSend?: Promise<unknown>;
    cr007DelayPrivateClose?: boolean;
    cr007DelayedCloses?: (() => void)[];
    hcp107: typeof Fixture;
    hcp107Fixture: Awaited<ReturnType<typeof Fixture.renderFixture>>;
    hcp108SendClock: {
      samples: { milliseconds: number; phase: string; stack?: string }[];
      locks: { captured: number; callback: number; granted: boolean }[];
      transactions: string[];
      restore(): void;
    };
    hcp126HeldClock: {
      samples: {
        milliseconds: number;
        actualMilliseconds: number;
        stack?: string;
      }[];
      restore(): void;
    };
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>;
let sockets: WebSocketServer, endpoint: string, bundle: string;
let missingOwn = false;
let holdPrivateAck = false;
let listingStatus: 'active' | 'sold' | 'withdrawn' = 'active';
let sellerInbox:
  | 'normal'
  | 'missing'
  | 'unsupported'
  | 'unapproved'
  | 'malformed'
  | 'partial' = 'normal';
let incompleteListing = false;
let holdListingHead = false;
let headUpdates: (() => void)[] = [],
  deletionUpdates: (() => void)[] = [];
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
        if (!holdPrivateAck || event.kind !== 1059)
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
                ['status', listingStatus === 'sold' ? 'sold' : 'active']
              ]
            },
            sellerSecret
          );
          socket.send(JSON.stringify(['EVENT', frame[1], event]));
          headUpdates.push(() => {
            if (socket.readyState === 1)
              socket.send(
                JSON.stringify([
                  'EVENT',
                  frame[1],
                  finalizeEvent(
                    {
                      ...event,
                      created_at: 1700000080,
                      tags: event.tags.map((tag) =>
                        tag[0] === 'status'
                          ? [
                              'status',
                              listingStatus === 'sold' ? 'sold' : 'active'
                            ]
                          : tag
                      )
                    },
                    sellerSecret
                  )
                ])
              );
          });
        }
        if (filter.kinds?.includes(5))
          deletionUpdates.push(() => {
            if (socket.readyState === 1)
              socket.send(
                JSON.stringify([
                  'EVENT',
                  frame[1],
                  finalizeEvent(
                    {
                      kind: 5,
                      created_at: 1700000090,
                      tags: [['a', '30402:' + peer + ':carrots']],
                      content: ''
                    },
                    sellerSecret
                  )
                ])
              );
          });
        if (filter.kinds?.includes(5) && listingStatus === 'withdrawn')
          socket.send(
            JSON.stringify([
              'EVENT',
              frame[1],
              finalizeEvent(
                {
                  kind: 5,
                  created_at: 1700000070,
                  tags: [['a', '30402:' + peer + ':carrots']],
                  content: ''
                },
                sellerSecret
              )
            ])
          );
        if (filter.kinds?.includes(10050)) {
          if (filter.authors?.includes(peer) && sellerInbox !== 'missing')
            socket.send(
              JSON.stringify([
                'EVENT',
                frame[1],
                sellerInbox === 'normal' || sellerInbox === 'partial'
                  ? preference(sellerSecret, 'wss://peer.example.org')
                  : finalizeEvent(
                      {
                        kind: 10050,
                        created_at: 1700000060,
                        content: '',
                        tags:
                          sellerInbox === 'unapproved'
                            ? [['relay', 'wss://unapproved.example.org']]
                            : sellerInbox === 'unsupported'
                              ? [['relay', 'https://unsupported.example.org']]
                              : [['relay']]
                      },
                      sellerSecret
                    )
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
        (holdListingHead &&
          filters.some((filter) => filter.kinds?.includes(30402))) ||
        (incompleteListing &&
          filters.some((filter) => filter.kinds?.includes(5))) ||
        filters.some(
          (filter) =>
            filter.kinds?.includes(10050) &&
            ((holdOwnEose && filter.authors?.includes(owner)) ||
              ((holdPeerEose || sellerInbox === 'partial') &&
                filter.authors?.includes(peer)))
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
  holdPrivateAck = false;
  listingStatus = 'active';
  sellerInbox = 'normal';
  incompleteListing = false;
  holdListingHead = false;
  headUpdates = [];
  deletionUpdates = [];
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
          this.cr007Private = origin !== 'wss://discovery.example.org';
        }
        readonly cr007Private: boolean;
        override close(code?: number, reason?: string) {
          if (this.cr007Private && window.cr007DelayPrivateClose)
            (window.cr007DelayedCloses ??= []).push(() =>
              super.close(code, reason)
            );
          else super.close(code, reason);
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
    await page.evaluate(() => {
      window.cr007DelayPrivateClose = false;
      for (const release of window.cr007DelayedCloses ?? []) release();
      return window.hcp107Fixture?.close();
    });
});
const panel = (page: Page) => page.locator('[data-request-fixture]');
const editor = (page: Page) =>
  panel(page).getByRole('textbox', { name: 'Your message', exact: true });
for (const status of ['sold', 'withdrawn'] as const)
  test(`HCP108 known ${status} food has no new enquiry and remains readable`, async ({
    page
  }) => {
    listingStatus = status;
    await page.evaluate(async () => {
      window.hcp107Fixture = await window.hcp107.renderFixture();
    });
    await expect
      .poll(() =>
        page.evaluate(() => window.hcp107Fixture.viewSnapshot().outcome)
      )
      .toBe(status);
    await expect(
      panel(page).getByRole('button', { name: 'Message seller', exact: true })
    ).toHaveCount(0);
    expect(
      await page.evaluate(
        () => window.hcp107Fixture.viewSnapshot().reference.pubkey
      )
    ).toBe(peer);
    expect(
      await page.evaluate(() => window.hcp107Fixture.message())
    ).toBeUndefined();
    await noPrivate(page);
  });

for (const crossSecond of [false, true])
  test(
    crossSecond
      ? 'CR003 original product request reserves across a real owner second boundary'
      : 'HCP108 incomplete active terms need explicit original acknowledgement and retain signed citations',
    async ({ page }) => {
      incompleteListing = true;
      await page.evaluate(async () => {
        window.hcp107Fixture = await window.hcp107.renderFixture();
      });
      await expect
        .poll(() =>
          page.evaluate(() => window.hcp107Fixture.viewSnapshot().outcome)
        )
        .toBe('active');
      await expect(
        panel(page).getByRole('button', { name: 'Message seller', exact: true })
      ).toBeDisabled();
      expect(
        await page.evaluate(() =>
          window.hcp107Fixture.captureUncertain(undefined)
        )
      ).toBe(false);
      await panel(page)
        .getByRole('button', {
          name: 'I understand the listing status is uncertain',
          exact: true
        })
        .click();
      await panel(page)
        .getByRole('button', { name: 'Message seller', exact: true })
        .click();
      await expect(editor(page)).toBeVisible();
      const observed = await page.evaluate(() => ({
        product: window.hcp107Fixture.product(),
        view: window.hcp107Fixture.viewSnapshot(),
        message: window.hcp107Fixture.message()
      }));
      expect(observed.product?.eventId).toBe(observed.view.eventId);
      expect(observed.message?.tags).toEqual([['p', peer]]);
      expect(observed.message?.content).toContain(
        'nostr:' + observed.product?.nevent
      );
      await noPrivate(page);
      if (crossSecond)
        await editor(page).fill('HC_TEST_ONLY_unsent_boundary_request');
      await unlock(page);
      const before = await page.evaluate(async () => ({
        native: await window.hcp107Fixture.native(),
        counts: window.hcp107Fixture.counts(),
        text: window.hcp107Fixture.text()
      }));
      await page.evaluate((crossSecond) => {
        const original = Date.now;
        let phase = 'send';
        let crossed = false;
        const samples: {
          milliseconds: number;
          phase: string;
          stack?: string;
        }[] = [];
        const locks: {
          captured: number;
          callback: number;
          granted: boolean;
        }[] = [];
        const transactions: string[] = [];
        const manager = navigator.locks;
        const descriptor = Object.getOwnPropertyDescriptor(manager, 'request');
        const request = Reflect.get(manager, 'request');
        const owner = window.hcp107Fixture.owner;
        manager.request = (...args: unknown[]) => {
          const forwarded = args.slice();
          const at = args.length - 1;
          const callback = args[at] as (lock: Lock | null) => unknown;
          const options =
            args.length === 3 ? (args[1] as LockOptions) : undefined;
          forwarded[at] = async (lock: Lock | null) => {
            if (
              lock &&
              options?.ifAvailable &&
              args[0] === 'harvestcircle:owner:' + owner &&
              !crossed
            ) {
              crossed = true;
              const captured = samples.at(-1)!.milliseconds;
              if (crossSecond)
                await new Promise<void>((resolve) => {
                  function afterBoundary() {
                    if (
                      Math.floor(original() / 1000) ===
                      Math.floor(captured / 1000)
                    ) {
                      window.setTimeout(afterBoundary, 1);
                      return;
                    }
                    resolve();
                  }
                  window.setTimeout(
                    afterBoundary,
                    Math.max(
                      1,
                      (Math.floor(captured / 1000) + 1) * 1000 - original() + 1
                    )
                  );
                });
              locks.push({ captured, callback: original(), granted: true });
              phase = 'owner_reservation';
              try {
                return await callback(lock);
              } finally {
                phase = 'send';
              }
            }
            return callback(lock);
          };
          return Reflect.apply(request, manager, forwarded) as Promise<unknown>;
        };
        const prototype = IDBDatabase.prototype;
        const transactionDescriptor = Object.getOwnPropertyDescriptor(
          prototype,
          'transaction'
        )!;
        const transaction = Reflect.get(prototype, 'transaction');
        prototype.transaction = function (
          this: IDBDatabase,
          ...args: Parameters<IDBDatabase['transaction']>
        ) {
          const tx: IDBTransaction = Reflect.apply(transaction, this, args);
          if (
            tx.mode === 'readwrite' &&
            tx.objectStoreNames.contains('private_sends')
          )
            for (const outcome of ['abort', 'complete'])
              tx.addEventListener(outcome, () => transactions.push(outcome));
          return tx;
        } as typeof prototype.transaction;
        Date.now = () => {
          const milliseconds = original();
          if (samples.length < 128)
            samples.push({
              milliseconds,
              phase,
              stack: new Error().stack?.split('\n').slice(0, 5).join('\n')
            });
          return milliseconds;
        };
        window.hcp108SendClock = {
          samples,
          locks,
          transactions,
          restore: () => {
            Date.now = original;
            if (descriptor)
              Object.defineProperty(manager, 'request', descriptor);
            else Reflect.deleteProperty(manager, 'request');
            Object.defineProperty(
              prototype,
              'transaction',
              transactionDescriptor
            );
          }
        };
      }, crossSecond);
      try {
        await panel(page)
          .getByRole('button', { name: 'Send request', exact: true })
          .click();
        await expect
          .poll(() =>
            page.evaluate(() => window.hcp107Fixture.snapshot()?.busy)
          )
          .toBe(false);
        const terminal = await page.evaluate(() => {
          window.hcp108SendClock.restore();
          return {
            snapshot: window.hcp107Fixture.snapshot(),
            counts: window.hcp107Fixture.counts(),
            samples: window.hcp108SendClock.samples,
            locks: window.hcp108SendClock.locks,
            transactions: window.hcp108SendClock.transactions
          };
        });
        console.log(
          JSON.stringify({
            fixture: 'CR003_REAL_CLOCK_PRODUCT_REQUEST_DIAGNOSTIC',
            terminal
          })
        );
        if (crossSecond) {
          expect(terminal.locks).toHaveLength(1);
          expect(Math.floor(terminal.locks[0].callback / 1000)).toBeGreaterThan(
            Math.floor(terminal.locks[0].captured / 1000)
          );
          expect(terminal.transactions.length).toBeGreaterThan(0);
          expect(
            terminal.transactions.every((outcome) => outcome === 'complete')
          ).toBe(true);
        }
        expect(terminal.snapshot?.state).toBe('completed');
        expect(terminal.counts).toMatchObject({
          signs: before.counts.signs + 2,
          encrypts: before.counts.encrypts + 2
        });
        await expect
          .poll(() => events.filter((row) => row.event.kind === 1059).length)
          .toBe(2);
        expect(
          events
            .filter((row) => row.event.kind === 1059)
            .map((row) => row.origin)
            .sort()
        ).toEqual(['wss://archive.example.org', 'wss://peer.example.org']);
        await expect
          .poll(() =>
            page.evaluate(() => window.hcp107Fixture.snapshot()?.state)
          )
          .toBe('completed');
      } finally {
        await page.evaluate(() => window.hcp108SendClock.restore());
      }
    }
  );

test('HCP108 connecting as the verified publisher shows Manage and keeps unsent memory without self enquiry', async ({
  page
}) => {
  await ready(page);
  await editor(page).fill(
    'Keep this guest text after discovering my own listing'
  );
  await page.evaluate(() => window.hcp107Fixture.mode('changed_key'));
  await panel(page)
    .getByRole('button', { name: 'Connect extension', exact: true })
    .click();
  await expect(
    panel(page).getByRole('link', { name: 'Manage listing', exact: true })
  ).toBeVisible();
  await expect(
    panel(page).getByRole('button', { name: 'Send request', exact: true })
  ).toBeDisabled();
  await expect(editor(page)).toHaveValue(
    'Keep this guest text after discovering my own listing'
  );
  await noPrivate(page);
});

for (const status of [
  'missing',
  'unsupported',
  'malformed',
  'partial'
] as const)
  test(`HCP108 ${status} seller inbox cannot guess a send destination`, async ({
    page
  }) => {
    sellerInbox = status;
    await ready(page);
    await unlock(page);
    await panel(page)
      .getByRole('button', { name: 'Send request', exact: true })
      .click();
    if (status === 'partial') {
      await expect
        .poll(() => page.evaluate(() => window.hcp107Fixture.snapshot()?.state))
        .toBe('checking');
      await page.evaluate(() => window.hcp107Fixture.advance());
    }
    await expect(panel(page)).toContainText(
      status === 'missing'
        ? 'In-app messaging is not available for this seller in this prototype.'
        : status === 'unsupported' || status === 'malformed'
          ? 'The seller inbox configuration is unsupported in this prototype.'
          : 'The seller inbox could not be confirmed from the sources checked.'
    );
    await expect(
      panel(page).getByRole('button', { name: 'Send request', exact: true })
    ).toBeDisabled();
    await expect(editor(page)).toBeVisible();
    expect(
      await page.evaluate(() => window.hcp107Fixture.viewSnapshot().food?.title)
    ).toBe('Verified carrots');
    await noPrivate(page);
  });
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
test('HCP108 a signed but unapproved seller inbox is unavailable without guessed routing', async ({
  page
}) => {
  sellerInbox = 'unapproved';
  await ready(page);
  await unlock(page);
  await panel(page)
    .getByRole('button', { name: 'Send request', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp107Fixture.snapshot()?.state))
    .toBe('needs_action');
  expect(
    await page.evaluate(() => window.hcp107Fixture.snapshot()?.copy)
  ).toContain(
    'The seller inbox configuration is unsupported in this prototype.'
  );
  await noPrivate(page);
});
async function uncertain(page: Page) {
  incompleteListing = true;
  holdListingHead = true;
  await page.evaluate(async () => {
    window.hcp107Fixture = await window.hcp107.renderFixture();
  });
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp107Fixture.viewSnapshot().outcome)
    )
    .toBe('active');
  await panel(page)
    .getByRole('button', {
      name: 'I understand the listing status is uncertain',
      exact: true
    })
    .click();
  await panel(page)
    .getByRole('button', { name: 'Message seller', exact: true })
    .click();
  await expect(editor(page)).toBeVisible();
}
test('HCP108 copied consent and genuine consent from another original view cannot acknowledge uncertain terms', async ({
  page
}) => {
  await uncertain(page);
  await page.evaluate(() => window.hcp107Fixture.createOtherView());
  await expect
    .poll(() =>
      page.evaluate(() => window.hcp107Fixture.otherViewSnapshot()?.outcome)
    )
    .toBe('active');
  expect(
    await page.evaluate(() => window.hcp107Fixture.consentCases())
  ).toEqual({
    genuine: true,
    current: true,
    copied: false,
    other: false,
    otherControl: true,
    unreviewed: false
  });
  await noPrivate(page);
});
for (const change of ['coverage', 'close'] as const)
  test(`HCP108 original ${change} transition revokes genuine uncertainty consent`, async ({
    page
  }) => {
    await uncertain(page);
    expect(
      (await page.evaluate(() => window.hcp107Fixture.consentCases())).current
    ).toBe(true);
    await editor(page).fill('uncertainty-memory-marker');
    if (change === 'coverage')
      await page.evaluate(() => window.hcp107Fixture.advance());
    else await page.evaluate(() => window.hcp107Fixture.closeView());
    expect(
      await page.evaluate(() => window.hcp107Fixture.consentCurrent())
    ).toBe(false);
    expect(
      await page.evaluate(() => window.hcp107Fixture.message())
    ).toBeUndefined();
    if (change === 'coverage')
      expect(
        await page.evaluate(() => window.hcp107Fixture.product()?.text)
      ).toBe('uncertainty-memory-marker');
    else
      expect(
        await page.evaluate(() => window.hcp107Fixture.product())
      ).toBeUndefined();
    await noPrivate(page);
  });
for (const change of ['sold', 'withdrawn', 'version'] as const)
  test(`HCP108 a real newer ${change} observation revokes request consent while preserving unsent text`, async ({
    page
  }) => {
    await uncertain(page);
    expect(
      (await page.evaluate(() => window.hcp107Fixture.consentCases())).current
    ).toBe(true);
    await editor(page).fill('changed-listing-memory-marker');
    const before = await page.evaluate(
      () => window.hcp107Fixture.product()?.eventId
    );
    if (change === 'withdrawn') {
      listingStatus = 'withdrawn';
      for (const update of deletionUpdates) update();
    } else {
      listingStatus = change === 'sold' ? 'sold' : 'active';
      for (const update of headUpdates) update();
    }
    if (change === 'version')
      await expect
        .poll(() =>
          page.evaluate(() => window.hcp107Fixture.viewSnapshot().eventId)
        )
        .not.toBe(before);
    else
      await expect
        .poll(() =>
          page.evaluate(() => window.hcp107Fixture.viewSnapshot().outcome)
        )
        .toBe(change);
    expect(
      await page.evaluate(() => window.hcp107Fixture.consentCurrent())
    ).toBe(false);
    expect(
      await page.evaluate(() => window.hcp107Fixture.product()?.text)
    ).toBe('changed-listing-memory-marker');
    expect(
      await page.evaluate(() => window.hcp107Fixture.message())
    ).toBeUndefined();
    await expect(
      panel(page).getByRole('button', { name: 'Send request', exact: true })
    ).toBeDisabled();
    await noPrivate(page);
  });
for (const change of ['cancel', 'account'] as const)
  test(`HCP108 retired peer lookup cannot replace newer ${change} state`, async ({
    page
  }) => {
    await ready(page);
    await unlock(page);
    holdPeerEose = true;
    await panel(page)
      .getByRole('button', { name: 'Send request', exact: true })
      .click();
    await expect.poll(() => waitingEose.length).toBeGreaterThan(0);
    if (change === 'cancel') {
      await panel(page)
        .getByRole('button', { name: 'Cancel', exact: true })
        .click();
      await panel(page)
        .getByRole('button', { name: 'Keep editing', exact: true })
        .click();
    } else await page.evaluate(() => window.hcp107Fixture.logout());
    holdPeerEose = false;
    for (const release of waitingEose) release();
    await expect
      .poll(() => page.evaluate(() => window.hcp107Fixture.snapshot()?.busy))
      .toBe(false);
    expect(
      await page.evaluate(() => window.hcp107Fixture.snapshot()?.copy)
    ).toContain(
      change === 'cancel' ? 'Sending is paused.' : 'Connection changed.'
    );
    await noPrivate(page);
  });
for (const change of ['sold', 'withdrawn', 'version'] as const)
  test(`HCP108 ${change} during peer lookup reports listing review without relabelling a seller inbox failure`, async ({
    page
  }) => {
    await uncertain(page);
    await unlock(page);
    holdPeerEose = true;
    await panel(page)
      .getByRole('button', { name: 'Send request', exact: true })
      .click();
    await expect
      .poll(() => page.evaluate(() => window.hcp107Fixture.snapshot()?.state))
      .toBe('checking');
    if (change === 'withdrawn') {
      listingStatus = 'withdrawn';
      for (const update of deletionUpdates) update();
    } else {
      listingStatus = change === 'sold' ? 'sold' : 'active';
      for (const update of headUpdates) update();
    }
    await expect
      .poll(() => page.evaluate(() => window.hcp107Fixture.snapshot()?.busy))
      .toBe(false);
    expect(
      await page.evaluate(() => window.hcp107Fixture.snapshot()?.copy)
    ).toContain('Review the current listing before sending.');
    await noPrivate(page);
  });
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
  const before = await page.evaluate(async () => ({
    native: await window.hcp107Fixture.native(),
    counts: window.hcp107Fixture.counts()
  }));
  await page.evaluate(() => {
    const original = Date.now;
    const fixed = original();
    const samples: {
      milliseconds: number;
      actualMilliseconds: number;
      stack?: string;
    }[] = [];
    Date.now = () => {
      const milliseconds = fixed;
      if (samples.length < 128)
        samples.push({
          milliseconds,
          actualMilliseconds: original(),
          stack: new Error().stack?.split('\n').slice(0, 5).join('\n')
        });
      return milliseconds;
    };
    window.hcp126HeldClock = {
      samples,
      restore: () => {
        Date.now = original;
      }
    };
  });
  try {
    await panel(page)
      .getByRole('button', { name: 'Send request', exact: true })
      .click();
    await expect
      .poll(() =>
        page.evaluate(() => {
          const snapshot = window.hcp107Fixture.snapshot();
          return (
            window.hcp107Fixture.pending() ||
            (snapshot?.state === 'needs_action' && !snapshot.busy)
          );
        })
      )
      .toBe(true);
    expect(await page.evaluate(() => window.hcp107Fixture.pending())).toBe(
      true
    );
    await page.evaluate(() => {
      window.hcp126HeldClock.restore();
      window.hcp107Fixture.logout();
      window.hcp107Fixture.settle();
    });
    await expect
      .poll(() => page.evaluate(() => window.hcp107Fixture.pending()))
      .toBe(false);
    expect(events).toEqual([]);
  } finally {
    const terminal = await page.evaluate(async () => {
      window.hcp126HeldClock.restore();
      return {
        snapshot: window.hcp107Fixture.snapshot(),
        pending: window.hcp107Fixture.pending(),
        native: await window.hcp107Fixture.native(),
        counts: window.hcp107Fixture.counts(),
        text: window.hcp107Fixture.text(),
        samples: window.hcp126HeldClock.samples
      };
    });
    console.log(
      JSON.stringify({
        fixture: 'HCP126_ORIGINAL_HELD_SEND_DIAGNOSTIC',
        before,
        terminal,
        events
      })
    );
  }
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

for (const outcome of [
  'storage_denied',
  'busy',
  'confirmed_abort',
  'unknown_after_commit'
] as const)
  test(`CR003 actual ${outcome} keeps draft and permits only truthful explicit retry`, async ({
    page
  }) => {
    await ready(page);
    await unlock(page);
    await editor(page).fill('CR003_TEST_ONLY_original_unsent_question');
    const before = await page.evaluate(async () => ({
      counts: window.hcp107Fixture.counts(),
      native: await window.hcp107Fixture.native()
    }));
    await page.evaluate(async (outcome) => {
      if (outcome === 'storage_denied') {
        const descriptor = Object.getOwnPropertyDescriptor(indexedDB, 'open');
        indexedDB.open = () => {
          throw new DOMException('Controlled denied storage', 'SecurityError');
        };
        Reflect.set(window, 'cr003RestorePut', () => {
          if (descriptor) Object.defineProperty(indexedDB, 'open', descriptor);
          else Reflect.deleteProperty(indexedDB, 'open');
        });
        return;
      }
      if (outcome === 'busy') {
        let entered = () => {},
          release = () => {};
        const ready = new Promise<void>((resolve) => {
          entered = resolve;
        });
        const held = navigator.locks.request(
          'harvestcircle:owner:' + window.hcp107Fixture.owner,
          () =>
            new Promise<void>((resolve) => {
              release = resolve;
              entered();
            })
        );
        await ready;
        Reflect.set(window, 'cr003RestorePut', async () => {
          release();
          await held;
        });
        return;
      }
      const originalPut = Reflect.get<IDBObjectStore, 'put'>(
        IDBObjectStore.prototype,
        'put'
      );
      const originalAbort = Reflect.get<IDBTransaction, 'abort'>(
        IDBTransaction.prototype,
        'abort'
      );
      let intercepted = false;
      IDBObjectStore.prototype.put = function (...args) {
        if (this.name !== 'private_sends' || intercepted)
          return Reflect.apply(originalPut, this, args);
        intercepted = true;
        const request = Reflect.apply(originalPut, this, args);
        if (outcome === 'confirmed_abort')
          request.addEventListener(
            'success',
            () => Reflect.apply(originalAbort, this.transaction, []),
            { once: true }
          );
        else {
          this.transaction.abort = () => {
            throw Error('controlled lost abort capability');
          };
          throw Error('controlled lost put result after actual queued write');
        }
        return request;
      };
      // Only the actual trusted prototype's put is intercepted for one command.
      Reflect.set(window, 'cr003RestorePut', () => {
        IDBObjectStore.prototype.put = originalPut;
      });
    }, outcome);
    try {
      await panel(page)
        .getByRole('button', { name: 'Send request', exact: true })
        .click();
      await expect
        .poll(() => page.evaluate(() => window.hcp107Fixture.snapshot()?.busy))
        .toBe(false);
    } finally {
      await page.evaluate(async () => {
        const restore = Reflect.get(
          window,
          'cr003RestorePut'
        ) as () => void | Promise<void>;
        await restore();
        Reflect.deleteProperty(window, 'cr003RestorePut');
      });
    }
    const terminal = await page.evaluate(async () => ({
      snapshot: window.hcp107Fixture.snapshot(),
      text: window.hcp107Fixture.text(),
      counts: window.hcp107Fixture.counts(),
      native: await window.hcp107Fixture.native()
    }));
    expect(terminal.text).toBe('CR003_TEST_ONLY_original_unsent_question');
    expect(terminal.counts).toEqual(before.counts);
    expect(events.filter((row) => row.event.kind === 1059)).toEqual([]);
    if (outcome !== 'unknown_after_commit') {
      expect(terminal.snapshot).toMatchObject({
        state: 'idle',
        readyToSend: true
      });
      expect(terminal.native).toEqual(before.native);
      expect(
        await page.evaluate(() =>
          window.hcp107Fixture.update('CR003_TEST_ONLY_edited_after_abort')
        )
      ).toBe(true);
      await panel(page)
        .getByRole('button', { name: 'Send request', exact: true })
        .click();
      await expect
        .poll(() => page.evaluate(() => window.hcp107Fixture.snapshot()?.state))
        .toBe('completed');
      expect(events.filter((row) => row.event.kind === 1059)).toHaveLength(2);
    } else {
      expect(terminal.snapshot).toMatchObject({
        state: 'needs_action',
        readyToSend: false
      });
      expect(terminal.native.sends).toHaveLength(
        before.native.sends.length + 1
      );
      expect(
        await page.evaluate(() =>
          window.hcp107Fixture.update('attempted different body')
        )
      ).toBe(false);
      expect(await page.evaluate(() => window.hcp107Fixture.send())).toEqual({
        status: 'needs_action'
      });
      expect(await page.evaluate(() => window.hcp107Fixture.native())).toEqual(
        terminal.native
      );
      expect(events.filter((row) => row.event.kind === 1059)).toEqual([]);
    }
  });

test('CR007 pending actual product request EVENT plus route close settles stopped and preserves cleanup', async ({
  page
}) => {
  await ready(page);
  await unlock(page);
  holdPrivateAck = true;
  await page.evaluate(() => {
    window.cr007DelayPrivateClose = true;
    window.cr007PendingSend = window.hcp107Fixture.beginHeadless();
  });
  await expect
    .poll(() => events.filter(({ event }) => event.kind === 1059).length)
    .toBe(1);
  await page.evaluate(() => window.hcp107Fixture.closeView());
  expect(await page.evaluate(() => window.cr007PendingSend)).toEqual({
    status: 'stopped'
  });
  expect(
    await page.evaluate(() => window.hcp107Fixture.headlessSnapshot())
  ).toBeUndefined();
  expect(events.filter(({ event }) => event.kind === 1059)).toHaveLength(1);
  expect(
    await page.evaluate(() => window.cr007DelayedCloses?.length ?? 0)
  ).toBeGreaterThan(0);
  await page.evaluate(() => {
    window.cr007DelayPrivateClose = false;
    for (const release of window.cr007DelayedCloses ?? []) release();
  });
});
