import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/private-blocks.ts';
declare global {
  interface Window {
    hcp123: typeof Fixture;
    hcp123Fixture: Awaited<ReturnType<typeof Fixture.createFixture>>;
    hcp123Pending?: Promise<boolean>;
    hcp123Queue?: Awaited<ReturnType<typeof Fixture.createQueueFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string | undefined;
let holdRequests = false;
const heldReplies: (() => void)[] = [];
let events: unknown[][] = [],
  queries: unknown[][] = [];
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
  sockets.on('connection', (socket) =>
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
      if (frame[0] === 'EVENT' || frame[0] === 'AUTH') events.push(frame);
      if (frame[0] === 'REQ') {
        queries.push(frame);
        const finish = () => {
          if (socket.readyState === 1)
            socket.send(JSON.stringify(['EOSE', frame[1]]));
        };
        if (holdRequests) heldReplies.push(finish);
        else finish();
      }
    })
  );
});
test.afterAll(async () => {
  for (const socket of sockets?.clients ?? []) socket.terminate();
  if (sockets)
    await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server?.close();
});
test.beforeEach(async ({ page }) => {
  events = [];
  queries = [];
  holdRequests = false;
  heldReplies.length = 0;
  await page.addInitScript((endpoint) => {
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
          throw Error('no live relay authorization');
        super(endpoint, protocols);
      }
    };
  }, endpoint);
});
test.afterEach(async ({ page }) => {
  holdRequests = false;
  for (const finish of heldReplies.splice(0)) finish();
  if (!page.isClosed())
    await page.evaluate(async () => {
      window.hcp123Fixture?.mode('normal');
      window.hcp123Fixture?.settle();
      window.hcp123Fixture?.releaseInventory();
      window.hcp123Queue?.mode('normal');
      window.hcp123Queue?.settle();
      await window.hcp123Pending;
      await window.hcp123Fixture?.close();
      window.hcp123Queue?.close();
    });
});
async function start(
  page: Page,
  scenario: 'inbound' | 'backfill' | 'contexts' = 'inbound'
) {
  await loadFixtureScripts(page);
  await page.evaluate(async (scenario) => {
    window.hcp123Fixture = await window.hcp123.createFixture(scenario);
  }, scenario);
}
async function loadFixtureScripts(page: Page) {
  await page.goto(server.url + '/privacy');
  if (!bundle) {
    const built = await build({
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
            new URL('./harness/private-blocks.ts', import.meta.url)
          ),
          name: 'hcp123',
          formats: ['iife']
        }
      }
    });
    const result = Array.isArray(built) ? built[0] : built;
    if (!('output' in result)) throw Error('fixture bundle unavailable');
    const chunks = result.output.filter((row) => row.type === 'chunk');
    expect(chunks).toHaveLength(1);
    bundle = chunks[0].code;
  }
  await page.addScriptTag({ content: bundle });
}
async function unlocked(page: Page) {
  await page.evaluate(() => window.hcp123Fixture.checkSetup());
  const result = await page.evaluate(async () => {
    const success = await window.hcp123Fixture.unlock();
    const { state, reason, queued, count } = window.hcp123Fixture.snapshot();
    const preferences = window.hcp123Fixture.preferences();
    return {
      success,
      diagnostic: {
        state,
        reason,
        queued,
        count,
        preferences: preferences.state
      }
    };
  });
  expect(result.success, JSON.stringify(result.diagnostic)).toBe(true);
  await expect(page.locator('[data-message-id]')).not.toHaveCount(0);
  // Wait for the original displayed-read transaction to release its native
  // owner lock before explicitly testing another independent owner mutation.
  const displayed = await page.locator('[data-message-id]').count();
  await expect
    .poll(async () => {
      const native = await page.evaluate(() => window.hcp123Fixture.native());
      return native.received.filter(
        (row) =>
          (JSON.parse((row as { wire: string }).wire) as { read: unknown })
            .read !== null
      ).length;
    })
    .toBe(displayed);
  await page.evaluate(() => window.hcp123Fixture.waitOwner());
}
test('HCP123 SSR privacy discloses reversible local blocking without account inventory', async ({
  page
}) => {
  await page.goto(server.url + '/privacy');
  expect(await page.locator('main').innerText()).toContain(
    'Blocking a person is local'
  );
  await expect(page.getByRole('button', { name: 'Undo block' })).toHaveCount(0);
  expect(events).toEqual([]);
  expect(queries).toEqual([]);
});
test('HCP123 genuine authenticated inner peer block removes thread and list previews while retaining evidence', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  const before = await page.evaluate(() => window.hcp123Fixture.native());
  await page
    .getByText('Identity and conversation options', { exact: true })
    .click();
  await page.getByRole('button', { name: 'Block person', exact: true }).click();
  await expect(
    page.getByText(/Blocking hides messages and previews/)
  ).toBeVisible();
  await page
    .getByRole('button', { name: 'Confirm local block', exact: true })
    .click();
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp123Fixture.list())).toMatchObject({
    requests: [],
    conversations: []
  });
  expect(await page.evaluate(() => window.hcp123Fixture.native())).toEqual(
    before
  );
  expect(
    await page.evaluate(() => window.hcp123Fixture.preferences())
  ).toMatchObject({
    state: 'ready',
    peers: [await page.evaluate(() => window.hcp123Fixture.peer)]
  });
  expect(events).toEqual([]);
  expect(queries).toEqual([]);
});
test('HCP123 Privacy Undo starts no decrypt signer or relay work and restores eligible cached projection', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  expect(await page.evaluate(() => window.hcp123Fixture.block())).toEqual({
    status: 'saved'
  });
  const before = await page.evaluate(() => window.hcp123Fixture.delta());
  await page.getByRole('button', { name: 'Undo block', exact: true }).click();
  await expect(page.locator('[data-message-id]')).toHaveCount(1);
  expect(await page.evaluate(() => window.hcp123Fixture.delta())).toEqual(
    before
  );
  expect(events).toEqual([]);
  expect(queries).toEqual([]);
});
test('HCP123 copied room or displayed peer cannot authorize a private block', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  expect(await page.evaluate(() => window.hcp123Fixture.fake())).toEqual({
    status: 'invalid'
  });
  expect(await page.evaluate(() => window.hcp123Fixture.rows())).toEqual([]);
  await expect(page.locator('[data-message-id]')).toHaveCount(1);
});
test('HCP123 corrupt preferences fail closed without resetting stored facts', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  await page.evaluate(() => window.hcp123Fixture.corrupt());
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp123Fixture.rows())).toHaveLength(
    1
  );
  expect(await page.evaluate(() => window.hcp123Fixture.next())).toBe(false);
  await expect(
    page.getByRole('button', { name: 'Undo block', exact: true })
  ).toHaveCount(0);
});
test('HCP123 actual native abort gives no Saved or enforced block', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  expect(await page.evaluate(() => window.hcp123Fixture.abortWrite())).toEqual({
    status: 'aborted'
  });
  expect(await page.evaluate(() => window.hcp123Fixture.rows())).toEqual([]);
  await expect(page.locator('[data-message-id]')).toHaveCount(1);
});
test('HCP123 failed readback after completed native write is unknown until explicit reconciliation', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  expect(
    await page.evaluate(() => window.hcp123Fixture.failReadback())
  ).toEqual({ status: 'unknown_completion' });
  expect(await page.evaluate(() => window.hcp123Fixture.rows())).toHaveLength(
    1
  );
  await page.evaluate(() => window.hcp123Fixture.refresh());
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
});
test('HCP123 logout retires current private inventory without deleting preference or encrypted evidence', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  await page.evaluate(() => window.hcp123Fixture.block());
  const settled = await page.evaluate(async () => ({
    rows: await window.hcp123Fixture.rows(),
    native: await window.hcp123Fixture.native()
  }));
  await page.evaluate(() => window.hcp123Fixture.logout());
  expect(
    await page.evaluate(() => window.hcp123Fixture.preferences())
  ).toMatchObject({ state: 'unavailable' });
  expect(await page.evaluate(() => window.hcp123Fixture.rows())).toEqual(
    settled.rows
  );
  expect(await page.evaluate(() => window.hcp123Fixture.native())).toEqual(
    settled.native
  );
  await expect(
    page.getByRole('button', { name: 'Undo block', exact: true })
  ).toHaveCount(0);
});
test('HCP123 live block fences an already pending authentic decrypt and subsequent automatic continuation', async ({
  page
}) => {
  await start(page, 'backfill');
  await unlocked(page);
  await page.evaluate(() => {
    window.hcp123Fixture.mode('hold_decrypt');
    window.hcp123Pending = window.hcp123Fixture.next();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp123Fixture.pending()))
    .toBe(true);
  const before = await page.evaluate(() => window.hcp123Fixture.native());
  expect(await page.evaluate(() => window.hcp123Fixture.block())).toEqual({
    status: 'saved'
  });
  await page.evaluate(() => {
    window.hcp123Fixture.mode('normal');
    window.hcp123Fixture.settle();
  });
  await page.evaluate(() => window.hcp123Pending);
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp123Fixture.native())).toEqual(
    before
  );
  expect(events).toEqual([]);
  expect(queries).toEqual([]);
});
for (const width of [320, 1024])
  test(`HCP123 ${width}px consequences and Undo remain operable`, async ({
    page
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await start(page);
    await unlocked(page);
    await page
      .getByText('Identity and conversation options', { exact: true })
      .click();
    await page
      .getByRole('button', { name: 'Block person', exact: true })
      .click();
    await page
      .getByRole('button', { name: 'Confirm local block', exact: true })
      .click();
    await expect(
      page.getByRole('button', { name: 'Undo block', exact: true })
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > window.innerWidth
      )
    ).toBe(false);
  });

test('HCP123 a second genuine same-owner generation cannot import the original room custody', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  expect(
    await page.evaluate(() => window.hcp123Fixture.foreignGeneration())
  ).toEqual({ status: 'invalid' });
  expect(await page.evaluate(() => window.hcp123Fixture.rows())).toEqual([]);
});
test('HCP123 disposing one genuine preference view preserves its sibling and current Inbox owner', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  expect(
    await page.evaluate(() => window.hcp123Fixture.disposeSibling())
  ).toMatchObject({ state: 'ready', revision: 0, peers: [] });
  await expect(page.locator('[data-message-id]')).toHaveCount(1);
  expect(await page.evaluate(() => window.hcp123Fixture.block())).toEqual({
    status: 'saved'
  });
});
test('HCP123 block acknowledged during native inventory loading starts no subsequent unwrap', async ({
  page
}) => {
  await start(page, 'backfill');
  await unlocked(page);
  const before = await page.evaluate(() => window.hcp123Fixture.delta());
  await page.evaluate(() => {
    window.hcp123Fixture.holdInventory();
    window.hcp123Pending = window.hcp123Fixture.next();
  });
  await expect
    .poll(() => page.evaluate(() => window.hcp123Fixture.inventoryHeld()))
    .toBe(true);
  expect(await page.evaluate(() => window.hcp123Fixture.block())).toEqual({
    status: 'saved'
  });
  await page.evaluate(() => window.hcp123Fixture.releaseInventory());
  expect(await page.evaluate(() => window.hcp123Pending)).toBe(false);
  expect(await page.evaluate(() => window.hcp123Fixture.delta())).toEqual(
    before
  );
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
});
test('HCP123 persisted authenticated unknown peer block still charges twenty opaque envelopes and Undo starts no work', async ({
  page
}) => {
  await loadFixtureScripts(page);
  const result = await page.evaluate(async () => {
    const f = await window.hcp123.createQueueFixture(21, true);
    window.hcp123Queue = f;
    const saved = await f.persistInnerBlock();
    const before = f.delta();
    const captured = f.capture();
    const batch = await f.batch();
    const after = f.delta();
    const snapshot = f.snapshot();
    const unchanged = await f.encryptedUnchanged();
    const publicContains = await f.publicContains();
    const rooms = f.visibleRooms();
    const undo = await f.undoInnerBlock();
    const afterUndo = f.delta();
    const next = await f.batch();
    const afterNext = f.delta();
    return {
      saved,
      captured,
      batch,
      before,
      after,
      snapshot,
      unchanged,
      publicContains,
      rooms,
      undo,
      afterUndo,
      next,
      afterNext
    };
  });
  expect(result.saved).toEqual({ status: 'saved' });
  expect(result.captured).toBe(true);
  expect(result.batch).toBe(true);
  expect(result.snapshot).toMatchObject({
    attempted: 20,
    blocked: 20,
    admitted: 0,
    queued: 1,
    state: 'needs_action'
  });
  expect(result.after.decrypts - result.before.decrypts).toBe(40);
  expect(result.after.signs - result.before.signs).toBe(0);
  expect(result.after.encrypts - result.before.encrypts).toBe(0);
  expect(result.rooms).toEqual([]);
  expect(result.unchanged).toBe(true);
  expect(result.publicContains).toBe(false);
  expect(result.undo).toEqual({ status: 'saved' });
  expect(result.afterUndo).toEqual(result.after);
  expect(result.next).toBe(true);
  expect(result.afterNext.decrypts - result.afterUndo.decrypts).toBe(40);
  expect(events).toEqual([]);
  expect(queries).toEqual([]);
});

test('HCP123 actual owner lock contention acknowledges no block or native write', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  const before = await page.evaluate(() => window.hcp123Fixture.native());
  await page.evaluate(() => window.hcp123Fixture.holdReadLock());
  expect(await page.evaluate(() => window.hcp123Fixture.block())).toEqual({
    status: 'busy'
  });
  expect(
    await page.evaluate(() => window.hcp123Fixture.preferences())
  ).toMatchObject({ state: 'ready', revision: 0, peers: [] });
  expect(await page.evaluate(() => window.hcp123Fixture.rows())).toEqual([]);
  expect(await page.evaluate(() => window.hcp123Fixture.native())).toEqual(
    before
  );
  await expect(page.locator('[data-message-id]')).toHaveCount(1);
  await page.evaluate(() => window.hcp123Fixture.releaseRead());
});
test('HCP123 existing acknowledged native peer row reconciles the whole revision without another write', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  const result = await page.evaluate(() =>
    window.hcp123Fixture.matchingNativeRow()
  );
  expect(result.result).toEqual({ status: 'existing' });
  expect(result.after).toEqual(result.before);
  expect(result.writes).toBe(0);
  expect(result.snapshot).toMatchObject({ state: 'ready', revision: 17 });
  if (result.snapshot.state !== 'ready')
    throw Error('no current native preferences');
  expect(result.snapshot.peers).toEqual(
    [
      await page.evaluate(() => window.hcp123Fixture.peer),
      result.unrelatedPeer
    ].sort()
  );
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
  expect(await page.evaluate(() => window.hcp123Fixture.list())).toMatchObject({
    requests: [],
    conversations: []
  });
});
test('HCP123 genuine competing native transaction fails full-wire CAS and preserves unrelated peer facts', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  const result = await page.evaluate(() =>
    window.hcp123Fixture.competingNativeRow()
  );
  expect(result.result).toEqual({ status: 'conflict' });
  expect(result.rows).toHaveLength(1);
  const wire = JSON.parse((result.rows[0] as { wire: string }).wire) as {
    revision: number;
    peers: string[];
  };
  expect(wire).toMatchObject({ revision: 19, peers: [result.unrelatedPeer] });
  expect(result.snapshot).toMatchObject({
    state: 'unavailable',
    reason: 'conflict'
  });
  await expect(page.locator('[data-message-id]')).toHaveCount(0);
  const counts = await page.evaluate(() => window.hcp123Fixture.delta());
  expect(await page.evaluate(() => window.hcp123Fixture.next())).toBe(false);
  expect(await page.evaluate(() => window.hcp123Fixture.delta())).toEqual(
    counts
  );
  await page.evaluate(() => window.hcp123Fixture.refresh());
  expect(
    await page.evaluate(() => window.hcp123Fixture.preferences())
  ).toMatchObject({
    state: 'ready',
    revision: 19,
    peers: [result.unrelatedPeer]
  });
});
test('HCP123 acknowledged block reloads for the original account and stays isolated from another observed owner', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  expect(await page.evaluate(() => window.hcp123Fixture.block())).toEqual({
    status: 'saved'
  });
  const native = await page.evaluate(() => window.hcp123Fixture.native());
  const rows = await page.evaluate(() => window.hcp123Fixture.rows());
  const result = await page.evaluate(() => window.hcp123Fixture.reloadOwners());
  expect(result.snapshots).toHaveLength(3);
  const [same, other, returned] = result.snapshots;
  if (!same || !other || !returned)
    throw Error('actual owner reload sequence incomplete');
  expect(same.snapshot).toMatchObject({
    state: 'ready',
    owner: await page.evaluate(() => window.hcp123Fixture.owner),
    revision: 1,
    peers: [await page.evaluate(() => window.hcp123Fixture.peer)]
  });
  expect(other.snapshot).toMatchObject({
    state: 'ready',
    owner: await page.evaluate(() => window.hcp123Fixture.peer),
    revision: 0,
    peers: []
  });
  expect(returned.snapshot).toEqual(same.snapshot);
  for (const row of result.snapshots) expect(row.after).toEqual(row.before);
  expect(result.old).toMatchObject({ state: 'unavailable', reason: 'stopped' });
  expect(result.rows).toEqual(rows);
  expect(await page.evaluate(() => window.hcp123Fixture.native())).toEqual(
    native
  );
});
test('HCP123 block retires captured genuine reply citation and read actions before late citation completion', async ({
  page
}) => {
  await start(page, 'contexts');
  await unlocked(page);
  holdRequests = true;
  const before = await page.evaluate(() =>
    window.hcp123Fixture.captureActions()
  );
  expect(before.current).toBe(true);
  expect(before.citation).toBeDefined();
  expect(before.text).toBe('HCP123_OLD_REPLY_MEMORY_ONLY');
  await expect.poll(() => queries.length).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.hcp123Fixture.block())).toEqual({
    status: 'saved'
  });
  const native = await page.evaluate(() => window.hcp123Fixture.native());
  const counts = await page.evaluate(() => window.hcp123Fixture.delta());
  const old = await page.evaluate(() => window.hcp123Fixture.capturedActions());
  expect(old).toMatchObject({
    current: false,
    send: { status: 'stopped' },
    displayed: false
  });
  expect(old.citation).toBeUndefined();
  expect(old.message).toBeUndefined();
  expect(old.reply).toBeUndefined();
  expect(old.text).toBeUndefined();
  const queryCount = queries.length;
  holdRequests = false;
  for (const release of heldReplies.splice(0)) release();
  expect(
    await page.evaluate(() => window.hcp123Fixture.capturedActions())
  ).toEqual(old);
  expect(await page.evaluate(() => window.hcp123Fixture.native())).toEqual(
    native
  );
  expect(await page.evaluate(() => window.hcp123Fixture.delta())).toEqual(
    counts
  );
  expect(queries).toHaveLength(queryCount);
  expect(events).toEqual([]);
  await expect(
    page.getByText('HCP123_OLD_REPLY_MEMORY_ONLY', { exact: true })
  ).toHaveCount(0);
});
test('HCP123 lost completion channel after genuine native block fails closed until explicit reconciliation', async ({
  page
}) => {
  await start(page);
  await unlocked(page);
  expect(
    await page.evaluate(() => window.hcp123Fixture.failLockCompletion())
  ).toEqual({ status: 'unknown_completion' });
  const rows = await page.evaluate(() => window.hcp123Fixture.rows());
  expect(rows).toHaveLength(1);
  expect(JSON.parse((rows[0] as { wire: string }).wire)).toMatchObject({
    revision: 1,
    peers: [await page.evaluate(() => window.hcp123Fixture.peer)]
  });
  expect(
    await page.evaluate(() => window.hcp123Fixture.preferences())
  ).toMatchObject({ state: 'unavailable', reason: 'unknown_completion' });
  await page.evaluate(() => window.hcp123Fixture.refresh());
  expect(
    await page.evaluate(() => window.hcp123Fixture.preferences())
  ).toMatchObject({
    state: 'ready',
    revision: 1,
    peers: [await page.evaluate(() => window.hcp123Fixture.peer)]
  });
  expect(await page.evaluate(() => window.hcp123Fixture.rows())).toEqual(rows);
});
