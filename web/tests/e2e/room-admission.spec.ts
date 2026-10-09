import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/room-admission.ts';
declare global {
  interface Window {
    hcp092: typeof Fixture;
    hcp092Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/room-admission.ts', import.meta.url)
        ),
        name: 'hcp092',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('bundle missing');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  await server.close();
});
test.beforeEach(async ({ page }) => {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp092Fixture?.close());
});
test('actual SDK-authenticated inbound selects sender and rejects forged or copied room custody', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = (window.hcp092Fixture = await window.hcp092.makeFixture());
    const unlocked = await f.unlock();
    return {
      unlocked,
      room: f.admit('inbound'),
      peer: f.peer,
      owner: f.owner,
      forged: f.forged(),
      copied: f.copied(),
      id: f.rumorId
    };
  });
  expect(r.unlocked).toBe('authenticated');
  expect(r.room).toMatchObject({
    owner: r.owner,
    peer: r.peer,
    sender: r.peer,
    recipient: r.owner,
    role: 'inbound',
    rumorId: r.id
  });
  expect(r.forged).toBeUndefined();
  expect(r.copied).toBeUndefined();
});
test('actual authenticated self archive preserves original peer recipient and rumor id', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = (window.hcp092Fixture =
      await window.hcp092.makeFixture('self_archive'));
    return {
      unlocked: await f.unlock(),
      room: f.admit('self_archive'),
      owner: f.owner,
      peer: f.peer,
      id: f.rumorId
    };
  });
  expect(r.unlocked).toBe('authenticated');
  expect(r.room).toMatchObject({
    owner: r.owner,
    sender: r.owner,
    recipient: r.peer,
    peer: r.peer,
    role: 'self_archive',
    rumorId: r.id
  });
});
test('actual signed decrypted third-party rumor cannot admit connected owner room', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = (window.hcp092Fixture =
      await window.hcp092.makeFixture('third_party'));
    return { unlocked: await f.unlock(), room: f.admit('inbound') };
  });
  expect(r.unlocked).toBe('authenticated');
  expect(r.room).toBeUndefined();
});
test('explicit expected role mismatch cannot reinterpret a valid inbound as archive', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = (window.hcp092Fixture = await window.hcp092.makeFixture());
    return {
      unlocked: await f.unlock(),
      wrong: f.admit('self_archive'),
      right: f.admit('inbound')
    };
  });
  expect(r.unlocked).toBe('authenticated');
  expect(r.wrong).toBeUndefined();
  expect(r.right?.role).toBe('inbound');
});
test('body URL and subject never retarget actual authenticated peer', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = (window.hcp092Fixture =
      await window.hcp092.makeFixture('hostile_text'));
    return {
      unlocked: await f.unlock(),
      room: f.admit('inbound'),
      peer: f.peer,
      other: f.other
    };
  });
  expect(r.unlocked).toBe('authenticated');
  expect(r.room?.peer).toBe(r.peer);
  expect(r.room?.peer).not.toBe(r.other);
});
test('missing private parent and untrusted hint cause zero public or private socket lookups', async ({
  page
}) => {
  const requests: string[] = [];
  const sockets: string[] = [];
  page.on('request', (request) => requests.push(request.url()));
  page.on('websocket', (socket) => sockets.push(socket.url()));
  const r = await page.evaluate(async () => {
    const f = (window.hcp092Fixture =
      await window.hcp092.makeFixture('missing_parent'));
    return { unlocked: await f.unlock(), room: f.admit('inbound') };
  });
  expect(r.unlocked).toBe('authenticated');
  expect(r.room?.replyTo).toBe('a'.repeat(64));
  expect(requests).toEqual([]);
  expect(sockets).toEqual([]);
});
test('logout removes current genuine room custody and prevents readmission', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = (window.hcp092Fixture = await window.hcp092.makeFixture());
    await f.unlock();
    const before = f.admit('inbound');
    f.disconnect();
    return { before, after: f.snapshot(), again: f.admit('inbound') };
  });
  expect(r.before).toBeDefined();
  expect(r.after).toBeUndefined();
  expect(r.again).toBeUndefined();
});
test('Stop revokes original nested-dependent room proof without cancelling already settled SDK work', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = (window.hcp092Fixture = await window.hcp092.makeFixture());
    await f.unlock();
    const before = f.admit('inbound'),
      counts = f.counts();
    f.stop();
    return {
      before,
      after: f.snapshot(),
      again: f.admit('inbound'),
      counts,
      afterCounts: f.counts()
    };
  });
  expect(r.before).toBeDefined();
  expect(r.after).toBeUndefined();
  expect(r.again).toBeUndefined();
  expect(r.afterCounts).toEqual(r.counts);
});
