import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/private-receipts.ts';
declare global {
  interface Window {
    hcp086: typeof Fixture;
    hcp086Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let frames: unknown[][] = [],
  response = true;
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
    throw Error('loopback unavailable');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket) =>
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
          ? bytes
          : Array.isArray(bytes)
            ? Buffer.concat(bytes)
            : Buffer.from(bytes),
        frame = JSON.parse(raw.toString()) as unknown[];
      frames.push(frame);
      if (frame[0] === 'EVENT')
        socket.send(
          JSON.stringify([
            'OK',
            (frame[1] as { id: string }).id,
            response,
            response ? '' : 'blocked: fixture refusal'
          ])
        );
    })
  );
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/private-receipts.ts', import.meta.url)
        ),
        name: 'hcp086',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('bundle unavailable');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
async function load(page: import('@playwright/test').Page) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
}
test.beforeEach(async ({ page }) => {
  frames = [];
  response = true;
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const origin = new URL(String(url)).origin;
          if (
            !['wss://peer.example.org', 'wss://archive.example.org'].includes(
              origin
            )
          )
            throw Error('unapproved fixture destination');
          super(endpoint + '/?origin=' + encodeURIComponent(origin), protocols);
        }
      };
    },
    { endpoint }
  );
  await load(page);
  await page.evaluate(async () => {
    window.hcp086Fixture = await window.hcp086.makeFixture();
    await window.hcp086Fixture.prepare();
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp086Fixture?.close());
});
test('actual archive ACK persists without claiming recipient receipt', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcp086Fixture,
      attempt = await f.deliver('self_archive'),
      saved = await f.observe('self_archive', attempt.status);
    return { attempt, saved, state: await f.state() };
  });
  expect(result.attempt.status).toBe('accepted');
  expect(result.saved.status).toBe('saved');
  expect(result.state.status?.recipient.state).toBe('unknown');
  expect(result.state.status?.archive.state).toBe('accepted_by_inbox_relay');
  expect(frames.filter((f) => f[0] === 'EVENT')).toHaveLength(1);
});
test('canonical receipt successor retains genuine custody for the remaining exact archive artifact', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp086Fixture,
      original = await f.storedPair(),
      peer = await f.deliver('peer'),
      saved = await f.observe('peer', peer.status),
      verified = await f.verified(),
      archive = await f.deliver('self_archive'),
      savedArchive = await f.observe('self_archive', archive.status);
    return {
      original,
      peer,
      saved,
      verified,
      archive,
      savedArchive,
      state: await f.state()
    };
  });
  expect(r.saved.status).toBe('saved');
  expect(r.verified).toBe(true);
  expect(r.archive.status).toBe('accepted');
  expect(r.savedArchive.status).toBe('saved');
  expect(r.state.status?.partial).toBe(false);
  expect(
    r.state.row?.family === 'private_send_operation' && r.state.row.self
  ).toEqual(r.original!.self);
  expect(
    frames.filter((f) => f[0] === 'EVENT').map((f) => JSON.stringify(f[1]))
  ).toEqual([r.original!.peerArtifact!.wire, r.original!.self.wire]);
});
test('late duplicate actual ACK is idempotent without changing the retained first observation', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp086Fixture,
      a = await f.deliver('peer');
    await f.observe('peer', a.status, 100);
    const before = await f.state(),
      again = await f.observe('peer', a.status, 200);
    return { before, again, after: await f.state() };
  });
  expect(r.again.status).toBe('existing');
  expect(r.after.wire).toBe(r.before.wire);
  expect(frames.filter((f) => f[0] === 'EVENT')).toHaveLength(1);
});
test('actual named refusal stays distinct from recipient success and archive pending', async ({
  page
}) => {
  response = false;
  const r = await page.evaluate(async () => {
    const f = window.hcp086Fixture,
      a = await f.deliver('peer');
    return {
      attempt: a,
      saved: await f.observe('peer', a.status),
      state: await f.state()
    };
  });
  expect(r.attempt.status).toBe('refused');
  expect(r.saved.status).toBe('saved');
  expect(r.state.status?.recipient.state).toBe('unknown');
  expect(r.state.status?.archive.state).toBe('pending');
});
test('exact observed ciphertext readback is retained separately from acceptance', async ({
  page
}) => {
  await page.evaluate(async () => {
    await window.hcp086Fixture.deliver('peer');
  });
  const event = frames.find((f) => f[0] === 'EVENT')?.[1];
  expect(event).toBeDefined();
  const r = await page.evaluate(async (wire) => {
    const f = window.hcp086Fixture;
    return {
      saved: await f.observe('peer', 'readback', 100, wire),
      state: await f.state()
    };
  }, JSON.stringify(event));
  expect(r.saved.status).toBe('saved');
  expect(r.state.status?.recipient.readbackTargets).toEqual([
    'wss://peer.example.org'
  ]);
  expect(r.state.status?.recipient.acceptedTargets).toEqual([]);
});
test('real full-wire CAS races preserve the winning role and reject stale receipt append', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp086Fixture;
    return { results: await f.race(), state: await f.state() };
  });
  expect(r.results.map((x) => x.status).sort()).toEqual(['conflict', 'saved']);
  expect(
    r.state.row?.family === 'private_send_operation' && r.state.row.receipts
  ).toHaveLength(1);
  expect(frames).toEqual([]);
});
test('concurrent genuine receipt renewal keeps one exact winner and admits only its remaining stored artifact', async ({
  page
}) => {
  const r = await page.evaluate(() => window.hcp086Fixture.genuineRace());
  expect(r.results.map((x) => x.status).sort()).toEqual(['conflict', 'saved']);
  expect(r.verified).toBe(true);
  expect(
    r.observed.row?.family === 'private_send_operation' &&
      r.observed.row.receipts
  ).toHaveLength(1);
  expect(r.remaining.status).toBe('accepted');
  const events = frames.filter((f) => f[0] === 'EVENT');
  expect(events).toHaveLength(1);
  const row = r.observed.row;
  if (!row || row.family !== 'private_send_operation')
    throw Error('missing winning row');
  const winner = row.receipts![0].role;
  expect(JSON.stringify(events[0][1])).toBe(
    winner === 'peer' ? row.self.wire : row.peerArtifact!.wire
  );
});
test('native put request followed by actual abort never acknowledges a receipt', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp086Fixture,
      before = await f.state();
    f.failAfterPut('abort');
    const saved = await f.observe('peer', 'accepted');
    const faults = f.faults();
    f.clearFault();
    return { before, saved, faults, after: await f.state() };
  });
  expect(r.faults).toBe(1);
  expect(r.saved.status).toBe('aborted');
  expect(r.after.wire).toBe(r.before.wire);
});
test('failed actual post-put readback preserves uncertainty and cannot renew effect custody', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp086Fixture;
    f.failAfterPut('readback');
    const saved = await f.observe('peer', 'accepted'),
      faults = f.faults();
    f.clearFault();
    return {
      saved,
      faults,
      verified: await f.verified(),
      state: await f.state(),
      attempt: await f.deliver('self_archive')
    };
  });
  expect(r.faults).toBe(1);
  expect(r.saved.status).toBe('unknown_completion');
  expect(r.verified).toBe(false);
  expect(
    r.state.row?.family === 'private_send_operation' && r.state.row.receipts
  ).toHaveLength(1);
  expect(r.attempt.status).toBe('stopped');
  expect(frames).toEqual([]);
});
test('reload retains named receipt uncertainty as local evidence without sender presence', async ({
  page
}) => {
  const identity = await page.evaluate(async () => {
    const f = window.hcp086Fixture;
    await f.observe('peer', 'unknown');
    return { owner: f.owner, id: f.id };
  });
  await load(page);
  const state = await page.evaluate(
    ({ owner, id }) => window.hcp086.readExisting(owner, id),
    identity
  );
  expect(state.status?.recipient.state).toBe('unknown');
  expect(
    state.row?.family === 'private_send_operation' &&
      state.row.receipts?.[0].status
  ).toBe('unknown');
  expect(frames).toEqual([]);
});
test('forged pair acknowledgment cannot append a fact or mint a successor', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp086Fixture,
      before = await f.state();
    return { before, saved: await f.forged(), after: await f.state() };
  });
  expect(r.saved.status).toBe('invalid');
  expect(r.after.wire).toBe(r.before.wire);
  expect(frames).toEqual([]);
});
test('private receipt storage never contains the composer marker or plaintext fields', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp086Fixture;
    await f.observe('self_archive', 'unknown');
    return { marker: f.marker, state: await f.state() };
  });
  expect(r.state.wire).not.toContain(r.marker);
  expect(Object.keys(r.state.row ?? {})).not.toContain('body');
  expect(JSON.stringify(r.state.status)).not.toMatch(
    /Delivered|Seen|Order accepted|Reserved/
  );
  expect(frames).toEqual([]);
});
test('unrecognized record revision remains fenced after a genuine receipt successor', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp086Fixture;
    await f.observe('peer', 'accepted');
    await f.changePairRevision();
    return {
      verified: await f.verified(),
      attempt: await f.deliver('self_archive')
    };
  });
  expect(r.verified).toBe(false);
  expect(r.attempt.status).toBe('stopped');
  expect(frames).toEqual([]);
});
