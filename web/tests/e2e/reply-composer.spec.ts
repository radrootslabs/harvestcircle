import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/reply-composer.ts';
import { finalizeEvent } from 'applesauce-core/helpers';
declare global {
  interface Window {
    cr007PendingSend?: Promise<unknown>;
    cr007DelayPrivateClose?: boolean;
    cr007DelayedCloses?: (() => void)[];
    hcp104: typeof Fixture;
    hcp104Fixture: Awaited<ReturnType<typeof Fixture.renderReplyFixture>>;
    hcp104Pending?: Promise<unknown>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let events: unknown[][] = [],
  requests: unknown[][] = [];
let servePeerPreference = false;
let holdPeerEose = false;
let holdPrivateAck = false;
let heldPeerRequest:
  | {
      socket: WebSocket;
      id: string;
      filter: unknown;
      eventSent: boolean;
      eoseSent: boolean;
      closed: boolean;
      closedBeforeEose: boolean;
      disconnectedBeforeCompletion: boolean;
    }
  | undefined;
const peerPreference = finalizeEvent(
  {
    kind: 10050,
    created_at: 100,
    tags: [['relay', 'wss://peer.example.org']],
    content: ''
  },
  new Uint8Array(32).fill(84)
);
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
  sockets.on('connection', (socket) => {
    socket.on('close', () => {
      if (heldPeerRequest?.socket === socket && !heldPeerRequest.closed)
        heldPeerRequest.disconnectedBeforeCompletion = true;
    });
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
          ? bytes
          : Array.isArray(bytes)
            ? Buffer.concat(bytes)
            : Buffer.from(bytes),
        frame = JSON.parse(raw.toString()) as unknown[];
      if (
        frame[0] === 'CLOSE' &&
        heldPeerRequest?.socket === socket &&
        heldPeerRequest.id === frame[1]
      ) {
        if (heldPeerRequest.eoseSent) heldPeerRequest.closed = true;
        else heldPeerRequest.closedBeforeEose = true;
      }
      if (frame[0] === 'EVENT') {
        events.push(frame);
        const event = frame[1] as { id: string };
        if (!holdPrivateAck)
          socket.send(JSON.stringify(['OK', event.id, true, '']));
      }
      if (frame[0] === 'REQ') {
        requests.push(frame);
        const filter = frame[2] as { kinds?: number[]; authors?: string[] };
        if (
          servePeerPreference &&
          filter.kinds?.includes(10050) &&
          filter.authors?.includes(peerPreference.pubkey)
        ) {
          if (holdPeerEose) {
            if (heldPeerRequest) throw Error('duplicate held peer request');
            heldPeerRequest = {
              socket,
              id: String(frame[1]),
              filter: frame[2],
              eventSent: true,
              eoseSent: false,
              closed: false,
              closedBeforeEose: false,
              disconnectedBeforeCompletion: false
            };
          }
          socket.send(JSON.stringify(['EVENT', frame[1], peerPreference]));
          if (holdPeerEose) return;
        }
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      }
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
          new URL('./harness/reply-composer.ts', import.meta.url)
        )
      }
    },
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/reply-composer.ts', import.meta.url)
        ),
        name: 'hcp104',
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
          this.cr007Private =
            new URL(String(url)).origin !== 'wss://discovery.example.org';
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
}
test.beforeEach(async ({ page }) => {
  events = [];
  requests = [];
  servePeerPreference = false;
  holdPeerEose = false;
  holdPrivateAck = false;
  heldPeerRequest = undefined;
  await load(page);
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(async () => {
      window.hcp104Fixture?.settle();
      await window.hcp104Pending;
      window.cr007DelayPrivateClose = false;
      for (const release of window.cr007DelayedCloses ?? []) release();
      await window.hcp104Fixture?.close();
    });
});
async function ready(
  page: import('@playwright/test').Page,
  qualified = true,
  production = false
) {
  await page.evaluate(
    async ({ qualified, production }) => {
      window.hcp104Fixture = await window.hcp104.renderReplyFixture(
        qualified,
        production
      );
    },
    { qualified, production }
  );
  await expect(
    page.locator('[data-reply-fixture]').getByLabel('Reply', { exact: true })
  ).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const native = await window.hcp104Fixture.native();
        return native.received.every((row) => {
          const record = JSON.parse((row as { wire: string }).wire) as {
            read: unknown;
          };
          return record.read !== null;
        });
      })
    )
    .toBe(true);
}
const composer = (page: import('@playwright/test').Page) =>
  page.locator('[data-reply-fixture]');
const sentinel = 'HCP104_UNSENT_MEMORY_ONLY & <private>\nThursday pickup?';

for (const crossSecond of [false, true]) {
  test(
    crossSecond
      ? 'CR003 production reply reserves across a real owner second boundary'
      : 'production explicit Send renews an expired peer lookup without losing the draft or automatically sending',
    async ({ page }) => {
      await ready(page, true, true);
      await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
      const before = await page.evaluate(async () => ({
        native: await window.hcp104Fixture.native(),
        delta: window.hcp104Fixture.delta()
      }));
      expect(requests).toEqual([]);
      expect(await page.evaluate(() => window.hcp104Fixture.send())).toEqual({
        status: 'needs_action'
      });
      await expect.poll(() => requests.length).toBe(1);
      expect(requests[0][2]).toMatchObject({
        kinds: [10050],
        authors: [peerPreference.pubkey],
        limit: 200
      });
      // An active missing-peer run is reused. Neither its EOSE nor another Send
      // silently resets the bounded lookup or invents private readiness.
      expect(await page.evaluate(() => window.hcp104Fixture.send())).toEqual({
        status: 'needs_action'
      });
      expect(requests).toHaveLength(1);
      expect(events).toEqual([]);
      await page.evaluate(async () => {
        window.hcp104Fixture.advance();
        await window.hcp104Fixture.refreshSetup();
      });
      expect(requests).toHaveLength(1);
      expect(await page.evaluate(() => window.hcp104Fixture.text())).toBe(
        sentinel
      );
      expect(
        await page.evaluate(async () => ({
          native: await window.hcp104Fixture.native(),
          delta: window.hcp104Fixture.delta()
        }))
      ).toEqual(before);
      servePeerPreference = true;
      holdPeerEose = true;
      expect(await page.evaluate(() => window.hcp104Fixture.send())).toEqual({
        status: 'needs_action'
      });
      // The unchanged producer never issues this second original runtime query.
      await expect.poll(() => requests.length).toBe(2);
      expect(requests[1][2]).toEqual(requests[0][2]);
      expect(events).toEqual([]);
      expect(await page.evaluate(() => window.hcp104Fixture.text())).toBe(
        sentinel
      );
      expect(heldPeerRequest?.id).toBe(requests[1][1]);
      expect(heldPeerRequest?.filter).toEqual(requests[1][2]);
      expect(heldPeerRequest).toMatchObject({
        eventSent: true,
        eoseSent: false,
        closed: false,
        closedBeforeEose: false,
        disconnectedBeforeCompletion: false
      });
      expect(
        await page.evaluate(() => window.hcp104Fixture.snapshot()?.state)
      ).toBe('needs_action');
      expect(
        await page.evaluate(async () => ({
          native: await window.hcp104Fixture.native(),
          delta: window.hcp104Fixture.delta()
        }))
      ).toEqual(before);
      // Server REQ arrival is not consumer readiness. With the original run's
      // virtual clock held, this exact subscription can finish only after its
      // actual signed EVENT and released EOSE are consumed and cleaned up.
      const originalPeerRequest = heldPeerRequest;
      if (!originalPeerRequest) throw Error('missing original peer request');
      originalPeerRequest.eoseSent = true;
      originalPeerRequest.socket.send(
        JSON.stringify(['EOSE', originalPeerRequest.id])
      );
      await expect.poll(() => originalPeerRequest.closed).toBe(true);
      expect(originalPeerRequest.closedBeforeEose).toBe(false);
      expect(originalPeerRequest.disconnectedBeforeCompletion).toBe(false);
      expect(requests).toHaveLength(2);
      expect(events).toEqual([]);
      await page.evaluate(
        (crossSecond) => window.hcp104Fixture.configureSendClock(crossSecond),
        crossSecond
      );
      await page.evaluate(() => window.hcp104Fixture.resetDiagnostics());
      const beforeSend = await page.evaluate(() =>
        window.hcp104Fixture.diagnostic()
      );
      await composer(page)
        .getByRole('button', { name: 'Send reply', exact: true })
        .click();
      // Observe actual completion before inspecting the owned operation.
      // Observe completion of the attempt before asserting its complete outcome.
      await expect
        .poll(() => page.evaluate(() => window.hcp104Fixture.snapshot()?.busy))
        .toBe(false);
      await page.evaluate(() => window.hcp104Fixture.restoreSendClock());
      const terminal = await page.evaluate(async () => ({
        snapshot: window.hcp104Fixture.snapshot(),
        diagnostic: await window.hcp104Fixture.diagnostic(),
        native: await window.hcp104Fixture.native(),
        delta: window.hcp104Fixture.delta()
      }));
      if (crossSecond) {
        const samples = terminal.diagnostic.clockSamples;
        expect(Math.floor(samples[1].value / 1000)).toBeGreaterThan(
          Math.floor(samples[0].value / 1000)
        );
        expect(terminal.diagnostic.nativeTransactions.length).toBeGreaterThan(
          0
        );
        expect(
          terminal.diagnostic.nativeTransactions.every(
            (row) => row.outcome === 'complete'
          )
        ).toBe(true);
      }
      expect(
        terminal,
        JSON.stringify({ beforeSend, terminal, published: events.length })
      ).toMatchObject({
        snapshot: { state: 'completed', busy: false }
      });
      expect(requests).toHaveLength(2);
      expect(events).toHaveLength(2);
      expect(
        await page.evaluate(() => window.hcp104Fixture.delta())
      ).toMatchObject({
        encrypts: before.delta.encrypts + 2,
        signs: before.delta.signs + 2
      });
    }
  );
}

test('typing is memory only with no autosave SDK discovery publication or order transition', async ({
  page
}) => {
  const messages: string[] = [];
  page.on('console', (message) => messages.push(message.text()));
  page.on('pageerror', (error) => messages.push(error.message));
  await ready(page);
  const before = await page.evaluate(async () => ({
    native: await window.hcp104Fixture.native(),
    delta: window.hcp104Fixture.delta()
  }));
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  await expect(composer(page)).toContainText('unsaved memory');
  await expect(composer(page)).toContainText('does not accept an order');
  expect(await page.evaluate(() => window.hcp104Fixture.text())).toBe(sentinel);
  expect(await page.evaluate(() => window.hcp104Fixture.lookups())).toBe(0);
  expect(
    await page.evaluate(async () => ({
      native: await window.hcp104Fixture.native(),
      delta: window.hcp104Fixture.delta()
    }))
  ).toEqual(before);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
  expect(
    await composer(page)
      .getByRole('button', { name: /Accept order/i })
      .count()
  ).toBe(0);
  expect(
    await page.evaluate(
      (text) =>
        JSON.stringify({
          local: { ...localStorage },
          session: { ...sessionStorage }
        }).includes(text),
      sentinel
    )
  ).toBe(false);
  // This controlled composer is mounted on Search, not a private route.
  // Real private title/noindex qualification is in privacy-output.spec.ts.
  expect(await page.locator('head').textContent()).not.toContain(sentinel);
  expect(page.url()).not.toContain(encodeURIComponent(sentinel));
  expect(messages.join('\n')).not.toContain(sentinel);
  await expect(composer(page).getByText(/unsaved memory/)).toBeVisible();
  await expect(
    composer(page).getByText('It does not accept an order.', { exact: false })
  ).toBeVisible();
});

test('explicit Send creates an actual self-first paired reply with the authenticated peer and direct inner parent', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  // This controlled-provider source case exercises successful paired delivery.
  // The production scenarios above also use an advancing actual wall clock.
  await page.evaluate(() => window.hcp104Fixture.configureSendClock(false));
  const before = await page.evaluate(() => window.hcp104Fixture.delta());
  await composer(page)
    .getByRole('button', { name: 'Send reply', exact: true })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.hcp104Fixture.snapshot()?.busy))
    .toBe(false);
  const terminal = await page.evaluate(async () => ({
    snapshot: window.hcp104Fixture.snapshot(),
    diagnostic: await window.hcp104Fixture.diagnostic()
  }));
  expect(terminal, JSON.stringify(terminal)).toMatchObject({
    snapshot: { state: 'completed', busy: false }
  });
  expect(events).toHaveLength(2);
  expect(
    events.every((frame) => (frame[1] as { kind: number }).kind === 1059)
  ).toBe(true);
  expect(await page.evaluate(() => window.hcp104Fixture.delta())).toMatchObject(
    { encrypts: before.encrypts + 2, signs: before.signs + 2 }
  );
  await expect(composer(page)).toContainText('Saved encrypted in this browser');
  await expect(composer(page)).toContainText(
    'Accepted by recipient inbox relay'
  );
  expect(
    await page.evaluate(() => window.hcp104Fixture.list().requests)
  ).toEqual([]);
  expect(
    await page.evaluate(() =>
      window.hcp104Fixture.list().conversations.map((row) => row.peer)
    )
  ).toEqual([await page.evaluate(() => window.hcp104Fixture.peer)]);
  const { nip44 } = await import('applesauce-core/helpers/encryption');
  const { getEventHash } = await import('applesauce-core/helpers');
  const owner = await page.evaluate(() => window.hcp104Fixture.owner),
    peer = await page.evaluate(() => window.hcp104Fixture.peer),
    parent = await page.evaluate(() => window.hcp104Fixture.parent);
  const rumors = events.map((frame) => {
    const outer = frame[1] as {
      pubkey: string;
      content: string;
      tags: string[][];
    };
    const key = new Uint8Array(32).fill(outer.tags[0][1] === owner ? 83 : 84);
    const seal = JSON.parse(
      nip44.v2.decrypt(
        outer.content,
        nip44.v2.utils.getConversationKey(key, outer.pubkey)
      )
    ) as { pubkey: string; content: string; kind: number; tags: string[][] };
    expect(seal.kind).toBe(13);
    expect(seal.tags).toEqual([]);
    expect(seal.pubkey).toBe(owner);
    const rumor = JSON.parse(
      nip44.v2.decrypt(
        seal.content,
        nip44.v2.utils.getConversationKey(key, seal.pubkey)
      )
    ) as {
      id: string;
      kind: number;
      pubkey: string;
      tags: string[][];
      content: string;
      created_at: number;
    };
    expect(rumor.kind).toBe(14);
    expect(rumor.pubkey).toBe(owner);
    expect(rumor.content).toBe(sentinel);
    expect(rumor.tags).toEqual([
      ['p', peer],
      ['e', parent]
    ]);
    expect(getEventHash(rumor)).toBe(rumor.id);
    key.fill(0);
    return rumor;
  });
  expect(rumors[0]).toEqual(rumors[1]);
  expect(requests).toEqual([]);
});

for (const outcome of [
  'busy',
  'confirmed_abort',
  'unknown_after_commit'
] as const)
  test(`CR003 actual reply ${outcome} retains draft with the correct retry disposition`, async ({
    page
  }) => {
    await ready(page);
    await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
    const before = await page.evaluate(async () => ({
      native: await window.hcp104Fixture.native(),
      delta: window.hcp104Fixture.delta()
    }));
    await page.evaluate(async (outcome) => {
      let restore = () => {};
      if (outcome === 'busy') {
        let entered = () => {},
          release = () => {};
        const ready = new Promise<void>((resolve) => {
          entered = resolve;
        });
        const held = navigator.locks.request(
          'harvestcircle:owner:' + window.hcp104Fixture.owner,
          () =>
            new Promise<void>((resolve) => {
              release = resolve;
              entered();
            })
        );
        await ready;
        Reflect.set(window, 'cr003ReplyRelease', async () => {
          release();
          await held;
        });
      } else {
        const originalPut = Reflect.get<IDBObjectStore, 'put'>(
            IDBObjectStore.prototype,
            'put'
          ),
          originalAbort = Reflect.get<IDBTransaction, 'abort'>(
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
            throw Error('controlled lost result after actual queued write');
          }
          return request;
        };
        restore = () => {
          IDBObjectStore.prototype.put = originalPut;
        };
      }
      Reflect.set(window, 'cr003ReplyRestore', restore);
    }, outcome);
    try {
      await composer(page)
        .getByRole('button', { name: 'Send reply', exact: true })
        .click();
      await expect
        .poll(() => page.evaluate(() => window.hcp104Fixture.snapshot()?.busy))
        .toBe(false);
    } finally {
      await page.evaluate(async () => {
        (Reflect.get(window, 'cr003ReplyRestore') as () => void)();
        const release = Reflect.get(window, 'cr003ReplyRelease') as
          (() => Promise<void>) | undefined;
        await release?.();
        Reflect.deleteProperty(window, 'cr003ReplyRestore');
        Reflect.deleteProperty(window, 'cr003ReplyRelease');
      });
    }
    const terminal = await page.evaluate(async () => ({
      snapshot: window.hcp104Fixture.snapshot(),
      text: window.hcp104Fixture.text(),
      native: await window.hcp104Fixture.native(),
      delta: window.hcp104Fixture.delta()
    }));
    expect(terminal.text).toBe(sentinel);
    expect(terminal.delta).toEqual(before.delta);
    expect(events).toEqual([]);
    if (outcome === 'unknown_after_commit') {
      expect(terminal.snapshot).toMatchObject({
        state: 'needs_action',
        readyToSend: false
      });
      expect(terminal.native.sends).toHaveLength(
        before.native.sends.length + 1
      );
      expect(
        await page.evaluate(() =>
          window.hcp104Fixture.update('different attempted body')
        )
      ).toBe(false);
      expect(await page.evaluate(() => window.hcp104Fixture.send())).toEqual({
        status: 'needs_action'
      });
      expect(await page.evaluate(() => window.hcp104Fixture.native())).toEqual(
        terminal.native
      );
    } else {
      expect(terminal.snapshot).toMatchObject({
        state: 'idle',
        readyToSend: true
      });
      expect(terminal.native).toEqual(before.native);
      expect(
        await page.evaluate(() =>
          window.hcp104Fixture.update('CR003_TEST_ONLY_edited_reply')
        )
      ).toBe(true);
      await composer(page)
        .getByRole('button', { name: 'Send reply', exact: true })
        .click();
      await expect
        .poll(() => page.evaluate(() => window.hcp104Fixture.snapshot()?.state))
        .toBe('completed');
      expect(events).toHaveLength(2);
    }
  });

test('cancelled same-page setup retains the exact draft and clearing the gate never auto-sends', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  const before = await page.evaluate(async () => ({
    native: await window.hcp104Fixture.native(),
    delta: window.hcp104Fixture.delta()
  }));
  await page.evaluate(() => window.hcp104Fixture.gate(true));
  expect(await page.evaluate(() => window.hcp104Fixture.send())).toEqual({
    status: 'needs_action'
  });
  await expect(composer(page).getByLabel('Reply', { exact: true })).toHaveValue(
    sentinel
  );
  await page.evaluate(() => window.hcp104Fixture.gate(false));
  expect(
    await page.evaluate(async () => ({
      native: await window.hcp104Fixture.native(),
      delta: window.hcp104Fixture.delta()
    }))
  ).toEqual(before);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});

test('unexercised original inbox access leaves the memory draft honest and cannot send', async ({
  page
}) => {
  await ready(page, false);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  const before = await page.evaluate(async () => ({
    native: await window.hcp104Fixture.native(),
    delta: window.hcp104Fixture.delta()
  }));
  expect(await page.evaluate(() => window.hcp104Fixture.send())).toEqual({
    status: 'needs_action'
  });
  expect(await page.evaluate(() => window.hcp104Fixture.text())).toBe(sentinel);
  expect(
    await page.evaluate(async () => ({
      native: await window.hcp104Fixture.native(),
      delta: window.hcp104Fixture.delta()
    }))
  ).toEqual(before);
  expect(events).toEqual([]);
});

test('unreviewed Send and oversized edits retain the previous memory revision without effects', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  expect(
    await page.evaluate(() => window.hcp104Fixture.update('x'.repeat(4097)))
  ).toBe(false);
  expect(
    await page.evaluate(() => window.hcp104Fixture.send('unreviewed'))
  ).toEqual({ status: 'invalid' });
  expect(await page.evaluate(() => window.hcp104Fixture.text())).toBe(sentinel);
  expect(await page.evaluate(() => window.hcp104Fixture.lookups())).toBe(0);
  expect(events).toEqual([]);
});

test('dirty route navigation defaults to Keep editing and preserves the draft and focus', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  expect(await page.evaluate(() => window.hcp104Fixture.navigate())).toBe(true);
  await expect(
    composer(page).getByRole('button', { name: 'Keep editing', exact: true })
  ).toBeFocused();
  await composer(page)
    .getByRole('button', { name: 'Keep editing', exact: true })
    .click();
  await expect(composer(page).getByLabel('Reply', { exact: true })).toHaveValue(
    sentinel
  );
  await expect(
    composer(page).getByLabel('Reply', { exact: true })
  ).toBeFocused();
  expect(
    await page.evaluate(() => window.hcp104Fixture.destinations())
  ).toEqual([]);
  expect(events).toEqual([]);
});

test('reviewed navigation discard clears only the same draft and returns to the safe inbox', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  const before = await page.evaluate(() => window.hcp104Fixture.native());
  expect(await page.evaluate(() => window.hcp104Fixture.navigate())).toBe(true);
  await composer(page)
    .getByRole('button', { name: 'Discard draft', exact: true })
    .click();
  expect(await page.evaluate(() => window.hcp104Fixture.text())).toBe('');
  expect(
    await page.evaluate(() => window.hcp104Fixture.destinations())
  ).toEqual(['/messages']);
  expect(await page.evaluate(() => window.hcp104Fixture.native())).toEqual(
    before
  );
  expect(events).toEqual([]);
});

test('actual native unload event warns for unsaved text without attempting a save', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  const before = await page.evaluate(() => window.hcp104Fixture.delta());
  expect(
    await page.evaluate(() => {
      const event = new Event('beforeunload', { cancelable: true });
      return window.dispatchEvent(event);
    })
  ).toBe(false);
  expect(await page.evaluate(() => window.hcp104Fixture.delta())).toEqual(
    before
  );
  expect(events).toEqual([]);
});

test('unmount during the actual held seal signature retires continuations and never auto-publishes a late result', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  await page.evaluate(() => {
    window.hcp104Fixture.holdSigner();
    window.hcp104Pending = window.hcp104Fixture.send();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp104Fixture.pending()))
    .toBe(true);
  await expect(composer(page)).toContainText('Preparing');
  expect(events).toEqual([]);
  await page.evaluate(() => window.hcp104Fixture.closeComposer());
  await page.evaluate(async () => {
    window.hcp104Fixture.settle();
    await window.hcp104Pending;
  });
  expect(
    await page.evaluate(() => window.hcp104Fixture.text())
  ).toBeUndefined();
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
  expect(await page.evaluate(() => window.hcp104Fixture.delta())).toMatchObject(
    { encrypts: 1, signs: 1 }
  );
});

test('logout during actual held signer work hides the draft and refuses late peer crypto or publication', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  await page.evaluate(() => {
    window.hcp104Fixture.holdSigner();
    window.hcp104Pending = window.hcp104Fixture.send();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp104Fixture.pending()))
    .toBe(true);
  await page.evaluate(() => window.hcp104Fixture.logout());
  await page.evaluate(async () => {
    window.hcp104Fixture.settle();
    await window.hcp104Pending;
  });
  expect(
    await page.evaluate(() => window.hcp104Fixture.text())
  ).toBeUndefined();
  expect(events).toEqual([]);
  await expect(composer(page).getByLabel('Reply', { exact: true })).toHaveCount(
    0
  );
  expect(await page.locator('body').textContent()).not.toContain(sentinel);
  expect(await page.locator('head').textContent()).not.toContain(sentinel);
});

test('copied controllers and arbitrary private or product hashes cannot borrow reply custody', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  expect(await page.evaluate(() => window.hcp104Fixture.copied())).toEqual({
    update: false,
    send: { status: 'invalid' },
    navigation: 'invalid',
    keep: false,
    discard: 'invalid'
  });
  expect(
    await page.evaluate(() => window.hcp104Fixture.selectParent('f'.repeat(64)))
  ).toBe(false);
  expect(
    await page.evaluate(() =>
      window.hcp104Fixture.selectParent('nostr:naddr1invalid')
    )
  ).toBe(false);
  expect(await page.evaluate(() => window.hcp104Fixture.text())).toBe(sentinel);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});

test('explicitly leaving and reloading loses the unsaved memory draft while ciphertext remains intact', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  const before = await page.evaluate(() => window.hcp104Fixture.native());
  await page.evaluate(() => window.hcp104Fixture.close());
  await page.reload();
  expect(
    await page.evaluate(() =>
      document.body.textContent?.includes('HCP104_UNSENT_MEMORY_ONLY')
    )
  ).toBe(false);
  expect(
    await page.evaluate(
      (rows) => JSON.stringify(rows).includes('HCP104_UNSENT_MEMORY_ONLY'),
      before
    )
  ).toBe(false);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});

test('later actual admitted Room replies reject every copied custody token and a genuine different same-owner identity generation', async ({
  page
}) => {
  await ready(page);
  const before = await page.evaluate(() => window.hcp104Fixture.native());
  const admission = await page.evaluate(() => window.hcp104Fixture.admission());
  expect(admission.copied).toEqual([false, false, false, false]);
  expect(admission.foreign).toBe(false);
  const rumor = JSON.parse(admission.wire!) as {
    pubkey: string;
    tags: string[][];
    content: string;
    kind: number;
  };
  expect(rumor.pubkey).toBe(
    await page.evaluate(() => window.hcp104Fixture.owner)
  );
  expect(rumor.tags).toEqual([
    ['p', await page.evaluate(() => window.hcp104Fixture.peer)],
    ['e', await page.evaluate(() => window.hcp104Fixture.parent)]
  ]);
  expect(rumor.kind).toBe(14);
  expect(rumor.content).toBe('Local admission fixture only');
  expect(await page.evaluate(() => window.hcp104Fixture.native())).toEqual(
    before
  );
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});

test('later edit after opening dirty warning invalidates the reviewed discard revision without erasing the new draft or navigating', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  expect(await page.evaluate(() => window.hcp104Fixture.navigate())).toBe(true);
  await composer(page)
    .getByLabel('Reply', { exact: true })
    .fill(sentinel + '\nKeep this newer text');
  await composer(page)
    .getByRole('button', { name: 'Discard draft', exact: true })
    .click();
  expect(await page.evaluate(() => window.hcp104Fixture.text())).toBe(
    sentinel + '\nKeep this newer text'
  );
  expect(
    await page.evaluate(() => window.hcp104Fixture.destinations())
  ).toEqual([]);
  expect(events).toEqual([]);
  expect(requests).toEqual([]);
});

test('CR007 pending actual reply EVENT plus route close settles stopped and preserves physical cleanup', async ({
  page
}) => {
  await ready(page);
  await composer(page).getByLabel('Reply', { exact: true }).fill(sentinel);
  holdPrivateAck = true;
  await page.evaluate(() => {
    window.cr007DelayPrivateClose = true;
    window.cr007PendingSend = window.hcp104Fixture.send();
  });
  await expect.poll(() => events.length).toBe(1);
  await page.evaluate(() => window.hcp104Fixture.closeComposer());
  expect(await page.evaluate(() => window.cr007PendingSend)).toEqual({
    status: 'stopped'
  });
  expect(
    await page.evaluate(() => window.hcp104Fixture.snapshot())
  ).toBeUndefined();
  expect(
    await page.evaluate(() => window.hcp104Fixture.text())
  ).toBeUndefined();
  expect(events).toHaveLength(1);
  expect(
    await page.evaluate(() => window.cr007DelayedCloses?.length ?? 0)
  ).toBeGreaterThan(0);
  await page.evaluate(() => {
    window.cr007DelayPrivateClose = false;
    for (const release of window.cr007DelayedCloses ?? []) release();
  });
});
