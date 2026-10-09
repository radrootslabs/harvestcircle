import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/retry-send.ts';
declare global {
  interface Window {
    hcp087: typeof Fixture;
    hcp087Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
    hcp087Pending: ReturnType<
      Awaited<ReturnType<typeof Fixture.makeFixture>>['run']
    >;
    hcp087Advance: () => void;
    hcp087Release: () => void;
    hcp087Waiting: boolean;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let events: { origin: string; wire: string }[] = [],
  response: 'accepted' | 'refused' | 'unknown' | 'silent' = 'accepted';
let archiveResponse: 'accepted' | 'refused' | 'unknown' | 'silent' | undefined;
let pendingAcks: { socket: import('ws').WebSocket; id: string }[] = [];
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
  sockets.on('connection', (socket, request) =>
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
          ? bytes
          : Array.isArray(bytes)
            ? Buffer.concat(bytes)
            : Buffer.from(bytes),
        frame = JSON.parse(raw.toString()) as unknown[];
      if (frame[0] !== 'EVENT') return;
      const event = frame[1] as { id: string };
      pendingAcks.push({ socket, id: event.id });
      events.push({
        origin: new URL(request.url!, 'http://localhost').searchParams.get(
          'origin'
        )!,
        wire: JSON.stringify(event)
      });
      const outcome =
        events.at(-1)!.origin === 'wss://archive.example.org'
          ? (archiveResponse ?? response)
          : response;
      if (outcome !== 'silent')
        socket.send(
          JSON.stringify([
            'OK',
            event.id,
            outcome === 'accepted',
            outcome === 'unknown'
              ? 'Timeout'
              : outcome === 'refused'
                ? 'auth-required: fixture refusal'
                : ''
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
          new URL('./harness/retry-send.ts', import.meta.url)
        ),
        name: 'hcp087',
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
async function load(page: import('@playwright/test').Page, initialise = true) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async (initialise) => {
    window.hcp087Fixture = await window.hcp087.makeFixture(initialise);
  }, initialise);
}
test.beforeEach(async ({ page }) => {
  events = [];
  response = 'accepted';
  archiveResponse = undefined;
  pendingAcks = [];
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
            throw Error('unapproved destination');
          super(endpoint + '/?origin=' + encodeURIComponent(origin), protocols);
        }
      };
    },
    { endpoint }
  );
  await load(page);
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => window.hcp087Fixture?.close());
});
test('successful same-owner observations preserve recovered rumor and both original artifacts', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp087Fixture,
      prepared = await f.prepare();
    f.capture();
    const before = f.countsValue(),
      result = await f.run();
    return {
      prepared,
      result,
      before,
      after: f.countsValue(),
      recovered: f.recovered(),
      row: await f.row()
    };
  });
  expect(r.prepared.status).toBe('existing');
  expect(r.result.status).toBe('completed');
  expect(r.recovered.rumor).toBe(true);
  expect(r.after.encrypts).toBe(r.before.encrypts);
  expect(r.after.signs).toBe(r.before.signs);
  expect(events.map((e) => e.wire)).toEqual([
    r.row.record.peerArtifact!.wire,
    r.row.record.self.wire
  ]);
});
test('actual reload retries only the remaining original artifact with no re-sign or rewrap', async ({
  page
}) => {
  archiveResponse = 'unknown';
  const original = await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    const result = await f.run();
    if (result.status !== 'needs_action')
      throw Error('missing real archive uncertainty');
    return (await f.row()).record;
  });
  await page.evaluate(() => window.hcp087Fixture.close());
  await page.reload();
  await load(page, false);
  response = 'accepted';
  archiveResponse = undefined;
  events = [];
  const r = await page.evaluate(async () => {
    const f = window.hcp087Fixture,
      baseline = f.countsValue(),
      prepared = await f.prepare();
    f.capture();
    const before = f.countsValue(),
      result = await f.run();
    return {
      prepared,
      result,
      baseline,
      before,
      after: f.countsValue(),
      row: (await f.row()).record
    };
  });
  expect(r.result.status).toBe('completed');
  // The existing fixture's genuine capability probe already exercised crypto.
  // Recovery and publication must add no encryption/signing to that baseline.
  expect(r.before.encrypts).toBe(r.baseline.encrypts);
  expect(r.before.signs).toBe(r.baseline.signs);
  expect(r.before.decrypts - r.baseline.decrypts).toBe(2);
  expect(r.after.encrypts).toBe(r.baseline.encrypts);
  expect(r.after.signs).toBe(r.baseline.signs);
  expect(r.row.self).toEqual(original.self);
  expect(r.row.peerArtifact).toEqual(original.peerArtifact);
  expect(events.map((e) => e.wire)).toEqual([original.self.wire]);
  expect(r.row.receipts).toHaveLength(5);
});
test('a second explicit action performs no duplicate accepted target work', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    const first = await f.run();
    f.capture();
    const second = await f.run();
    return { first, second, row: (await f.row()).record };
  });
  expect(r.first.status).toBe('completed');
  expect(r.second.status).toBe('completed');
  expect(events).toHaveLength(2);
  expect(r.row.receipts).toHaveLength(2);
});
test('three real unknown attempts share one action and never schedule a fourth', async ({
  page
}) => {
  response = 'unknown';
  const r = await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    return { result: await f.run(), row: (await f.row()).record };
  });
  expect(r.result.status).toBe('needs_action');
  expect(events).toHaveLength(3);
  expect(new Set(events.map((e) => e.wire)).size).toBe(1);
  expect(r.row.receipts?.map((r) => r.attempt)).toEqual([1, 2, 3]);
  expect(new Set(r.row.receipts?.map((r) => r.actionId)).size).toBe(1);
});
test('authoritative auth refusal pauses without spinning or touching archive', async ({
  page
}) => {
  response = 'refused';
  const r = await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    return { result: await f.run(), row: (await f.row()).record };
  });
  expect(r.result.status).toBe('needs_action');
  expect(events).toHaveLength(1);
  expect(r.row.receipts?.[0].status).toBe('refused');
});
test('wrong review and reused completed action cannot silently start another effect', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    const wrong = f.capture('unreviewed');
    f.capture();
    const refused = await f.run('unreviewed');
    const first = await f.run();
    const second = await f.run();
    return { wrong, refused, first, second };
  });
  expect(r.wrong).toBe(false);
  expect(r.refused.status).toBe('invalid');
  expect(r.first.status).toBe('completed');
  expect(r.second.status).toBe('invalid');
  expect(events).toHaveLength(2);
});
test('changed stored revision prevents all retry effects', async ({ page }) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    await f.write('revision');
    return f.run();
  });
  expect(r.status).toBe('needs_action');
  expect(events).toEqual([]);
});
test('Stop before execution fences every effect and preserves ciphertext', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    const before = await f.row();
    f.stop();
    return { result: await f.run(), before, after: await f.row() };
  });
  expect(r.result.status).toBe('stopped');
  expect(r.after.wire).toBe(r.before.wire);
  expect(events).toEqual([]);
});
test('actual changed owner retires recovered private body before any EVENT', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    f.mode('changed_key');
    return { result: await f.run(), recovered: f.recovered() };
  });
  expect(r.result.status).toBe('stopped');
  expect(r.recovered.rumor).toBe(false);
  expect(events).toEqual([]);
});
test('missing persisted ciphertext cannot become a successful retry', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    await f.write('erase');
    return f.run();
  });
  expect(r.status).toBe('needs_action');
  expect(events).toEqual([]);
});
test('the shared 45 second network budget ends a real silent attempt without another EVENT', async ({
  page
}) => {
  response = 'silent';
  await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    const native = performance.now.bind(performance);
    let offset = 0;
    Object.defineProperty(performance, 'now', {
      configurable: true,
      value: () => native() + offset
    });
    window.hcp087Advance = () => {
      offset += 45001;
    };
    window.hcp087Pending = f.run();
  });
  await expect.poll(() => events.length).toBe(1);
  await page.evaluate(() => window.hcp087Advance());
  const r = await page.evaluate(() => window.hcp087Pending);
  expect(r.status).toBe('needs_action');
  expect(events).toHaveLength(1);
});
test('time spent awaiting the genuine provider observation is outside network budget', async ({
  page
}) => {
  await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    const native = performance.now.bind(performance);
    let offset = 0;
    Object.defineProperty(performance, 'now', {
      configurable: true,
      value: () => native() + offset
    });
    const provider = window.nostr as { getPublicKey(): Promise<string> };
    const original = provider.getPublicKey.bind(provider);
    let held = true;
    provider.getPublicKey = async () => {
      if (held) {
        held = false;
        window.hcp087Waiting = true;
        await new Promise<void>((resolve) => {
          window.hcp087Release = resolve;
        });
      }
      return original();
    };
    window.hcp087Advance = () => {
      offset += 60000;
    };
    window.hcp087Pending = f.run();
  });
  await expect.poll(() => page.evaluate(() => window.hcp087Waiting)).toBe(true);
  expect(events).toEqual([]);
  await page.evaluate(() => {
    window.hcp087Advance();
    window.hcp087Release();
  });
  const r = await page.evaluate(() => window.hcp087Pending);
  expect(r.status).toBe('completed');
  expect(events).toHaveLength(2);
});
test('a controlled genuine receipt successor accepted during key wait suppresses duplicate target publication', async ({
  page
}) => {
  await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    const provider = window.nostr as { getPublicKey(): Promise<string> },
      original = provider.getPublicKey.bind(provider);
    let calls = 0;
    provider.getPublicKey = async () => {
      calls++;
      if (calls === 2) {
        window.hcp087Waiting = true;
        await new Promise<void>((resolve) => {
          window.hcp087Release = resolve;
        });
      }
      return original();
    };
    window.hcp087Pending = f.run();
  });
  await expect.poll(() => page.evaluate(() => window.hcp087Waiting)).toBe(true);
  expect(events).toHaveLength(1);
  const successor = await page.evaluate(() =>
    window.hcp087Fixture.controlledArchiveSuccessor()
  );
  expect(successor.status).toBe('saved');
  await page.evaluate(() => window.hcp087Release());
  const result = await page.evaluate(() => window.hcp087Pending);
  expect(result.status).toBe('completed');
  expect(events).toHaveLength(1);
});
test('network time consumed by earlier real attempts reduces the remaining action allowance', async ({
  page
}) => {
  response = 'silent';
  await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    const native = performance.now.bind(performance);
    let offset = 0;
    Object.defineProperty(performance, 'now', {
      configurable: true,
      value: () => native() + offset
    });
    window.hcp087Advance = () => {
      offset += 30000;
    };
    window.hcp087Pending = f.run();
  });
  await expect.poll(() => events.length).toBe(1);
  await page.evaluate(() => window.hcp087Advance());
  pendingAcks[0].socket.send(
    JSON.stringify(['OK', pendingAcks[0].id, false, 'Timeout'])
  );
  await expect.poll(() => events.length).toBe(2);
  await page.evaluate(() => window.hcp087Advance());
  pendingAcks[1].socket.send(
    JSON.stringify(['OK', pendingAcks[1].id, false, 'Timeout'])
  );
  const r = await page.evaluate(async () => ({
    result: await window.hcp087Pending,
    row: (await window.hcp087Fixture.row()).record
  }));
  expect(r.result.status).toBe('needs_action');
  expect(events).toHaveLength(2);
  expect(events[1].wire).toBe(events[0].wire);
  expect(r.row.receipts?.map((f) => f.status)).toEqual([
    'unknown',
    'timed_out'
  ]);
});
test('a codec-valid success row inserted between verification and load cannot claim genuine completion', async ({
  page
}) => {
  const r = await page.evaluate(async () => {
    const f = window.hcp087Fixture;
    await f.prepare();
    f.capture();
    const mutation = await f.injectUnacknowledgedSuccessBetweenReads(),
      proofBefore = f.proof(),
      result = await f.run(),
      injected = f.injected();
    f.clearFault();
    return {
      mutation,
      proofBefore,
      proofAfter: f.proof(),
      result,
      injected,
      row: await f.row()
    };
  });
  expect(r.injected).toBe(1);
  expect(r.row.wire).toBe(r.mutation.changedWire);
  expect(r.proofAfter).toEqual(r.proofBefore);
  expect(r.proofAfter?.revision).toBe(r.mutation.revision);
  expect(r.result.status).toBe('needs_action');
  expect(events).toEqual([]);
});
