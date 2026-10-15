import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-access-qualification.ts';
import type { PrivateAccessProbe } from '../../src/lib/persistence/private-records.ts';
declare global {
  interface Window {
    qa: typeof Fixture;
    qaFixture: Awaited<ReturnType<typeof Fixture.makeQualificationFixture>>;
    qaPending?: Promise<unknown>;
    qaRestore?: () => void;
    qaFinishAttempt?: () => Promise<string>;
    qaRelease?: () => void;
    qaCloseMode?: 'hold' | 'fail';
    qaHeldClose?: (() => void)[];
    qaCloseCalls?: number;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string,
  head = '';
let ackMode: 'accept' | 'refuse' | 'hold' = 'accept';
let authMode: 'accept' | 'refuse' | 'hold' = 'accept';
let holdRead = false,
  challenge = false;
const origins = [
    'wss://inbox.example.org',
    'wss://peer.example.org',
    'wss://archive.example.org'
  ],
  physical = new Set<WebSocket>(),
  physicalOrigins = new WeakMap<WebSocket, string>(),
  archived = new Map<string, unknown>(),
  frames: { origin: string; frame: unknown[] }[] = [];
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
    socket.on('close', () => physical.delete(socket));
    const origin = new URL(request.url!, 'http://127.0.0.1').searchParams.get(
      'origin'
    )!;
    physicalOrigins.set(socket, origin);
    if (challenge && origin !== 'wss://discovery.example.org')
      socket.send(JSON.stringify(['AUTH', 's5a-source-challenge']));
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
          ? bytes
          : Array.isArray(bytes)
            ? Buffer.concat(bytes)
            : Buffer.from(bytes),
        frame = JSON.parse(raw.toString()) as unknown[];
      if (frames.length >= 1000) throw Error('controlled fixture frame bound');
      frames.push({ origin, frame });
      if (frame[0] === 'REQ') {
        const filter = frame[2] as { kinds?: number[]; ids?: string[] };
        if (filter.kinds?.includes(10050) && head)
          socket.send(JSON.stringify(['EVENT', frame[1], JSON.parse(head)]));
        if (filter.kinds?.includes(1059)) {
          if (holdRead) return;
          const event = archived.get(JSON.stringify([origin, filter.ids?.[0]]));
          if (event) socket.send(JSON.stringify(['EVENT', frame[1], event]));
        }
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      } else if (frame[0] === 'AUTH') {
        const event = frame[1] as { id: string };
        if (authMode !== 'hold')
          socket.send(
            JSON.stringify([
              'OK',
              event.id,
              authMode === 'accept',
              'controlled auth response'
            ])
          );
      } else if (frame[0] === 'EVENT') {
        const event = frame[1] as { id: string };
        if (ackMode === 'accept')
          archived.set(JSON.stringify([origin, event.id]), event);
        if (ackMode !== 'hold')
          socket.send(
            JSON.stringify([
              'OK',
              event.id,
              ackMode === 'accept',
              'controlled refusal'
            ])
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
          new URL('./harness/inbox-access-qualification.ts', import.meta.url)
        ),
        name: 'qa',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('actual compiled nonce-facts harness');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
test.beforeEach(async ({ page }, testInfo) => {
  await expect.poll(() => physical.size).toBe(0);
  ackMode = 'accept';
  authMode = 'accept';
  holdRead = false;
  challenge = false;
  archived.clear();
  frames.length = 0;
  head = '';
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
        override close(code?: number, reason?: string) {
          window.qaCloseCalls = (window.qaCloseCalls ?? 0) + 1;
          if (window.qaCloseMode === 'fail')
            throw Error('actual controlled physical close failure');
          if (window.qaCloseMode === 'hold')
            (window.qaHeldClose ??= []).push(() => super.close(code, reason));
          else super.close(code, reason);
        }
      };
    },
    { endpoint, origins }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(
    async ({ origins, authRequired }) => {
      window.qaFixture = await window.qa.makeQualificationFixture(authRequired);
      return window.qaFixture.host.preferenceWire(origins);
    },
    { origins, authRequired: testInfo.title.includes('required AUTH') }
  );
  await page.evaluate(async () => {
    const f = window.qaFixture;
    await f.open();
    if ((await f.prepareOriginal()).status !== 'prepared')
      throw Error('actual original S3/R1');
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(async () => {
      window.qaRestore?.();
      window.qaCloseMode = undefined;
      for (const release of window.qaHeldClose ?? []) release();
      window.qaRelease?.();
      window.qaFixture.host.mode('normal');
      window.qaFixture.host.settle();
      window.qaFixture.close();
      await window.qaPending;
    });
});

async function prepare(page: Page, recovered: boolean) {
  expect(
    await page.evaluate(
      async (recovered) => (await window.qaFixture.qualify(recovered)).status,
      recovered
    )
  ).toBe('saved');
}
for (const recovered of [false, true]) {
  for (const mode of ['hold', 'fail'] as const)
    test(`${recovered ? 'REC' : 'original'} qualification respects actual ${mode} physical cleanup without implicit retries`, async ({
      page
    }) => {
      await prepare(page, recovered);
      await page.evaluate(async () => {
        const f = window.qaFixture,
          pool = window.qa.getPrivatePool(
            f.actors().session,
            f.host.policy,
            ['wss://inbox.example.org'],
            Object.freeze({}),
            'receive'
          );
        if (!pool) throw Error('actual independent receive scope');
        const auth = await window.qa.beginInboxAuthentication(
          pool,
          'wss://inbox.example.org',
          'reviewed_connection_auth'
        );
        if (!auth) throw Error('actual independent AUTH connection');
        window.qaRelease = () => {
          window.qa.closeInboxAuthentication(auth);
          window.qa.closePrivatePool(pool);
        };
      });
      await expect
        .poll(() =>
          [...physical].some(
            (socket) =>
              physicalOrigins.get(socket) === 'wss://inbox.example.org'
          )
        )
        .toBe(true);
      const result = await page.evaluate(async (mode) => {
        const f = window.qaFixture;
        window.qaCloseMode = mode;
        let failed = false;
        try {
          window.qaRelease!();
        } catch {
          failed = true;
        }
        const before = f.host.counts(),
          closes = window.qaCloseCalls ?? 0,
          held = window.qaHeldClose?.length ?? 0,
          hook = f.nativeFault('read_loss');
        try {
          const result = await f.qualifyAccess();
          return {
            status: result.status,
            current: !!window.qa.inboxProbeNonceAcknowledgementBinding(
              f.repository,
              f.actors().session,
              f.nonce()!,
              f.host.policy
            ),
            before,
            after: f.host.counts(),
            closes,
            afterCloses: window.qaCloseCalls ?? 0,
            held,
            afterHeld: window.qaHeldClose?.length ?? 0,
            failed,
            native: hook.counts()
          };
        } finally {
          hook.restore();
        }
      }, mode);
      expect(result.status).toBe('busy');
      expect(result.current).toBe(true);
      expect(result.after).toEqual(result.before);
      expect(result.native.readonly).toBe(0);
      expect(result.afterCloses).toBe(result.closes);
      expect(result.afterHeld).toBe(result.held);
      if (mode === 'fail') expect(result.failed).toBe(true);
      else expect(result.held).toBeGreaterThan(0);
      await page.evaluate(() => {
        window.qaCloseMode = undefined;
        for (const release of window.qaHeldClose ?? []) release();
        try {
          window.qaRelease!();
        } catch (error) {
          if (
            !(error instanceof Error) ||
            ![
              'private_pool_close_pending',
              'private_pool_close_failed'
            ].includes(error.message)
          )
            throw error;
        }
      });
      await expect
        .poll(() => page.evaluate(() => window.qaFixture.idle()))
        .toBe(true);
      expect(
        await page.evaluate(
          async () => (await window.qaFixture.qualifyAccess()).status
        )
      ).toBe('qualified');
    });
  for (const foreign of [
    'operator',
    'policy',
    'controller',
    'session',
    'repository'
  ] as const)
    test(`${recovered ? 'REC' : 'original'} removed qualification rejects different genuine or copied ${foreign} with no effects`, async ({
      page
    }) => {
      await prepare(page, recovered);
      const result = await page.evaluate(async (foreign) => {
        const f = window.qaFixture;
        if (!(await f.select()) || (await f.remove()).status !== 'removed')
          throw Error('actual reviewed removal');
        const a = f.actors(),
          other =
            foreign === 'controller'
              ? {
                  access: (await f.replaceController())!,
                  session: a.session,
                  close() {}
                }
              : foreign === 'session'
                ? await f.alternateController(true)
                : undefined,
          opened =
            foreign === 'repository'
              ? await window.qa.openBrowserDatabase()
              : undefined,
          repository =
            opened?.state === 'ready'
              ? window.qa.createPrivateStorageRepository(
                  opened.owner,
                  f.host.owner
                )!
              : f.repository,
          policy =
            foreign === 'policy'
              ? window.qa.validateRelayPolicy(
                  JSON.stringify(window.qa.readRelayPolicy(f.host.policy))
                )!
              : f.host.policy,
          operator =
            foreign === 'operator'
              ? window.qa.validateInboxOperatorQualification(
                  window.qa.inboxOperatorQualificationWire(
                    f.host.qualification,
                    f.host.policy
                  )!
                )!
              : f.host.qualification,
          before = f.host.counts(),
          hook = f.nativeFault('read_loss');
        try {
          const result = window.qa.qualifyRemovedInboxAccess(
            repository,
            other?.session ?? a.session,
            policy,
            other?.access ?? a.access,
            operator,
            f.removal()!,
            'reviewed_qualify_inbox_access'
          );
          return {
            status: result.status,
            before,
            after: f.host.counts(),
            native: hook.counts()
          };
        } finally {
          hook.restore();
          other?.close();
          if (opened?.state === 'ready')
            window.qa.closeBrowserDatabase(opened.owner);
        }
      }, foreign);
      expect(result.status).toBe('invalid_scope');
      expect(result.after).toEqual(result.before);
      expect(result.native.readonly).toBe(0);
      expect(result.native.deletes).toBe(0);
    });
  for (const forged of ['nonce', 'observations'] as const)
    test(`${recovered ? 'REC' : 'original'} copied genuine ${forged} cannot substitute for applied source provenance`, async ({
      page
    }) => {
      await prepare(page, recovered);
      const result = await page.evaluate(async (forged) => {
        const f = window.qaFixture,
          a = f.actors(),
          before = f.host.counts(),
          hook = f.nativeFault('read_loss');
        try {
          const result = await window.qa.qualifyInboxAccess(
            f.repository,
            a.session,
            f.host.policy,
            a.access,
            f.host.qualification,
            forged === 'nonce' ? { ...f.nonce()! } : f.nonce()!,
            forged === 'observations'
              ? f.observations().map((x) => ({ ...x }))
              : f.observations(),
            'reviewed_qualify_inbox_access'
          );
          return {
            status: result.status,
            before,
            after: f.host.counts(),
            native: hook.counts()
          };
        } finally {
          hook.restore();
        }
      }, forged);
      expect(result.status).toBe('invalid_scope');
      expect(result.after).toEqual(result.before);
      expect(result.native.readonly).toBe(0);
    });
  test(`${recovered ? 'REC' : 'original'} a copied genuine removal cannot issue access and the exact real removal remains eligible`, async ({
    page
  }) => {
    await prepare(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.qaFixture,
        a = f.actors();
      if (!(await f.select()) || (await f.remove()).status !== 'removed')
        throw Error('actual successful reviewed removal');
      const before = f.host.counts(),
        forged = window.qa.qualifyRemovedInboxAccess(
          f.repository,
          a.session,
          f.host.policy,
          a.access,
          f.host.qualification,
          { ...f.removal()! },
          'reviewed_qualify_inbox_access'
        ),
        genuine = f.qualifyRemoved();
      return {
        forged: forged.status,
        genuine: genuine.status,
        before,
        after: f.host.counts()
      };
    });
    expect(result.forged).toBe('invalid_scope');
    expect(result.genuine).toBe('qualified');
    expect(result.after).toEqual(result.before);
  });
  test(`${recovered ? 'REC' : 'original'} wrong fresh owner key cannot qualify`, async ({
    page
  }) => {
    await prepare(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.qaFixture,
        before = f.host.counts(),
        hook = f.nativeFault('read_loss');
      f.host.mode('wrong_key');
      try {
        const result = await f.qualifyAccess();
        return {
          status: result.status,
          before,
          after: f.host.counts(),
          native: hook.counts()
        };
      } finally {
        f.host.mode('normal');
        hook.restore();
      }
    });
    expect(result.status).toBe('stopped');
    expect(result.after).toEqual({
      ...result.before,
      keys: result.before.keys + 1
    });
    expect(result.native.readonly).toBe(0);
  });
  test(`${recovered ? 'REC' : 'original'} explicit qualification Stop prevents reuse of the same genuine nonce`, async ({
    page
  }) => {
    await prepare(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.qaFixture,
        first = await f.qualifyAccess(),
        token = first.qualification!,
        context = f.context(),
        before = f.host.counts();
      window.qa.stopInboxAccessQualification(token);
      const again = await f.qualifyAccess();
      return {
        current:
          window.qa.observeQualifiedInboxAccess(token)(context)?.current() ??
          false,
        snapshot: window.qa.inboxAccessQualificationSnapshot(token),
        status: again.status,
        before,
        after: f.host.counts()
      };
    });
    expect(result).toMatchObject({ current: false, status: 'stopped' });
    expect(result.snapshot).toBeUndefined();
    expect(result.after).toEqual(result.before);
  });
  test(`${recovered ? 'REC' : 'original'} unrelated actual SDK local storage work does not invalidate issued access`, async ({
    page
  }) => {
    await prepare(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.qaFixture;
      await f.qualifyAccess();
      const context = f.context(),
        before = f.host.counts();
      const during = await f.withHeld('SDK', async () => ({
        current: f.observation(context)?.current(),
        status: (await f.qualifyAccess()).status
      }));
      return {
        during,
        afterCurrent: f.observation(context)?.current(),
        before,
        after: f.host.counts()
      };
    });
    expect(result).toMatchObject({
      during: { current: true, status: 'qualified' },
      afterCurrent: true
    });
    expect(result.after).toEqual(result.before);
  });
  for (const lane of ['archive', 'read'] as const)
    test(`${recovered ? 'REC' : 'original'} later actual selected ${lane} immediately and permanently invalidates issued access`, async ({
      page
    }) => {
      await prepare(page, recovered);
      expect(
        await page.evaluate(
          async () => (await window.qaFixture.qualifyAccess()).status
        )
      ).toBe('qualified');
      const mark = traffic().length;
      if (lane === 'archive') ackMode = 'hold';
      else holdRead = true;
      await page.evaluate(
        ({ recovered, lane }) => {
          const f = window.qaFixture;
          window.qaPending =
            lane === 'archive'
              ? recovered
                ? f.publish('wss://peer.example.org')
                : f.originalPublish('wss://peer.example.org')
              : recovered
                ? f.read('wss://peer.example.org')
                : f.originalRead('wss://peer.example.org');
        },
        { recovered, lane }
      );
      await expect.poll(() => traffic().length).toBeGreaterThan(mark);
      const during = await page.evaluate(async () => {
        const f = window.qaFixture,
          before = f.host.counts(),
          result = await f.qualifyAccess();
        return {
          status: result.status,
          current: f.observation(f.context())?.current() ?? false,
          before,
          after: f.host.counts()
        };
      });
      expect(during.status).toBe('stopped');
      expect(during.current).toBe(false);
      expect(during.after).toEqual(during.before);
      const actual = traffic()
        .slice(mark)
        .find((x) => x.frame[0] === (lane === 'archive' ? 'EVENT' : 'REQ'))!;
      for (const socket of physical)
        if (physicalOrigins.get(socket) === actual.origin) {
          if (lane === 'archive') {
            const event = actual.frame[1] as { id: string };
            archived.set(JSON.stringify([actual.origin, event.id]), event);
            socket.send(
              JSON.stringify([
                'OK',
                event.id,
                true,
                'actual held archive accepted'
              ])
            );
          } else socket.send(JSON.stringify(['EOSE', actual.frame[1]]));
        }
      const final = await page.evaluate(async () => {
        const attempt = (await window.qaPending) as { status: string };
        const f = window.qaFixture,
          before = f.host.counts(),
          again = await f.qualifyAccess();
        return {
          attempt: attempt.status,
          status: again.status,
          current: f.observation(f.context())?.current() ?? false,
          before,
          after: f.host.counts()
        };
      });
      expect(final.attempt).toBe(lane === 'archive' ? 'accepted' : 'empty');
      expect(final.status).toBe('stopped');
      expect(final.current).toBe(false);
      expect(final.after).toEqual(final.before);
    });
  test(`${recovered ? 'REC' : 'original'} unapplied archive blocks first qualification and later append cannot revive that input`, async ({
    page
  }) => {
    await prepare(page, recovered);
    const result = await page.evaluate(async (recovered) => {
      const f = window.qaFixture,
        archive = await (recovered
          ? f.publish('wss://peer.example.org')
          : f.originalPublish('wss://peer.example.org'));
      if (!archive || archive.status !== 'accepted')
        throw Error('actual accepted archive');
      const before = f.host.counts(),
        first = await f.qualifyAccess(),
        after = f.host.counts(),
        appended = await f.append(archive),
        again = await f.qualifyAccess([...f.observations(), archive]);
      return {
        first: first.status,
        again: again.status,
        appended: appended.status,
        before,
        after
      };
    }, recovered);
    expect(result).toMatchObject({
      first: 'stopped',
      again: 'stopped',
      appended: 'saved'
    });
    expect(result.after).toEqual(result.before);
  });
  test(`${recovered ? 'REC' : 'original'} an actual cleanup lease and aborted delete permanently retire the issued journal fence`, async ({
    page
  }) => {
    await prepare(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.qaFixture,
        first = await f.qualifyAccess(),
        old = window.qa.observeQualifiedInboxAccess(first.qualification!)(
          f.context()
        )!;
      if (!(await f.select())) throw Error('actual selected cleanup');
      const hook = f.nativeFault('abort');
      try {
        const removal = await f.remove(),
          before = f.host.counts(),
          current = old.current(),
          again = await f.qualifyAccess();
        const rows = (await f.host.stored()).length;
        return {
          removed: removal.status,
          counts: hook.counts(),
          current,
          again: again.status,
          before,
          after: f.host.counts(),
          rows
        };
      } finally {
        hook.restore();
      }
    });
    expect(result.removed).toBe('aborted');
    expect(result.counts).toMatchObject({
      deletes: 1,
      completes: 0,
      aborts: 1
    });
    expect(result.rows).toBe(1);
    expect(result.current).toBe(false);
    expect(result.again).toBe('stopped');
    expect(result.after).toEqual(result.before);
  });
  for (const mode of ['accept', 'refuse', 'hold'] as const)
    test(`${recovered ? 'REC' : 'original'} required AUTH ${mode} uses actual C7 response and challenge currentness`, async ({
      page
    }) => {
      await prepare(page, recovered);
      const before = await page.evaluate(() => window.qaFixture.host.counts());
      expect(
        await page.evaluate(
          async () => (await window.qaFixture.qualifyAccess()).status
        )
      ).toBe('invalid_scope');
      expect(await page.evaluate(() => window.qaFixture.host.counts())).toEqual(
        before
      );
      challenge = true;
      expect(['waiting', 'challenge']).toContain(
        await page.evaluate(async () =>
          window.qa.authenticateInboxAccess(
            window.qaFixture.actors().access,
            'wss://inbox.example.org',
            'reviewed_inbox_connection_auth'
          )
        )
      );
      for (const socket of physical)
        if (physicalOrigins.get(socket) === 'wss://inbox.example.org')
          socket.send(JSON.stringify(['AUTH', 's5a-source-challenge']));
      await expect
        .poll(() =>
          page.evaluate(
            () =>
              window.qa
                .inboxAccessControllerSnapshot(window.qaFixture.actors().access)
                ?.auth.find((x) => x.origin === 'wss://inbox.example.org')
                ?.state
          )
        )
        .toBe('challenge');
      expect(
        await page.evaluate(
          async () => (await window.qaFixture.qualifyAccess()).status
        )
      ).toBe('invalid_scope');
      authMode = mode;
      await page.evaluate(() => {
        window.qaPending = window.qa.respondInboxAccessAuthentication(
          window.qaFixture.actors().access,
          'wss://inbox.example.org',
          'reviewed_inbox_auth_response'
        );
      });
      await expect
        .poll(() => frames.filter((x) => x.frame[0] === 'AUTH').length)
        .toBe(1);
      if (mode === 'hold') {
        expect(
          await page.evaluate(
            async () => (await window.qaFixture.qualifyAccess()).status
          )
        ).toBe('invalid_scope');
        // Complete this exact held response on its actual connection.
        const sent = frames.find((x) => x.frame[0] === 'AUTH')!,
          event = sent.frame[1] as { id: string };
        for (const socket of physical)
          if (physicalOrigins.get(socket) === sent.origin)
            socket.send(
              JSON.stringify([
                'OK',
                event.id,
                true,
                'controlled held auth accepted'
              ])
            );
      }
      expect(await page.evaluate(async () => await window.qaPending)).toBe(
        mode === 'refuse' ? 'refused' : 'accepted'
      );
      const result = await page.evaluate(async () => {
        const f = window.qaFixture,
          before = f.host.counts(),
          q = await f.qualifyAccess();
        return {
          status: q.status,
          before,
          after: f.host.counts(),
          current: f.observation(f.context())?.current()
        };
      });
      if (mode === 'refuse') {
        expect(result.status).toBe('invalid_scope');
        expect(result.after).toEqual(result.before);
      } else {
        expect(result.status).toBe('qualified');
        expect(result.current).toBe(true);
        for (const socket of physical)
          if (physicalOrigins.get(socket) === 'wss://inbox.example.org')
            socket.send(JSON.stringify(['AUTH', 's5a-new-real-challenge']));
        await expect
          .poll(() =>
            page.evaluate(
              () =>
                window.qa.inboxAccessControllerSnapshot(
                  window.qaFixture.actors().access
                )?.auth[0]?.state
            )
          )
          .toBe('challenge');
        expect(
          await page.evaluate(
            () =>
              window.qaFixture
                .observation(window.qaFixture.context())
                ?.current() ?? false
          )
        ).toBe(false);
      }
    });
  for (const held of ['lock', 'SDK'] as const)
    test(`${recovered ? 'REC' : 'original'} actual ${held} contention refuses before provider and permits an explicit retry`, async ({
      page
    }) => {
      await prepare(page, recovered);
      const result = await page.evaluate(async (held) => {
        const f = window.qaFixture,
          before = f.host.counts(),
          hook = f.nativeFault('read_loss');
        try {
          const denied = await f.withHeld(held, () => f.qualifyAccess()),
            during = hook.counts(),
            after = f.host.counts(),
            retry = await f.qualifyAccess();
          return {
            status: denied.status,
            before,
            after,
            during,
            retry: retry.status
          };
        } finally {
          hook.restore();
        }
      }, held);
      expect(result.status).toBe('busy');
      expect(result.after).toEqual(result.before);
      expect(result.during.readonly).toBe(0);
      expect(result.retry).toBe('qualified');
    });
  for (const kind of [
    'missing',
    'changed',
    'corrupt_tail',
    'alias',
    'foreign_tail'
  ] as const)
    test(`${recovered ? 'REC' : 'original'} actual native ${kind} is checked before qualification`, async ({
      page
    }) => {
      await prepare(page, recovered);
      const result = await page.evaluate(async (kind) => {
        const f = window.qaFixture,
          row = (await f.host.stored()).find((x) => x.id === f.command())!;
        if (kind === 'missing') await f.removeFixtureRow();
        else if (kind === 'changed') {
          const record = JSON.parse(row.wire) as PrivateAccessProbe;
          await f.host.inject([
            {
              ...row,
              wire: JSON.stringify({ ...record, revision: record.revision + 1 })
            }
          ]);
        } else if (kind === 'alias')
          await f.host.inject([{ ...row, id: crypto.randomUUID() }]);
        else
          await f.host.inject([
            {
              owner: kind === 'foreign_tail' ? 'f'.repeat(64) : f.host.owner,
              id: crypto.randomUUID(),
              wire: '{'
            }
          ]);
        const rows = await f.host.stored(),
          before = f.host.counts(),
          result = await f.qualifyAccess(),
          after = f.host.counts();
        return {
          status: result.status,
          before,
          after,
          unchanged:
            JSON.stringify(rows) === JSON.stringify(await f.host.stored())
        };
      }, kind);
      if (kind === 'foreign_tail') expect(result.status).toBe('qualified');
      else expect(result.status).not.toBe('qualified');
      expect(result.after).toEqual({
        ...result.before,
        keys: result.before.keys + 1
      });
      expect(result.unchanged).toBe(true);
    });
  for (const at of ['provider', 'native'] as const)
    for (const loss of ['head', 'controller', 'session'] as const)
      test(`${recovered ? 'REC' : 'original'} genuine ${loss} loss at ${at} cannot issue access`, async ({
        page
      }) => {
        await prepare(page, recovered);
        const result = await page.evaluate(
          async ({ at, loss }) => {
            const f = window.qaFixture,
              a = f.actors();
            let losses = 0;
            const lose = () => {
              losses++;
              if (loss === 'head') a.churnHead();
              else if (loss === 'session')
                window.qa.closePrivateSession(a.session);
              else f.stop();
            };
            const off =
              at === 'provider' ? f.host.keyCallback(1, lose) : undefined;
            const hook =
              at === 'native' ? f.nativeFault('read_loss', lose) : undefined;
            const before = f.host.counts();
            try {
              const result = await f.qualifyAccess();
              return {
                status: result.status,
                before,
                after: f.host.counts(),
                losses,
                native: hook?.counts()
              };
            } finally {
              off?.();
              hook?.restore();
            }
          },
          { at, loss }
        );
        expect(result.losses).toBe(1);
        expect(result.status).not.toBe('qualified');
        expect(result.after).toEqual({
          ...result.before,
          keys: result.before.keys + 1
        });
        if (at === 'native') expect(result.native?.deletes).toBe(0);
      });
  test(`${recovered ? 'REC' : 'original'} actual native qualification read abort cannot issue access`, async ({
    page
  }) => {
    await prepare(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.qaFixture,
        hook = f.nativeFault('initial_read_abort');
      try {
        const result = await f.qualifyAccess();
        return { status: result.status, counts: hook.counts() };
      } finally {
        hook.restore();
      }
    });
    expect(result.status).not.toBe('qualified');
    expect(result.counts.readonly).toBe(1);
    expect(result.counts.deletes).toBe(0);
  });
  test(`${recovered ? 'REC' : 'original'} held fresh key serializes reentry and controller stop fences late settlement`, async ({
    page
  }) => {
    await prepare(page, recovered);
    await page.evaluate(() => {
      const f = window.qaFixture;
      f.host.mode('hold_key');
      window.qaPending = f.qualifyAccess();
    });
    await expect
      .poll(() => page.evaluate(() => window.qaFixture.host.waiting()))
      .toBe('key');
    const during = await page.evaluate(async () => {
      const f = window.qaFixture,
        before = f.host.counts(),
        second = await f.qualifyAccess();
      f.stop();
      f.host.settle();
      const first = await window.qaPending;
      return { before, after: f.host.counts(), second: second.status, first };
    });
    expect(during.second).toBe('busy');
    expect(during.after).toEqual(during.before);
    expect(during.first).toMatchObject({ status: 'stopped' });
  });
  for (const foreign of ['controller', 'session', 'repository'] as const)
    test(`${recovered ? 'REC' : 'original'} different genuine ${foreign} cannot consume source qualification`, async ({
      page
    }) => {
      await prepare(page, recovered);
      const result = await page.evaluate(async (foreign) => {
        const f = window.qaFixture,
          a = f.actors(),
          other =
            foreign === 'controller'
              ? {
                  access: (await f.replaceController())!,
                  session: a.session,
                  close() {}
                }
              : foreign === 'session'
                ? await f.alternateController(true)
                : undefined,
          opened =
            foreign === 'repository'
              ? await window.qa.openBrowserDatabase()
              : undefined;
        const repository =
          opened?.state === 'ready'
            ? window.qa.createPrivateStorageRepository(
                opened.owner,
                f.host.owner
              )!
            : f.repository;
        const before = f.host.counts(),
          hook = f.nativeFault('read_loss');
        try {
          const result = await window.qa.qualifyInboxAccess(
            repository,
            other?.session ?? a.session,
            f.host.policy,
            other?.access ?? a.access,
            f.host.qualification,
            f.nonce()!,
            f.observations(),
            'reviewed_qualify_inbox_access'
          );
          return {
            status: result.status,
            before,
            after: f.host.counts(),
            native: hook.counts()
          };
        } finally {
          hook.restore();
          other?.close();
          if (opened?.state === 'ready')
            window.qa.closeBrowserDatabase(opened.owner);
        }
      }, foreign);
      expect(result.status).toBe('invalid_scope');
      expect(result.after).toEqual(result.before);
      expect(result.native.readonly).toBe(0);
    });
  test(`${recovered ? 'REC' : 'original'} a genuinely later applied read and distinct nonce restores access within the original meter`, async ({
    page
  }) => {
    await prepare(page, recovered);
    archived.clear();
    expect(
      await page.evaluate(async (recovered) => {
        const f = window.qaFixture;
        return (
          await (recovered
            ? f.read('wss://inbox.example.org')
            : f.originalRead('wss://inbox.example.org'))
        )?.status;
      }, recovered)
    ).toBe('empty');
    expect(
      await page.evaluate(
        async () => (await window.qaFixture.qualifyAccess()).status
      )
    ).toBe('stopped');
    // Restore only the actual signed archived event, through a real accepted EVENT.
    expect(
      await page.evaluate(async (recovered) => {
        const f = window.qaFixture,
          archive = await (recovered
            ? f.publish('wss://peer.example.org')
            : f.originalPublish('wss://peer.example.org'));
        if (!archive) throw Error('actual remaining archive attempt');
        return (await f.append(archive)).status;
      }, recovered)
    ).toBe('saved');
    expect(
      await page.evaluate(
        async (recovered) =>
          window.qaFixture.renewQualificationNonce(
            recovered,
            'wss://peer.example.org'
          ),
        recovered
      )
    ).toBe('saved');
    expect(
      await page.evaluate(
        async () => (await window.qaFixture.qualifyAccess()).status
      )
    ).toBe('qualified');
  });
  for (const afterRemoval of [false, true])
    test(`${recovered ? 'REC' : 'original'} later applied archive cannot mask an unaccounted read ${afterRemoval ? 'through removal' : 'before first qualification'}`, async ({
      page
    }) => {
      await prepare(page, recovered);
      archived.clear();
      const result = await page.evaluate(
        async ({ recovered, afterRemoval }) => {
          const f = window.qaFixture,
            read = await (recovered
              ? f.read('wss://peer.example.org')
              : f.originalRead('wss://peer.example.org')),
            archive = await (recovered
              ? f.publish('wss://archive.example.org')
              : f.originalPublish('wss://archive.example.org'));
          if (!archive || archive.status !== 'accepted')
            throw Error('actual later accepted archive');
          const appended = await f.append(archive),
            observations = [...f.observations(), archive],
            before = f.host.counts();
          let status: string;
          let removed: string | undefined;
          if (afterRemoval) {
            if (
              !(await f.select(
                'reviewed_inbox_probe_cleanup_selection',
                observations
              ))
            )
              throw Error('actual selection with later archive');
            removed = (await f.remove()).status;
            status = f.qualifyRemoved().status;
          } else status = (await f.qualifyAccess(observations)).status;
          return {
            read: read?.status,
            appended: appended.status,
            status,
            removed,
            coverage: !!f.removalBinding()?.qualification,
            before,
            after: f.host.counts()
          };
        },
        { recovered, afterRemoval }
      );
      expect(result.read).toBe('empty');
      expect(result.appended).toBe('saved');
      if (afterRemoval) {
        expect(result.removed).toBe('removed');
        expect(result.coverage).toBe(false);
        expect(result.status).toBe('invalid_scope');
      } else {
        expect(result.status).toBe('stopped');
        expect(result.after).toEqual(result.before);
      }
    });
  test(`${recovered ? 'REC' : 'original'} actual retained journal qualifies with one fresh key and no crypto/network/write`, async ({
    page
  }) => {
    await prepare(page, recovered);
    const beforeTraffic = traffic().length;
    const result = await page.evaluate(async () => {
      const f = window.qaFixture,
        before = f.host.counts(),
        rows = await f.host.stored(),
        first = await f.qualifyAccess(),
        token = first.qualification!,
        context = f.context(),
        observation = f.observation(context),
        after = f.host.counts(),
        again = await f.qualifyAccess();
      return {
        status: first.status,
        before,
        after,
        repeated: f.host.counts(),
        same: again.qualification === token,
        current: observation?.current(),
        snapshot: f.snapshot(),
        unchanged:
          JSON.stringify(rows) === JSON.stringify(await f.host.stored()),
        copied: !!window.qa.observeQualifiedInboxAccess({ ...token })(context)
      };
    });
    expect(result.status).toBe('qualified');
    expect(result.after).toEqual({
      ...result.before,
      keys: result.before.keys + 1
    });
    expect(result.repeated).toEqual(result.after);
    expect(result).toMatchObject({
      same: true,
      current: true,
      unchanged: true,
      copied: false,
      snapshot: { authority: 'source_fixture', sourceOnly: true }
    });
    expect(traffic().length).toBe(beforeTraffic);
  });
  test(`${recovered ? 'REC' : 'original'} actual successful cleanup preserves independent qualified coverage`, async ({
    page
  }) => {
    await prepare(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.qaFixture,
        initial = await f.qualifyAccess(),
        old = window.qa.observeQualifiedInboxAccess(initial.qualification!)(
          f.context()
        )!;
      if (!(await f.select())) throw Error('actual cleanup selection');
      const removed = await f.remove(),
        before = f.host.counts(),
        qualified = f.qualifyRemoved(),
        after = f.host.counts();
      return {
        removed: removed.status,
        status: qualified.status,
        before,
        after,
        old: old.current(),
        current: f.observation(f.context())?.current(),
        native: (await f.host.stored()).length,
        coverage: !!f.removalBinding()?.qualification
      };
    });
    expect(result).toMatchObject({
      removed: 'removed',
      status: 'qualified',
      old: false,
      current: true,
      native: 0,
      coverage: true
    });
    expect(result.after).toEqual(result.before);
  });
  for (const beforeFirst of [false, true])
    test(`${recovered ? 'REC' : 'original'} unjournaled read gap ${beforeFirst ? 'before first issuance' : 'after issuance'} never revives access through cleanup`, async ({
      page
    }) => {
      await prepare(page, recovered);
      if (!beforeFirst)
        expect(
          await page.evaluate(
            async () => (await window.qaFixture.qualifyAccess()).status
          )
        ).toBe('qualified');
      archived.clear();
      const result = await page.evaluate(async (recovered) => {
        const f = window.qaFixture,
          rows = await f.host.stored(),
          read = await (recovered
            ? f.read('wss://peer.example.org')
            : f.originalRead('wss://peer.example.org')),
          before = f.host.counts(),
          qualified = await f.qualifyAccess(),
          after = f.host.counts(),
          unchanged =
            JSON.stringify(rows) === JSON.stringify(await f.host.stored());
        if (!(await f.select()))
          throw Error('genuine C3B cleanup still eligible');
        const removal = await f.remove(),
          postBefore = f.host.counts(),
          post = f.qualifyRemoved(),
          postAfter = f.host.counts();
        return {
          read: read?.status,
          status: qualified.status,
          before,
          after,
          unchanged,
          removed: removal.status,
          post: post.status,
          postBefore,
          postAfter,
          coverage: !!f.removalBinding()?.qualification,
          rows: (await f.host.stored()).length
        };
      }, recovered);
      expect(result).toMatchObject({
        read: 'empty',
        status: 'stopped',
        unchanged: true,
        removed: 'removed',
        post: 'invalid_scope',
        coverage: false,
        rows: 0
      });
      expect(result.after).toEqual(result.before);
      expect(result.postAfter).toEqual(result.postBefore);
    });
  for (const removedPath of [false, true])
    test(`${recovered ? 'REC' : 'original'} ${removedPath ? 'removed' : 'journal'} genuine qualification survives its own actual foreground inbox`, async ({
      page
    }) => {
      await prepare(page, recovered);
      expect(
        await page.evaluate(async (removedPath) => {
          const f = window.qaFixture;
          if (!removedPath) return (await f.qualifyAccess()).status;
          if (!(await f.select()) || (await f.remove()).status !== 'removed')
            throw Error('actual own-inbox reviewed removal');
          return f.qualifyRemoved().status;
        }, removedPath)
      ).toBe('qualified');
      expect(
        await page.evaluate(async () => await window.qaFixture.startOwnInbox())
      ).toBe(true);
      await expect
        .poll(
          async () =>
            await page.evaluate(() => window.qaFixture.syncSnapshot()?.state)
        )
        .toBe('live');
      expect(
        await page.evaluate(() =>
          window.qaFixture.observation(window.qaFixture.context())?.current()
        )
      ).toBe(true);
      expect(traffic().some((x) => x.frame[0] === 'REQ')).toBe(true);
      const firstStop = await page.evaluate(() => {
        try {
          window.qaFixture.stopOwnInbox();
          return 'closed';
        } catch (error) {
          if (
            error instanceof Error &&
            error.message === 'private_inbox_cleanup_required'
          )
            return 'physical_close_pending';
          throw error;
        }
      });
      if (firstStop === 'physical_close_pending') {
        await expect
          .poll(
            () =>
              Array.from(physical).filter(
                (socket) =>
                  physicalOrigins.get(socket) !== 'wss://discovery.example.org'
              ).length
          )
          .toBe(0);
        await page.evaluate(() => window.qaFixture.stopOwnInbox());
      }
    });
  for (const kind of ['operator', 'policy'] as const)
    test(`${recovered ? 'REC' : 'original'} same-wire copied ${kind} is not qualification provenance`, async ({
      page
    }) => {
      await prepare(page, recovered);
      const result = await page.evaluate(async (kind) => {
        const f = window.qaFixture,
          a = f.actors(),
          before = f.host.counts(),
          policy =
            kind === 'policy'
              ? window.qa.validateRelayPolicy(
                  JSON.stringify(window.qa.readRelayPolicy(f.host.policy))
                )!
              : f.host.policy,
          operator =
            kind === 'operator'
              ? window.qa.validateInboxOperatorQualification(
                  window.qa.inboxOperatorQualificationWire(
                    f.host.qualification,
                    f.host.policy
                  )!
                )!
              : f.host.qualification,
          result = await window.qa.qualifyInboxAccess(
            f.repository,
            a.session,
            policy,
            a.access,
            operator,
            f.nonce()!,
            f.observations(),
            'reviewed_qualify_inbox_access'
          );
        return { status: result.status, before, after: f.host.counts() };
      }, kind);
      expect(result.status).toBe('invalid_scope');
      expect(result.after).toEqual(result.before);
    });
}

const traffic = () =>
  frames.filter(
    (x) =>
      x.origin !== 'wss://discovery.example.org' &&
      ['EVENT', 'REQ', 'AUTH'].includes(String(x.frame[0]))
  );
