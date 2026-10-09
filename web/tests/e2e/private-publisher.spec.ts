import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/private-publisher.ts';
declare global {
  interface Window {
    hcp085: typeof Fixture;
    hcp085Fixture: Awaited<ReturnType<typeof Fixture.makeFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let frames: { origin: string; frame: unknown[] }[] = [],
  response: 'accepted' | 'refused' = 'accepted';
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
  sockets.on('connection', (socket, request) => {
    const origin = new URL(request.url!, 'http://127.0.0.1').searchParams.get(
      'origin'
    )!;
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
      frames.push({ origin, frame });
      if (frame[0] === 'EVENT') {
        const event = frame[1] as { id: string };
        socket.send(
          JSON.stringify([
            'OK',
            event.id,
            response === 'accepted',
            response === 'accepted' ? '' : 'blocked: fixture refusal'
          ])
        );
      } else if (frame[0] === 'REQ') {
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
          new URL('./harness/private-publisher.ts', import.meta.url)
        ),
        name: 'hcp085',
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
  for (const s of sockets.clients) s.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  frames = [];
  response = 'accepted';
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const parsed = new URL(String(url));
          if (
            !['wss://peer.example.org', 'wss://archive.example.org'].includes(
              parsed.origin
            )
          )
            throw Error('unapproved fixture destination');
          super(
            endpoint + '/?origin=' + encodeURIComponent(parsed.origin),
            protocols
          );
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
    await page.evaluate(() => window.hcp085Fixture?.close());
});
async function fixture(
  page: import('@playwright/test').Page,
  writeOnly = false
) {
  await page.evaluate(async (writeOnly) => {
    window.hcp085Fixture = await window.hcp085.makeFixture(writeOnly);
  }, writeOnly);
}
test('missing and self-only persisted artifacts never acquire a private delivery capability', async ({
  page
}) => {
  await fixture(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp085Fixture;
    const missing = !!f.permission('peer');
    const self = await f.prepareSelfOnly();
    return {
      missing,
      self: self.status,
      selfOnly: !!f.permission('peer'),
      delivery: await f.deliver('peer')
    };
  });
  expect(result).toEqual({
    missing: false,
    self: 'saved',
    selfOnly: false,
    delivery: { status: 'invalid' }
  });
  expect(frames).toEqual([]);
});
test('actual SDK loopback sees only exact persisted independent peer and archive1059 bytes', async ({
  page
}) => {
  await fixture(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp085Fixture;
    const row = await f.prepare();
    const before = f.counts();
    const peer = await f.deliver('peer'),
      archive = await f.deliver('self_archive');
    const after = f.counts();
    return {
      row,
      peer,
      archive,
      before: {
        encrypts: before.encrypts,
        decrypts: before.decrypts,
        signs: before.signs
      },
      after: {
        encrypts: after.encrypts,
        decrypts: after.decrypts,
        signs: after.signs
      },
      marker: f.marker
    };
  });
  expect(result.peer.status).toBe('accepted');
  expect(result.archive.status).toBe('accepted');
  expect(result.after).toEqual(result.before);
  const events = frames.filter((x) => x.frame[0] === 'EVENT');
  expect(events).toHaveLength(2);
  expect(events.map((x) => x.origin)).toEqual([
    'wss://peer.example.org',
    'wss://archive.example.org'
  ]);
  const stored = [result.row!.peerArtifact!, result.row!.self];
  for (let i = 0; i < events.length; i++) {
    expect(JSON.stringify(events[i].frame[1])).toBe(stored[i].wire);
    expect((events[i].frame[1] as { kind: number }).kind).toBe(1059);
  }
  expect(JSON.stringify(frames)).not.toContain(result.marker);
  expect(stored[0].eventId).not.toBe(stored[1].eventId);
});
test('raw14 and13 and even a genuine signed1059 object cannot replace opaque persisted permission', async ({
  page
}) => {
  await fixture(page);
  const statuses = await page.evaluate(async () => {
    const f = window.hcp085Fixture,
      row = await f.prepare();
    if (!row?.peerArtifact) throw Error('missing actual peer ciphertext');
    const event = JSON.parse(row.peerArtifact.wire) as Record<string, unknown>;
    const results = [];
    for (const kind of [14, 13, 1059])
      results.push(
        (
          await window.hcp085.publishPrivateGiftWrapAttempt(
            f.pool,
            { ...event, kind } as unknown as Parameters<
              typeof window.hcp085.publishPrivateGiftWrapAttempt
            >[1],
            new AbortController().signal
          )
        ).status
      );
    return results;
  });
  expect(statuses).toEqual(['stopped', 'stopped', 'stopped']);
  expect(frames).toEqual([]);
});
test('role mismatches and unreviewed targets reject without a network effect', async ({
  page
}) => {
  await fixture(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp085Fixture;
    await f.prepare();
    return [
      !!f.permission('peer', f.archiveOrigin),
      !!f.permission('self_archive', f.peerOrigin),
      !!f.permission('peer', 'wss://other.example.org'),
      !!f.permission('peer', f.peerOrigin, 'not_reviewed')
    ];
  });
  expect(result).toEqual([false, false, false, false]);
  expect(frames).toEqual([]);
});
for (const mode of ['remove', 'revision', 'readback'] as const)
  test(
    'actual ' +
      mode +
      ' after capture prevents EVENT from stale persisted evidence',
    async ({ page }) => {
      await fixture(page);
      const result = await page.evaluate(async (mode) => {
        const f = window.hcp085Fixture;
        await f.prepare();
        const permission = f.permission('peer');
        if (!permission) throw Error('permission missing');
        if (mode === 'remove') await f.remove();
        else if (mode === 'revision') await f.changePairRevision();
        else f.failReadback();
        return f.publish(permission);
      }, mode);
      expect(result.status).toBe('stopped');
      expect(frames).toEqual([]);
    }
  );
for (const mode of ['Stop', 'Disconnect', 'changed key'] as const)
  test(
    mode + ' before effect fences the exact candidate and opens no socket',
    async ({ page }) => {
      await fixture(page);
      const result = await page.evaluate(async (mode) => {
        const f = window.hcp085Fixture;
        await f.prepare();
        const permission = f.permission('peer');
        if (!permission) throw Error('permission missing');
        if (mode === 'Stop') f.stop();
        else if (mode === 'Disconnect') f.disconnect();
        else f.mode('changed_key');
        return f.publish(permission);
      }, mode);
      expect(result.status).toBe('stopped');
      expect(frames).toEqual([]);
    }
  );
test('one opaque permission cannot issue a second SDK EVENT and detached views cannot retarget it', async ({
  page
}) => {
  await fixture(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp085Fixture;
    await f.prepare();
    const p = f.permission('peer');
    if (!p) throw Error('permission missing');
    const view = window.hcp085.privatePublicationSnapshot(p)!;
    const first = await f.publish(p);
    const second = await f.publish(p);
    return { first, second, view };
  });
  expect(result.first.status).toBe('accepted');
  expect(result.second.status).toBe('stopped');
  expect(result.view.role).toBe('peer');
  expect(frames.filter((x) => x.frame[0] === 'EVENT')).toHaveLength(1);
});
test('actual relay refusal is a named attempt outcome without recipient or order success', async ({
  page
}) => {
  response = 'refused';
  await fixture(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp085Fixture;
    await f.prepare();
    return f.deliver('self_archive');
  });
  expect(result.status).toBe('refused');
  expect(frames.filter((x) => x.frame[0] === 'EVENT')).toHaveLength(1);
});
test('a write-only destination can publish but private reads stay on read-admitted origins', async ({
  page
}) => {
  await fixture(page, true);
  const result = await page.evaluate(async () => {
    const f = window.hcp085Fixture;
    await f.prepare();
    const write = await f.deliver('peer');
    await f.readOrigins();
    return write;
  });
  expect(result.status).toBe('accepted');
  expect(
    frames.filter((x) => x.frame[0] === 'REQ').map((x) => x.origin)
  ).toEqual(['wss://archive.example.org']);
});
for (const newer of [false, true])
  test(
    'successful same-owner actual delivery preserves ' +
      (newer ? 'newer unsaved text' : 'captured saved encrypted text') +
      ' and its composer state',
    async ({ page }) => {
      await fixture(page);
      const result = await page.evaluate(async (newer) => {
        const f = window.hcp085Fixture;
        await f.prepare();
        const before = f.bindComposer(newer);
        const delivery = await f.deliver('peer');
        return { before, delivery, after: f.composerState() };
      }, newer);
      expect(result.delivery.status).toBe('accepted');
      expect(result.after).toEqual(result.before);
      expect(result.before.snapshot?.dirty).toBe(newer);
      expect(result.before.snapshot?.state).toBe(
        newer ? 'unsaved' : 'saved_encrypted'
      );
      expect(frames.filter((x) => x.frame[0] === 'EVENT')).toHaveLength(1);
      expect(JSON.stringify(frames)).not.toContain(
        'HCP085_NEWER_PRIVATE_TEXT_MEMORY_ONLY'
      );
    }
  );
test('actual owner loss still clears the bound private composer and prevents publication', async ({
  page
}) => {
  await fixture(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp085Fixture;
    await f.prepare();
    const before = f.bindComposer(true);
    const permission = f.permission('peer');
    if (!permission) throw Error('missing actual permission');
    f.disconnect();
    return {
      before,
      after: f.composerState(),
      attempt: await f.publish(permission)
    };
  });
  expect(result.before.snapshot?.dirty).toBe(true);
  expect(result.after).toEqual({ text: undefined, snapshot: undefined });
  expect(result.attempt.status).toBe('stopped');
  expect(frames).toEqual([]);
});
