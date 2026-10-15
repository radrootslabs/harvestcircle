import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-probe-exclusion.ts';
declare global {
  interface Window {
    qx: typeof Fixture;
    qxFixture: Awaited<ReturnType<typeof Fixture.makeExclusionFixture>>;
    qxChild: Awaited<ReturnType<typeof Fixture.openRecoveryContext>>;
    qxHold: ReturnType<typeof Fixture.holdExclusionNativeCompletion>;
    qxPending: Promise<string>;
    qxCancellation: AbortController;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string,
  head = '';
const physical = new Set<WebSocket>(),
  archived = new Map<string, unknown>(),
  physicalOrigins = new WeakMap<WebSocket, string>(),
  live = new Map<WebSocket, string>();
let receiveFrames: { id: string; created_at: number }[] = [];
let receiveLane: 'none' | 'history' | 'live' | 'both' = 'none';
const requests: {
  origin: string;
  filter: { kinds?: number[]; ids?: string[]; limit?: number; until?: number };
}[] = [];
const origins = [
  'wss://inbox.example.org',
  'wss://peer.example.org',
  'wss://archive.example.org'
];
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
    throw Error('actual loopback endpoint');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket, request) => {
    physical.add(socket);
    socket.on('close', () => {
      physical.delete(socket);
      live.delete(socket);
    });
    const origin = new URL(request.url!, 'http://127.0.0.1').searchParams.get(
      'origin'
    )!;
    physicalOrigins.set(socket, origin);
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
      if (frame[0] === 'REQ') {
        const filter = frame[2] as {
          kinds?: number[];
          ids?: string[];
          limit?: number;
          until?: number;
        };
        if (requests.length >= 10000)
          throw Error('bounded fixture request log');
        requests.push({ origin, filter });
        if (filter.kinds?.includes(10050) && head)
          socket.send(JSON.stringify(['EVENT', frame[1], JSON.parse(head)]));
        if (filter.kinds?.includes(1059)) {
          if (filter.ids) {
            const event = archived.get(JSON.stringify([origin, filter.ids[0]]));
            if (event) socket.send(JSON.stringify(['EVENT', frame[1], event]));
          } else {
            if (filter.limit === 0) live.set(socket, String(frame[1]));
            if (
              receiveLane === 'both' ||
              (receiveLane === 'history' && filter.limit !== 0) ||
              (receiveLane === 'live' && filter.limit === 0)
            ) {
              for (const event of receiveFrames)
                if (
                  filter.until === undefined ||
                  event.created_at <= filter.until
                )
                  socket.send(JSON.stringify(['EVENT', frame[1], event]));
            }
          }
        }
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      } else if (frame[0] === 'EVENT') {
        const event = frame[1] as { id: string };
        archived.set(JSON.stringify([origin, event.id]), event);
        socket.send(
          JSON.stringify(['OK', event.id, true, 'controlled named acceptance'])
        );
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
          new URL('./harness/inbox-probe-exclusion.ts', import.meta.url)
        ),
        name: 'qx',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('actual compiled exclusion harness');
  const chunks = output.output.filter((item) => item.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  await expect.poll(() => physical.size).toBe(0);
  head = '';
  archived.clear();
  live.clear();
  requests.length = 0;
  receiveFrames = [];
  receiveLane = 'none';
  await page.addInitScript(
    ({ endpoint, origins }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const origin = new URL(String(url)).origin;
          if (!['wss://discovery.example.org', ...origins].includes(origin))
            throw Error('unapproved fixture origin');
          super(endpoint + '/?origin=' + encodeURIComponent(origin), protocols);
        }
      };
    },
    { endpoint, origins }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(async (origins) => {
    window.qxFixture = await window.qx.makeExclusionFixture();
    return window.qxFixture.host.preferenceWire(origins);
  }, origins);
  await page.evaluate(async () => {
    await window.qxFixture.open();
    if ((await window.qxFixture.prepareOriginal()).status !== 'prepared')
      throw Error('actual original S3/R1 preparation');
  });
});
test.afterEach(async ({ page }) => {
  if (page.isClosed()) return;
  const closed = await page.evaluate(() => {
    try {
      window.qxFixture.close();
      return true;
    } catch (error) {
      if (
        error instanceof Error &&
        [
          'private_inbox_cleanup_required',
          'private_history_cleanup_required',
          'private_auth_cleanup_required'
        ].includes(error.message)
      )
        return false;
      throw error;
    }
  });
  if (!closed) {
    await expect
      .poll(
        () =>
          Array.from(physical).filter(
            (socket) =>
              physicalOrigins.get(socket) !== 'wss://discovery.example.org'
          ).length
      )
      .toBe(0);
    await page.evaluate(() => window.qxFixture.close());
  }
});
test('original actual admitted journal excludes exact self ciphertext before received retention', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.qxFixture,
      before = f.host.counts();
    const status = await f.retain();
    return {
      status,
      before,
      after: f.host.counts(),
      received: (await f.receivedRows()).length
    };
  });
  expect(result.status).toBe('probe_excluded');
  expect(result.received).toBe(0);
  expect(result.after).toEqual(result.before);
});
test('original actual C7 loss during native observation uses ordinary owner-current retention', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.qxFixture;
    const fault = f.nativeFault('read_loss', () => f.stop());
    try {
      const status = await f.retain();
      return {
        status,
        native: fault.counts(),
        received: (await f.receivedRows()).length
      };
    } finally {
      fault.restore();
    }
  });
  expect(result.native.readonly).toBe(1);
  expect(result.status).toBe('retained');
  expect(result.received).toBe(1);
});

for (const recovered of [false, true]) {
  const label = recovered ? 'REC' : 'original';
  test(`${label} actual readonly exclusion performs zero native mutations`, async ({
    page
  }) => {
    const result = await page.evaluate(async (recovered) => {
      const f = window.qxFixture;
      if (recovered && (await f.qualify(true)).status !== 'saved')
        throw Error('actual REC');
      const wire = await f.selfWire(),
        before = f.host.counts(),
        watch = window.qx.watchExclusionNative();
      try {
        const status = await f.retain(wire);
        return {
          status,
          native: watch.counts(),
          before,
          after: f.host.counts()
        };
      } finally {
        watch.restore();
      }
    }, recovered);
    expect(result.status).toBe('probe_excluded');
    expect(result.native.readonly).toBeGreaterThan(0);
    expect(result.native).toMatchObject({ readwrite: 0, puts: 0, deletes: 0 });
    expect(result.after).toEqual(result.before);
  });
  test(`${label} held actual owner WebLock refuses before native or provider work`, async ({
    page
  }) => {
    const result = await page.evaluate(async (recovered) => {
      const f = window.qxFixture;
      if (recovered && (await f.qualify(true)).status !== 'saved')
        throw Error('actual REC');
      const wire = await f.selfWire(),
        before = f.host.counts(),
        watch = window.qx.watchExclusionNative();
      try {
        return await navigator.locks.request(
          'harvestcircle:owner:' + f.host.owner,
          async () => ({
            status: await f.retain(wire),
            native: watch.counts(),
            before,
            after: f.host.counts()
          })
        );
      } finally {
        watch.restore();
      }
    }, recovered);
    expect(result.status).toBe('busy');
    expect(result.native).toEqual({
      readonly: 0,
      readwrite: 0,
      puts: 0,
      deletes: 0
    });
    expect(result.after).toEqual(result.before);
  });
  for (const loss of ['cancel', 'session', 'C7', 'head'] as const)
    test(`${label} actual ${loss} loss while native completion is held cannot exclude`, async ({
      page
    }) => {
      await page.evaluate(async (recovered) => {
        const f = window.qxFixture;
        if (recovered && (await f.qualify(true)).status !== 'saved')
          throw Error('actual REC');
      }, recovered);
      await page.evaluate(async () => {
        const wire = await window.qxFixture.selfWire();
        window.qxCancellation = new AbortController();
        window.qxHold = window.qx.holdExclusionNativeCompletion();
        window.qxPending = window.qxFixture.retain(
          wire,
          window.qxCancellation.signal
        );
      });
      await expect
        .poll(() => page.evaluate(() => window.qxHold.waiting()))
        .toBe(true);
      const result = await page.evaluate(async (loss) => {
        const f = window.qxFixture;
        try {
          if (loss === 'cancel') window.qxCancellation.abort();
          else if (loss === 'session')
            window.qx.closePrivateSession(f.actors().session);
          else if (loss === 'head') f.actors().churnHead();
          else if (loss === 'C7') f.stop();
        } finally {
          window.qxHold.release();
        }
        return {
          status: await window.qxPending,
          received: (await f.receivedRows()).length
        };
      }, loss);
      expect(result.status).toBe(
        loss === 'C7' || loss === 'head' ? 'retained' : 'stopped'
      );
      expect(result.received).toBe(loss === 'C7' || loss === 'head' ? 1 : 0);
    });
  test(`${label} copied signed id with changed ciphertext fails ordinary admission`, async ({
    page
  }) => {
    const result = await page.evaluate(async (recovered) => {
      const f = window.qxFixture;
      if (recovered && (await f.qualify(true)).status !== 'saved')
        throw Error('actual REC');
      const event = JSON.parse(await f.selfWire()) as { content: string };
      event.content =
        event.content.slice(0, -1) + (event.content.endsWith('A') ? 'B' : 'A');
      const before = f.host.counts(),
        status = await f.retain(JSON.stringify(event));
      return {
        status,
        before,
        after: f.host.counts(),
        received: (await f.receivedRows()).length
      };
    }, recovered);
    expect(result.status).toBe('invalid');
    expect(result.received).toBe(0);
    expect(result.after).toEqual(result.before);
  });
  test(`${label} copied envelope and different genuine repository session and owner cannot borrow exclusion`, async ({
    page
  }) => {
    const result = await page.evaluate(async (recovered) => {
      const q = window.qx,
        f = window.qxFixture;
      if (recovered && (await f.qualify(true)).status !== 'saved')
        throw Error('actual REC');
      const wire = await f.selfWire(),
        a = f.actors(),
        opened = await q.openBrowserDatabase();
      if (opened.state !== 'ready') throw Error('actual database');
      const other = q.createPrivateStorageRepository(
        opened.owner,
        f.host.owner
      )!;
      const identity = q.createIdentitySession();
      await q.connectIdentity(identity);
      await q.probeIdentityMessaging(identity, 'reviewed_self_copy');
      const session = await q.createPrivateSession(
        identity,
        'reviewed_private_session'
      );
      if (!session || session === a.session || other === f.repository)
        throw Error('actual distinct source actors');
      try {
        const envelope = q.admitInboxEnvelope(
          wire,
          f.host.owner,
          originsForBrowser(),
          Date.now()
        )!;
        const before = f.host.counts();
        const copied = await q.retainInboxEnvelope(f.repository, a.session, {
          ...envelope
        });
        const foreign = await q.retainInboxEnvelope(
          f.host.foreignRepository(),
          a.session,
          envelope
        );
        const otherRepository = await q.retainInboxEnvelope(
          other,
          a.session,
          envelope
        );
        const otherSession = await q.retainInboxEnvelope(
          f.repository,
          session,
          envelope
        );
        return {
          copied: copied.status,
          foreign: foreign.status,
          otherRepository: otherRepository.status,
          otherSession: otherSession.status,
          before,
          after: f.host.counts()
        };
      } finally {
        q.closePrivateSession(session);
        q.disconnectIdentity(identity);
        q.closeBrowserDatabase(opened.owner);
      }
      function originsForBrowser() {
        return 'wss://inbox.example.org';
      }
    }, recovered);
    expect(result).toMatchObject({
      copied: 'invalid',
      foreign: 'invalid',
      otherRepository: 'retained',
      otherSession: 'duplicate'
    });
    expect(result.after).toEqual(result.before);
  });
  test(`${label} only a new genuine source admission restores exclusion after observed absence`, async ({
    page
  }) => {
    const result = await page.evaluate(async (recovered) => {
      const f = window.qxFixture;
      if (recovered && (await f.qualify(true)).status !== 'saved')
        throw Error('actual REC');
      const wire = await f.selfWire(),
        row = await f.rowWire();
      await f.removeFixtureRow();
      const missing = await f.retain(wire);
      await f.replaceRow(row);
      const restored = await f.retain(wire);
      await f.freshSession();
      const recovery = await f.recover();
      if (!recovery || recovery.status !== 'recovered')
        throw Error('actual fresh recovery');
      const adopted = await f.adopt();
      if (!('acknowledgement' in adopted)) throw Error('actual fresh adoption');
      return {
        missing,
        restored,
        readmitted: await f.retain(wire),
        received: (await f.receivedRows()).length
      };
    }, recovered);
    expect(result).toEqual({
      missing: 'retained',
      restored: 'duplicate',
      readmitted: 'probe_excluded',
      received: 1
    });
  });
  for (const lane of ['finite', 'sync'] as const)
    test(`${label} genuine ${lane} pending queue stays bounded while actual native completion is held`, async ({
      page
    }) => {
      await qualify(page, recovered);
      const wire = await page.evaluate(() => window.qxFixture.selfWire());
      receiveFrames = Array.from(
        { length: 501 },
        () => JSON.parse(wire) as (typeof receiveFrames)[number]
      );
      receiveLane = lane === 'finite' ? 'history' : 'live';
      await page.evaluate(() => {
        window.qxHold = window.qx.holdExclusionNativeCompletion();
      });
      try {
        expect(
          await page.evaluate(
            (lane) => window.qxFixture.startReceive(lane),
            lane
          )
        ).toBe(true);
        await expect
          .poll(() => page.evaluate(() => window.qxHold.waiting()))
          .toBe(true);
        await expect
          .poll(() =>
            page.evaluate(() => window.qxFixture.receiveSnapshot()?.reason)
          )
          .toBe('budget');
        const held = await page.evaluate(() =>
          window.qxFixture.receiveSnapshot()
        );
        expect(held!.pending).toBeGreaterThan(0);
        expect(held!.pending).toBeLessThanOrEqual(500);
        expect(held!.excludedProbes).toBe(0);
      } finally {
        await page.evaluate(() => window.qxHold.release());
      }
      await expect
        .poll(() =>
          page.evaluate(() => window.qxFixture.receiveSnapshot()?.pending)
        )
        .toBe(0);
      expect(
        await page.evaluate(
          async () => (await window.qxFixture.receivedRows()).length
        )
      ).toBe(0);
    });
  test(`${label} repeated full inclusive probe page saturates and actual Stop clears transient cursors`, async ({
    page
  }) => {
    await qualify(page, recovered);
    expect(
      await page.evaluate(() => window.qxFixture.startReceive('sync'))
    ).toBe(true);
    await liveReady(page);
    const wire = await page.evaluate(() => window.qxFixture.selfWire());
    receiveFrames = Array.from(
      { length: 200 },
      () => JSON.parse(wire) as (typeof receiveFrames)[number]
    );
    receiveLane = 'history';
    expect(await page.evaluate(() => window.qxFixture.older())).toBe(true);
    expect(
      (await page.evaluate(() => window.qxFixture.olderSnapshot()))!.sources[0]
        .state
    ).toBe('capped');
    expect(await page.evaluate(() => window.qxFixture.older())).toBe(true);
    expect(
      (await page.evaluate(() => window.qxFixture.olderSnapshot()))!.sources[0]
        .state
    ).toBe('saturated');
    const count = requests.length;
    expect(await page.evaluate(() => window.qxFixture.older())).toBe(true);
    // Shared500 cap may leave later sources partial; the saturated FIRST
    // source itself cannot be requested a third time.
    expect(
      requests
        .slice(count)
        .filter((row) => row.origin === origins[0] && row.filter.limit !== 0)
    ).toHaveLength(0);
    await page.evaluate(() => {
      try {
        window.qxFixture.stopReceive();
      } catch (error) {
        if (
          !(error instanceof Error) ||
          ![
            'private_history_cleanup_required',
            'private_inbox_cleanup_required'
          ].includes(error.message)
        )
          throw error;
      }
    });
    expect(
      (await page.evaluate(() => window.qxFixture.olderSnapshot()))!.sources
    ).toEqual([]);
    expect(await page.evaluate(() => window.qxFixture.older())).toBe(false);
    expect(
      await page.evaluate(
        async () => (await window.qxFixture.receivedRows()).length
      )
    ).toBe(0);
  });
  test(`${label} observed generation loss clears transient excluded cursor metadata`, async ({
    page
  }) => {
    await qualify(page, recovered);
    expect(
      await page.evaluate(() => window.qxFixture.startReceive('sync'))
    ).toBe(true);
    await liveReady(page);
    const wire = await page.evaluate(() => window.qxFixture.selfWire());
    receiveFrames = [JSON.parse(wire) as (typeof receiveFrames)[number]];
    receiveLane = 'history';
    expect(await page.evaluate(() => window.qxFixture.older())).toBe(true);
    expect(
      (await page.evaluate(() => window.qxFixture.olderSnapshot()))!.sources
        .length
    ).toBeGreaterThan(0);
    await page.evaluate(() => window.qxFixture.actors().churnHead());
    expect(await page.evaluate(() => window.qxFixture.older())).toBe(false);
    expect(
      (await page.evaluate(() => window.qxFixture.olderSnapshot()))!.sources
    ).toEqual([]);
  });
  test(`${label} same current direct probe remains excluded on repeated delivery`, async ({
    page
  }) => {
    const result = await page.evaluate(async (recovered) => {
      const f = window.qxFixture;
      if (recovered && (await f.qualify(true)).status !== 'saved')
        throw Error('actual REC source');
      const before = f.host.counts(),
        first = await f.retain(),
        second = await f.retain();
      return {
        first,
        second,
        before,
        after: f.host.counts(),
        received: (await f.receivedRows()).length
      };
    }, recovered);
    expect(result.first).toBe('probe_excluded');
    expect(result.second).toBe('probe_excluded');
    expect(result.received).toBe(0);
    expect(result.after).toEqual(result.before);
  });
  test(`${label} fresh actual session without adoption receives stored probe through ordinary decrypt`, async ({
    page
  }) => {
    const result = await page.evaluate(async (recovered) => {
      const f = window.qxFixture;
      if (recovered && (await f.qualify(true)).status !== 'saved')
        throw Error('actual REC source');
      const wire = await f.selfWire();
      await f.freshSession();
      const before = f.host.counts(),
        status = await f.retain(wire),
        afterRetain = f.host.counts(),
        decrypted = await f.decryptStored();
      return { status, before, afterRetain, decrypted };
    }, recovered);
    expect(result.status).toBe('retained');
    expect(result.afterRetain).toEqual(result.before);
    expect(result.decrypted.snapshot).toMatchObject({
      attempted: 1,
      invalid: 1,
      admitted: 0
    });
    expect(
      result.decrypted.after.decrypts - result.decrypted.before.decrypts
    ).toBe(2);
  });
  test(`${label} actual browser reload loses suppression admission while keeping native journal`, async ({
    page
  }) => {
    const setup = await page.evaluate(() => {
      const f = window.qxFixture;
      return {
        owner: f.host.owner,
        policyWire: JSON.stringify(window.qx.readRelayPolicy(f.host.policy)),
        operatorWire: window.qx.inboxOperatorQualificationWire(
          f.host.qualification,
          f.host.policy
        )!
      };
    });
    await page.evaluate((url) => {
      const child = document.createElement('iframe');
      child.id = 'exclusion-realm';
      child.src = url;
      document.body.append(child);
    }, server.url + '/search');
    const frame = await (
      await page.locator('#exclusion-realm').elementHandle()
    ).contentFrame();
    if (!frame) throw Error('actual same-origin child');
    await frame.waitForLoadState();
    await frame.addScriptTag({ content: bundle });
    const original = await frame.evaluate(
      async ({ setup, recovered }) => {
        window.qx.installHostedProvider();
        const policy = window.qx.validateRelayPolicy(setup.policyWire)!,
          operator = window.qx.validateInboxOperatorQualification(
            setup.operatorWire
          )!;
        const id = crypto.randomUUID();
        const context = await window.qx.openRecoveryContext(
          setup.owner,
          policy,
          operator,
          id
        );
        const record = await window.qx.prepareHostedOriginal(context);
        if (recovered) {
          const result = await context.recover(
            context.capture('reviewed_inbox_probe_recovery', record.id)
          );
          if (!('proof' in result)) throw Error('actual child recovery');
          const adopted = await window.qx.adoptRecoveredInboxProbe(
            context.repository,
            context.session,
            context.access,
            policy,
            operator,
            result.proof,
            'reviewed_original_inbox_probe_retry'
          );
          if (!('acknowledgement' in adopted))
            throw Error('actual child adoption');
        }
        const wire = record.self!.wire;
        const before = await window.qx.retainInRecoveryContext(context, wire);
        context.close();
        return { id: record.id, wire, before };
      },
      { setup, recovered }
    );
    expect(original.before).toBe('probe_excluded');
    await frame.goto(server.url + '/search');
    await frame.addScriptTag({ content: bundle });
    const before = await page.evaluate(() => window.qxFixture.host.counts());
    const result = await frame.evaluate(
      async ({ setup, original }) => {
        window.qx.installHostedProvider();
        window.qxChild = await window.qx.openRecoveryContext(
          setup.owner,
          window.qx.validateRelayPolicy(setup.policyWire)!,
          window.qx.validateInboxOperatorQualification(setup.operatorWire)!,
          original.id
        );
        const status = await window.qx.retainInRecoveryContext(
          window.qxChild,
          original.wire
        );
        return { status };
      },
      { setup, original }
    );
    const afterRetain = await page.evaluate(() =>
      window.qxFixture.host.counts()
    );
    const decrypted = await frame.evaluate(() =>
      window.qx.decryptInRecoveryContext(window.qxChild)
    );
    const after = await page.evaluate(() => window.qxFixture.host.counts());
    await frame.evaluate(() => window.qxChild.close());
    expect(
      await page.evaluate(
        async () => (await window.qxFixture.host.stored()).length
      )
    ).toBe(2);
    expect(result.status).toBe('retained');
    // Opening the actual fresh identity performs its existing capability probe.
    expect(afterRetain.decrypts - before.decrypts).toBe(1);
    expect(afterRetain.signs).toBe(before.signs);
    expect(decrypted.snapshot).toMatchObject({
      attempted: 1,
      invalid: 1,
      admitted: 0
    });
    expect(after.decrypts - afterRetain.decrypts).toBe(2);
  });
  for (const lane of ['finite', 'sync'] as const)
    test(`${label} genuine ${lane} probe flood exhausts original delivery budget before exclusion`, async ({
      page
    }) => {
      await qualify(page, recovered);
      const wire = await page.evaluate(() => window.qxFixture.selfWire()),
        event = JSON.parse(wire) as (typeof receiveFrames)[number];
      receiveFrames = Array.from({ length: 501 }, () => event);
      receiveLane = 'history';
      const before = await page.evaluate(() => window.qxFixture.host.counts());
      expect(
        await page.evaluate((lane) => window.qxFixture.startReceive(lane), lane)
      ).toBe(true);
      await expect
        .poll(() =>
          page.evaluate(() => window.qxFixture.receiveSnapshot()?.reason)
        )
        .toBe('budget');
      await expect
        .poll(() =>
          page.evaluate(() => window.qxFixture.receiveSnapshot()?.pending)
        )
        .toBe(0);
      const result = await page.evaluate(async () => ({
        snapshot: window.qxFixture.receiveSnapshot(),
        received: (await window.qxFixture.receivedRows()).length,
        after: window.qxFixture.host.counts()
      }));
      expect(result.snapshot!.candidates).toBeGreaterThan(0);
      expect(result.snapshot!.candidates).toBeLessThanOrEqual(501);
      expect(result.snapshot!.bytes).toBe(
        result.snapshot!.candidates * Buffer.byteLength(wire)
      );
      expect(result.snapshot!.excludedProbes).toBeLessThanOrEqual(
        result.snapshot!.candidates
      );
      expect(result.received).toBe(0);
      expect(result.after).toEqual({
        ...before,
        keys: before.keys + (lane === 'sync' ? 2 : 1)
      });
    });
  for (const order of [
    'ordinary-first',
    'probe-first',
    'between-pages'
  ] as const)
    test(`${label} mixed 1999-row ${order} preserves bounded cursor facts and honest native retention`, async ({
      page
    }) => {
      await qualify(page, recovered);
      const nativeUntil = await page.evaluate(() =>
        window.qxFixture.seedReceived()
      );
      expect(
        await page.evaluate(() => window.qxFixture.startReceive('sync'))
      ).toBe(true);
      await liveReady(page);
      const wire = await page.evaluate(() => window.qxFixture.selfWire()),
        probe = JSON.parse(wire) as (typeof receiveFrames)[number];
      const normal = JSON.parse(
        await page.evaluate(
          ({ recovered, time }) =>
            window.qxFixture.ordinaryWire(recovered, time),
          { recovered, time: probe.created_at - 1 }
        )
      ) as (typeof receiveFrames)[number];
      receiveFrames = order === 'ordinary-first' ? [normal, probe] : [probe];
      if (order === 'probe-first') receiveFrames.push(normal);
      receiveLane = 'history';
      expect(await page.evaluate(() => window.qxFixture.older())).toBe(true);
      if (order === 'between-pages') {
        expect(
          (await page.evaluate(() => window.qxFixture.olderSnapshot()))!.reason
        ).toBeNull();
        expect(
          await page.evaluate(
            (wire) => window.qxFixture.retain(wire),
            JSON.stringify(normal)
          )
        ).toBe('retained');
        receiveFrames = [];
        expect(await page.evaluate(() => window.qxFixture.older())).toBe(false);
      }
      expect(
        await page.evaluate(() => window.qxFixture.olderSnapshot())
      ).toMatchObject({
        state: 'needs_action',
        reason: 'capacity',
        historyComplete: false
      });
      expect(
        await page.evaluate(
          async () => (await window.qxFixture.receivedRows()).length
        )
      ).toBe(2000);
      await page.evaluate(
        (id) => window.qxFixture.deleteReceived(id),
        normal.id
      );
      receiveFrames = [];
      expect(await page.evaluate(() => window.qxFixture.older())).toBe(true);
      const after = await page.evaluate(() => window.qxFixture.olderSnapshot());
      if (order === 'ordinary-first') {
        expect(after!.sources[0].seen).not.toContain(probe.id);
        expect(after!.sources[0].until).toBe(nativeUntil);
      } else {
        expect(after!.sources[0].seen).toContain(probe.id);
        expect(after!.sources[0].until).toBe(probe.created_at);
      }
      expect(after!.historyComplete).toBe(false);
    });
}

async function qualify(page: Page, recovered: boolean) {
  expect(
    await page.evaluate(
      async (recovered) => (await window.qxFixture.qualify(recovered)).status,
      recovered
    )
  ).toBe('saved');
  expect(
    await page.evaluate(
      async () => (await window.qxFixture.qualifyAccess()).status
    )
  ).toBe('qualified');
}
async function liveReady(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(() => {
        const snapshot = window.qxFixture.receiveSnapshot();
        return snapshot && 'backfill' in snapshot
          ? {
              state: snapshot.state,
              backfill: snapshot.backfill,
              pending: snapshot.pending
            }
          : undefined;
      })
    )
    .toEqual({ state: 'live', backfill: 'complete', pending: 0 });
}
for (const recovered of [false, true]) {
  const label = recovered ? 'REC' : 'original';
  for (const variation of ['extra', 'order'] as const)
    test(`${label} same signed event with ${variation} wire variation follows ordinary envelope admission`, async ({
      page
    }) => {
      const result = await page.evaluate(
        async ({ recovered, variation }) => {
          const f = window.qxFixture;
          if (recovered && (await f.qualify(true)).status !== 'saved')
            throw Error('actual REC source');
          const event = JSON.parse(await f.selfWire()) as Record<
            string,
            unknown
          >;
          const changed =
            variation === 'extra'
              ? { ...event, diagnostic: 'not source authority' }
              : Object.fromEntries(Object.entries(event).reverse());
          const before = f.host.counts();
          const status = await f.retain(JSON.stringify(changed));
          return {
            status,
            before,
            after: f.host.counts(),
            received: (await f.receivedRows()).length
          };
        },
        { recovered, variation }
      );
      expect(result.status).toBe(
        variation === 'extra' ? 'invalid' : 'retained'
      );
      expect(result.received).toBe(variation === 'extra' ? 0 : 1);
      expect(result.after).toEqual(result.before);
    });
  test(`${label} observed missing journal then handwritten exact restoration cannot revive exclusion`, async ({
    page
  }) => {
    const result = await page.evaluate(async (recovered) => {
      const f = window.qxFixture;
      if (recovered && (await f.qualify(true)).status !== 'saved')
        throw Error('actual REC source');
      const wire = await f.selfWire(),
        row = await f.rowWire();
      await f.removeFixtureRow();
      const first = await f.retain(wire);
      await f.replaceRow(row);
      const second = await f.retain(wire);
      return { first, second, received: (await f.receivedRows()).length };
    }, recovered);
    expect(result).toEqual({
      first: 'retained',
      second: 'duplicate',
      received: 1
    });
  });
  for (const mode of ['changed', 'alias', 'abort', 'corrupt'] as const)
    test(`${label} actual ${mode} native custody cannot exclude`, async ({
      page
    }) => {
      const result = await page.evaluate(
        async ({ recovered, mode }) => {
          const f = window.qxFixture;
          if (recovered && (await f.qualify(true)).status !== 'saved')
            throw Error('actual REC source');
          const wire = await f.selfWire(),
            row = await f.rowWire();
          if (mode === 'changed') {
            const parsed = JSON.parse(row) as { revision: number };
            parsed.revision++;
            await f.replaceRow(JSON.stringify(parsed));
          } else if (mode === 'alias')
            await f.replaceRow(row, crypto.randomUUID());
          else if (mode === 'corrupt')
            await f.replaceRow('{malformed own journal');
          const fault =
            mode === 'abort' ? f.nativeFault('initial_read_abort') : undefined;
          try {
            const status = await f.retain(wire);
            return {
              status,
              received: (await f.receivedRows()).length,
              corruptPreserved:
                mode !== 'corrupt' ||
                (await f.host.stored()).find((row) => row.id === f.command())
                  ?.wire === '{malformed own journal',
              reads: fault?.counts().readonly
            };
          } finally {
            fault?.restore();
          }
        },
        { recovered, mode }
      );
      expect(result.status).toBe('retained');
      expect(result.received).toBe(1);
      expect(result.corruptPreserved).toBe(true);
      if (mode === 'abort') expect(result.reads).toBe(1);
    });
  test(`${label} foreign native namespace is preserved and does not poison genuine exclusion`, async ({
    page
  }) => {
    const result = await page.evaluate(async (recovered) => {
      const f = window.qxFixture;
      if (recovered && (await f.qualify(true)).status !== 'saved')
        throw Error('actual REC source');
      const owner =
        'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
      const id = crypto.randomUUID(),
        wire = await f.rowWire();
      await f.replaceRow(wire, id, owner);
      const before = (await f.host.stored()).find((row) => row.id === id)!;
      const status = await f.retain();
      const after = (await f.host.stored()).find((row) => row.id === id)!;
      return {
        status,
        preserved: JSON.stringify(before) === JSON.stringify(after)
      };
    }, recovered);
    expect(result).toEqual({ status: 'probe_excluded', preserved: true });
  });
  for (const lane of ['finite', 'history', 'live'] as const)
    test(`${label} genuine qualified ${lane} receive charges exact probe deliveries without retention or prompts`, async ({
      page
    }) => {
      await qualify(page, recovered);
      const wire = await page.evaluate(() => window.qxFixture.selfWire());
      receiveFrames = [JSON.parse(wire) as (typeof receiveFrames)[number]];
      receiveLane = lane === 'live' ? 'live' : 'history';
      const before = await page.evaluate(() => window.qxFixture.host.counts());
      expect(
        await page.evaluate(
          async (lane) =>
            window.qxFixture.startReceive(
              lane === 'finite' ? 'finite' : 'sync'
            ),
          lane
        )
      ).toBe(true);
      await expect
        .poll(() =>
          page.evaluate(
            () => window.qxFixture.receiveSnapshot()?.excludedProbes
          )
        )
        .toBe(lane === 'history' ? 6 : 3);
      if (lane !== 'finite') await liveReady(page);
      const result = await page.evaluate(async () => ({
        snapshot: window.qxFixture.receiveSnapshot(),
        received: (await window.qxFixture.receivedRows()).length,
        after: window.qxFixture.host.counts()
      }));
      expect(result.snapshot).toMatchObject({
        candidates: lane === 'history' ? 6 : 3,
        excludedProbes: lane === 'history' ? 6 : 3,
        retained: 0,
        duplicates: 0,
        rejected: 0,
        pending: 0
      });
      expect(result.snapshot!.bytes).toBe(
        (lane === 'history' ? 6 : 3) * Buffer.byteLength(wire)
      );
      expect(result.received).toBe(0);
      const settled = await page.evaluate(() => window.qxFixture.host.counts());
      // Existing live admission plus its two bounded history windows each
      // recheck the extension owner; no decryption or signing is added.
      expect(result.after).toEqual({
        ...before,
        keys: before.keys + (lane === 'finite' ? 1 : 3)
      });
      expect(settled).toEqual(result.after);
    });
  for (const mixed of [false, true])
    test(`${label} real load-older ${mixed ? 'mixed' : 'probe-only'} page preserves signed cursor progress and actual costs`, async ({
      page
    }) => {
      await qualify(page, recovered);
      expect(
        await page.evaluate(() => window.qxFixture.startReceive('sync'))
      ).toBe(true);
      await liveReady(page);
      const wire = await page.evaluate(() => window.qxFixture.selfWire()),
        probe = JSON.parse(wire) as (typeof receiveFrames)[number];
      let ordinary: (typeof receiveFrames)[number] | undefined;
      if (mixed) {
        const normal = await page.evaluate(
          ({ recovered, time }) =>
            window.qxFixture.ordinaryWire(recovered, time),
          { recovered, time: probe.created_at - 1 }
        );
        ordinary = JSON.parse(normal) as (typeof receiveFrames)[number];
      }
      receiveFrames = ordinary ? [probe, ordinary, probe] : [probe, probe];
      receiveLane = 'history';
      const before = await page.evaluate(() => window.qxFixture.host.counts());
      expect(await page.evaluate(() => window.qxFixture.older())).toBe(true);
      const state = await page.evaluate(async () => ({
        older: window.qxFixture.olderSnapshot(),
        received: (await window.qxFixture.receivedRows()).length,
        after: window.qxFixture.host.counts()
      }));
      expect(state.older!.historyComplete).toBe(false);
      expect(state.older!.reason).toBeNull();
      for (const cursor of state.older!.sources) {
        expect(cursor.until).toBe(ordinary?.created_at ?? probe.created_at);
        expect(cursor.seen).toContain(probe.id);
        if (ordinary) expect(cursor.seen).toContain(ordinary.id);
        // Existing cursor policy preserves duplicate delivery costs: unique
        // signed facts are fewer than actual deliveries, so coverage is partial.
        expect(cursor.partial).toBe(true);
      }
      expect(state.received).toBe(mixed ? 1 : 0);
      expect(state.after).toEqual({ ...before, keys: before.keys + 3 });
      receiveFrames = [];
      expect(await page.evaluate(() => window.qxFixture.older())).toBe(true);
      const next = await page.evaluate(() => window.qxFixture.olderSnapshot());
      for (const cursor of next!.sources) {
        expect(cursor.until).toBe(ordinary?.created_at ?? probe.created_at);
        expect(cursor.seen).toContain(probe.id);
      }
      expect(
        requests.filter(
          (row) =>
            row.filter.kinds?.includes(1059) &&
            !row.filter.ids &&
            row.filter.limit !== 0
        ).length
      ).toBe(12);
    });
  test(`${label} successful reviewed removal returns self ciphertext to normal explicit decrypt cost and self-pair rejection`, async ({
    page
  }) => {
    await qualify(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.qxFixture,
        wire = await f.selfWire();
      if (!(await f.select()) || (await f.remove()).status !== 'removed')
        throw Error('actual successful reviewed cleanup');
      if (f.qualifyRemoved().status !== 'qualified')
        throw Error('actual removed source qualification');
      const before = f.host.counts(),
        status = await f.retain(wire),
        retained = (await f.receivedRows()).length;
      const afterRetain = f.host.counts(),
        decrypted = await f.decryptStored();
      return { status, retained, before, afterRetain, decrypted };
    });
    expect(result.status).toBe('retained');
    expect(result.retained).toBe(1);
    expect(result.afterRetain).toEqual(result.before);
    expect(result.decrypted.snapshot).toMatchObject({
      attempted: 1,
      invalid: 1,
      admitted: 0
    });
    expect(result.decrypted.rooms).toBe(0);
    expect(
      result.decrypted.after.decrypts - result.decrypted.before.decrypts
    ).toBe(2);
  });
  test(`${label} genuine two-party message with identical diagnostic body remains a normal conversation`, async ({
    page
  }) => {
    const result = await page.evaluate(async (recovered) => {
      const f = window.qxFixture;
      if (recovered && (await f.qualify(true)).status !== 'saved')
        throw Error('actual REC source');
      const wire = f.ordinaryWire(recovered),
        status = await f.retain(wire),
        decrypted = await f.decryptStored();
      return { status, decrypted };
    }, recovered);
    expect(result.status).toBe('retained');
    expect(result.decrypted.snapshot).toMatchObject({
      attempted: 1,
      invalid: 0,
      admitted: 1
    });
    expect(result.decrypted.rooms).toBe(1);
    expect(
      result.decrypted.after.decrypts - result.decrypted.before.decrypts
    ).toBe(2);
  });
  test(`${label} genuine native-observed delivery proof is one-use and cannot survive final source loss`, async ({
    page
  }) => {
    if (recovered)
      expect(
        await page.evaluate(
          async () => (await window.qxFixture.qualify(true)).status
        )
      ).toBe('saved');
    const result = await page.evaluate(async () => {
      const f = window.qxFixture,
        a = f.actors(),
        wire = await f.selfWire(),
        event = JSON.parse(wire) as { id: string };
      return await navigator.locks.request(
        'harvestcircle:owner:' + f.host.owner,
        { mode: 'exclusive', ifAvailable: true },
        async (lock) => {
          if (!lock) throw Error('actual owner WebLock');
          const first = await window.qx.observeInboxProbeDelivery(
            f.repository,
            a.session,
            event.id,
            wire
          );
          if (typeof first === 'string')
            throw Error('actual native-observed source proof');
          const copy = window.qx.consumeInboxProbeDelivery(
            { ...first },
            f.repository,
            a.session,
            event.id,
            wire
          );
          const current = window.qx.consumeInboxProbeDelivery(
            first,
            f.repository,
            a.session,
            event.id,
            wire
          );
          const repeat = window.qx.consumeInboxProbeDelivery(
            first,
            f.repository,
            a.session,
            event.id,
            wire
          );
          const later = await window.qx.observeInboxProbeDelivery(
            f.repository,
            a.session,
            event.id,
            wire
          );
          if (typeof later === 'string')
            throw Error('actual second native source proof');
          f.stop();
          const lost = window.qx.consumeInboxProbeDelivery(
            later,
            f.repository,
            a.session,
            event.id,
            wire
          );
          return { copy, current, repeat, lost };
        }
      );
    });
    expect(result).toEqual({
      copy: false,
      current: true,
      repeat: false,
      lost: false
    });
  });
}
