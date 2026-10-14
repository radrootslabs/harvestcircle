import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-probe-retry.ts';
declare global {
  interface Window {
    pt: typeof Fixture;
    ptFixture: Awaited<ReturnType<typeof Fixture.makeRetryFixture>>;
    ptPending?: Promise<unknown>;
    ptRelease?: () => void;
    ptRestore?: () => void;
    ptClock?: ReturnType<Window['ptFixture']['installNetworkClock']>;
    ptAbort?: AbortController;
    ptDelayClose?: boolean;
    ptFailClose?: boolean;
    ptHeldClose?: (() => void)[];
    ptCloseCalls?: number;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string,
  head = '',
  holdEvent = false,
  holdRead = false,
  challenge = false,
  holdAuth = false;
const origins = [
  'wss://inbox.example.org',
  'wss://peer.example.org',
  'wss://archive.example.org'
];
const physical = new Set<WebSocket>(),
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
    if (challenge && origin !== 'wss://discovery.example.org')
      socket.send(JSON.stringify(['AUTH', 'c2b-actual-source-challenge']));
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
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
      } else if (frame[0] === 'EVENT') {
        const event = frame[1] as { id: string };
        archived.set(JSON.stringify([origin, event.id]), event);
        if (!holdEvent) socket.send(JSON.stringify(['OK', event.id, true, '']));
      } else if (frame[0] === 'AUTH') {
        const event = frame[1] as { id: string };
        if (!holdAuth) socket.send(JSON.stringify(['OK', event.id, true, '']));
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
          new URL('./harness/inbox-probe-retry.ts', import.meta.url)
        ),
        name: 'pt',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('actual compiled retry harness');
  const chunks = output.output.filter((x) => x.type === 'chunk');
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
  frames.length = 0;
  archived.clear();
  holdEvent = false;
  holdRead = false;
  challenge = false;
  holdAuth = false;
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
          window.ptCloseCalls = (window.ptCloseCalls ?? 0) + 1;
          if (window.ptFailClose)
            throw Error('actual controlled physical close failure');
          if (window.ptDelayClose)
            (window.ptHeldClose ??= []).push(() => super.close(code, reason));
          else super.close(code, reason);
        }
      };
    },
    { endpoint, origins }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(async (origins) => {
    window.ptFixture = await window.pt.makeRetryFixture();
    return window.ptFixture.host.preferenceWire(origins);
  }, origins);
  await page.evaluate(async () => {
    const f = window.ptFixture;
    await f.open();
    if ((await f.prepareOriginal()).status !== 'prepared')
      throw Error('actual original S3/R1 setup');
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => {
      window.ptRestore?.();
      window.ptRelease?.();
      window.ptDelayClose = false;
      window.ptFailClose = false;
      for (const release of window.ptHeldClose ?? []) release();
      window.ptFixture.host.settle();
      window.ptFixture.close();
    });
});
async function recover(page: Page, fresh = false) {
  expect(
    await page.evaluate(async (fresh) => {
      const f = window.ptFixture;
      if (fresh) await f.freshSession();
      return (await f.recover())?.status;
    }, fresh)
  ).toBe('recovered');
}
async function adopt(page: Page) {
  expect(
    await page.evaluate(async () => (await window.ptFixture.adopt()).status)
  ).toBe('adopted');
}
async function action(page: Page) {
  expect(
    await page.evaluate(async () => !!(await window.ptFixture.captureAction()))
  ).toBe(true);
}
const privateFrames = () =>
  frames.filter((x) => x.origin !== 'wss://discovery.example.org');
const events = () => privateFrames().filter((x) => x.frame[0] === 'EVENT');
async function nativeWire(page: Page) {
  return page.evaluate(async () => {
    const f = window.ptFixture;
    return (await f.host.stored()).find((x) => x.id === f.command())!.wire;
  });
}

test('cold original ciphertext recovery adopts readonly then observes actual same-artifact PUB READ native facts', async ({
  page
}) => {
  const original = await nativeWire(page);
  await recover(page, true);
  const before = await page.evaluate(() => window.ptFixture.host.counts());
  await adopt(page);
  await action(page);
  expect(await nativeWire(page)).toBe(original);
  expect(privateFrames()).toHaveLength(0);
  expect(await page.evaluate(() => window.ptFixture.host.counts())).toEqual(
    before
  );
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      pub = await f.publish();
    if (!pub) throw Error('actual publication missing');
    const appended = await f.append(pub),
      read = await f.read();
    if (!read) throw Error('actual readback missing');
    const saved = await f.appendRead(read);
    return {
      pub,
      appended,
      read,
      saved,
      row: f.snapshot()?.record,
      originalBinding: f.originalBinding() === undefined
    };
  });
  expect(result.pub.status).toBe('accepted');
  expect(result.read.status).toBe('readback');
  expect(result.appended.status).toBe('saved');
  expect(result.saved.status).toBe('saved');
  expect(result.originalBinding).toBe(true);
  const old = JSON.parse(original) as Record<string, unknown>;
  expect(result.row?.id).toBe(old.id);
  expect(result.row?.self).toEqual(old.self);
  expect(result.row?.rumorHash).toBe(old.rumorHash);
  expect(result.row?.createdAt).toBe(old.createdAt);
  expect(result.row?.receipts.map((x) => x.role)).toEqual([
    'access_archive',
    'access_readback'
  ]);
  expect(events()).toHaveLength(1);
  const req = privateFrames().find((x) => x.frame[0] === 'REQ')!.frame[2];
  expect(req).toEqual({
    kinds: [1059],
    ids: [result.row!.self!.eventId],
    '#p': [result.row!.owner],
    limit: 1
  });
  expect(
    await page.evaluate(() => {
      const f = window.ptFixture,
        a = f.actors();
      return !!window.pt.captureInboxProbeSealOperation(
        a.session,
        f.proof() as unknown as Parameters<
          typeof window.pt.captureInboxProbeSealOperation
        >[1],
        'reviewed_inbox_probe_seal'
      );
    })
  ).toBe(false);
});

test('repeated adoption and capture retain actual acknowledgement action and native revision', async ({
  page
}) => {
  await recover(page);
  await adopt(page);
  await action(page);
  const wire = await nativeWire(page);
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      ack = f.acknowledgement(),
      act = f.action(),
      again = await f.adopt(),
      next = await f.captureAction();
    return {
      status: again.status,
      sameAck: 'acknowledgement' in again && again.acknowledgement === ack,
      sameAction: next === act
    };
  });
  expect(result).toEqual({
    status: 'existing',
    sameAck: true,
    sameAction: true
  });
  expect(await nativeWire(page)).toBe(wire);
  expect(privateFrames()).toHaveLength(0);
});

test('actual original and recovered permissions share combined per-origin three-attempt meter', async ({
  page
}) => {
  const first = await page.evaluate(async () => {
    const f = window.ptFixture,
      result = await f.originalPublish();
    if (!result) throw Error('actual original effect');
    await f.append(result);
    return result;
  });
  expect(first.status).toBe('accepted');
  expect(first.attempt).toBe(1);
  await recover(page);
  await adopt(page);
  await action(page);
  const results = await page.evaluate(async () => {
    const f = window.ptFixture,
      read = await f.read();
    if (!read) throw Error('actual second action read');
    await f.appendRead(read);
    const pub = await f.publish();
    if (!pub) throw Error('actual third action event');
    await f.append(pub);
    const before = f.host.counts(),
      exhausted = await f.publish();
    return {
      read,
      pub,
      exhausted: exhausted === undefined,
      before,
      after: f.host.counts()
    };
  });
  expect(results.read.attempt).toBe(2);
  expect(results.pub.attempt).toBe(3);
  expect(results.read.actionId).toBe(first.actionId);
  expect(results.pub.actionId).toBe(first.actionId);
  expect(results.exhausted).toBe(true);
  expect(results.after).toEqual(results.before);
  expect(events()).toHaveLength(2);
});

test('new explicit controller recovery action replaces only ended clean action with same artifact', async ({
  page
}) => {
  await recover(page);
  await adopt(page);
  await action(page);
  const first = await page.evaluate(async () => {
    const f = window.ptFixture,
      result = await f.publish();
    if (!result) throw Error('actual first effect');
    await f.append(result);
    return result;
  });
  const source = JSON.parse(await nativeWire(page)) as Record<string, unknown>;
  expect(
    await page.evaluate(
      async () => !!(await window.ptFixture.replaceController())
    )
  ).toBe(true);
  await recover(page);
  await adopt(page);
  await action(page);
  const second = await page.evaluate(async () => {
    const f = window.ptFixture,
      result = await f.publish();
    if (!result) throw Error('actual new action effect');
    await f.append(result);
    return { result, row: f.snapshot()?.record };
  });
  expect(second.result.status).toBe('accepted');
  expect(second.result.attempt).toBe(1);
  expect(second.result.actionId).not.toBe(first.actionId);
  expect(second.row?.id).toBe(source.id);
  expect(second.row?.self).toEqual(source.self);
  expect(second.row?.receipts).toHaveLength(2);
});

for (const phase of ['adoption', 'action'] as const)
  test(`${phase} rejects exact-native physical alias before any provider or traffic`, async ({
    page
  }) => {
    await recover(page);
    if (phase === 'action') await adopt(page);
    const before = await page.evaluate(() => window.ptFixture.host.counts());
    await page.evaluate(async () => {
      const f = window.ptFixture,
        rows = await f.host.stored();
      const row = rows.find((x) => x.id === f.command())!;
      await f.host.inject([{ ...row, wire: ' ' + row.wire }]);
    });
    const refused = await page.evaluate(async (phase) => {
      const f = window.ptFixture;
      return phase === 'adoption'
        ? !('acknowledgement' in (await f.adopt()))
        : (await f.captureAction()) === undefined;
    }, phase);
    expect(refused).toBe(true);
    expect(privateFrames()).toHaveLength(0);
    expect(await page.evaluate(() => window.ptFixture.host.counts())).toEqual(
      before
    );
  });

test('handwritten historical labels grant no native adoption or remote effect without original proof', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      a = f.actors(),
      before = f.host.counts();
    const fake = { nonce_verified: true, accepted: true, current: () => true };
    const adoption = await window.pt.adoptRecoveredInboxProbe(
      f.repository,
      a.session,
      a.access,
      f.host.policy,
      f.host.qualification,
      fake as unknown as Parameters<
        typeof window.pt.adoptRecoveredInboxProbe
      >[5],
      'reviewed_original_inbox_probe_retry'
    );
    return {
      refused: !('acknowledgement' in adoption),
      before,
      after: f.host.counts()
    };
  });
  expect(result.refused).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(privateFrames()).toHaveLength(0);
});

test('actual native append of one genuine effect is idempotent without duplicate revision', async ({
  page
}) => {
  await recover(page);
  await adopt(page);
  await action(page);
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      effect = await f.publish();
    if (!effect) throw Error('actual effect missing');
    const first = await f.append(effect),
      revision = f.snapshot()?.record.revision,
      again = await f.append(effect);
    return {
      first,
      again,
      revision,
      after: f.snapshot()?.record.revision,
      receipts: f.snapshot()?.record.receipts.length
    };
  });
  expect(result.first.status).toBe('saved');
  expect(result.again.status).toBe('existing');
  expect(result.after).toBe(result.revision);
  expect(result.receipts).toBe(1);
  expect(events()).toHaveLength(1);
});

test('real pending original key job blocks recovered action before any new key or meter', async ({
  page
}) => {
  await recover(page);
  await adopt(page);
  await page.evaluate(() => {
    const f = window.ptFixture;
    f.host.mode('hold_key');
    window.ptPending = f.originalPublish();
  });
  await expect
    .poll(() => page.evaluate(() => window.ptFixture.host.waiting()))
    .toBe('key');
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      before = f.host.counts(),
      unavailable = await f.captureAction();
    return {
      unavailable: unavailable === undefined,
      before,
      after: f.host.counts()
    };
  });
  expect(result.unavailable).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(events()).toHaveLength(0);
  await page.evaluate(async () => {
    window.ptFixture.host.mode('normal');
    window.ptFixture.host.settle();
    const effect = await window.ptPending;
    if (
      !effect ||
      typeof effect !== 'object' ||
      !('status' in effect) ||
      effect.status !== 'accepted'
    )
      throw Error('actual original publication did not settle accepted');
  });
  await action(page);
  expect(events()).toHaveLength(1);
});

test('actual pending AUTH acknowledgement retains shared SDK slot and blocks new Retry action', async ({
  page
}) => {
  await recover(page);
  await adopt(page);
  challenge = true;
  holdAuth = true;
  await page.evaluate(async () => {
    const f = window.ptFixture;
    await window.pt.authenticateInboxAccess(
      f.actors().access,
      'wss://inbox.example.org',
      'reviewed_inbox_connection_auth'
    );
  });
  await expect
    .poll(() =>
      page.evaluate(() => {
        const f = window.ptFixture;
        return window.pt.inboxAccessControllerSnapshot(f.actors().access)
          ?.auth[0]?.state;
      })
    )
    .toBe('challenge');
  await page.evaluate(() => {
    const f = window.ptFixture;
    window.ptPending = window.pt.respondInboxAccessAuthentication(
      f.actors().access,
      'wss://inbox.example.org',
      'reviewed_inbox_auth_response'
    );
  });
  await expect
    .poll(() => privateFrames().filter((x) => x.frame[0] === 'AUTH').length)
    .toBe(1);
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      before = f.host.counts(),
      selected = await f.captureAction();
    return { refused: selected === undefined, before, after: f.host.counts() };
  });
  expect(result.refused).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(events()).toHaveLength(0);
  for (const socket of physical) {
    const auth = privateFrames().find((x) => x.frame[0] === 'AUTH')
      ?.frame[1] as { id: string };
    if (auth) socket.send(JSON.stringify(['OK', auth.id, true, '']));
  }
  expect(await page.evaluate(() => window.ptPending)).toBe('accepted');
  await action(page);
});

test('forged original wrap cannot retry actual retained failed private cleanup', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      a = f.actors();
    let calls = 0;
    const off = window.pt.subscribePrivateSessionClose(a.session, () => {
      calls++;
      throw Error('controlled actual retained cleanup');
    });
    window.pt.closePrivateSession(a.session);
    const before = calls;
    const refused = await window.pt.commitInboxProbe(
      f.repository,
      a.session,
      {} as Parameters<typeof window.pt.commitInboxProbe>[2],
      'reviewed_inbox_probe_commit'
    );
    const after = calls;
    off();
    return { before, after, refused: !('acknowledgement' in refused) };
  });
  expect(result.before).toBe(1);
  expect(result.after).toBe(1);
  expect(result.refused).toBe(true);
  expect(privateFrames()).toHaveLength(0);
});

for (const scope of [
  'foreign_repository',
  'same_owner_repository',
  'copied_policy',
  'copied_operator',
  'other_controller',
  'other_session',
  'stale_head',
  'original_identity',
  'original_acknowledgement',
  'unreviewed'
] as const)
  test(`genuine recovered adoption refuses ${scope} without provider native or network effects`, async ({
    page
  }) => {
    await recover(page);
    const wire = await nativeWire(page);
    const result = await page.evaluate(async (scope) => {
      const f = window.ptFixture,
        a = f.actors();
      let repository = f.repository,
        session = a.session,
        access = a.access,
        policy = f.host.policy,
        operator = f.host.qualification,
        proof = f.proof(),
        review: unknown = 'reviewed_original_inbox_probe_retry';
      const other =
        scope === 'other_controller' || scope === 'other_session'
          ? await f.alternateController(true)
          : undefined;
      const before = f.host.counts();
      if (scope === 'foreign_repository')
        repository = f.host.foreignRepository();
      if (scope === 'same_owner_repository') repository = a.repository;
      if (scope === 'copied_policy') policy = f.host.copiedPolicy();
      if (scope === 'copied_operator') {
        const copy = window.pt.validateInboxOperatorQualification(
          window.pt.inboxOperatorQualificationWire(operator, policy)!
        );
        if (!copy) throw Error('actual same-wire operator');
        operator = copy;
      }
      if (other && scope === 'other_controller') access = other.access;
      if (scope === 'other_session') {
        session = other!.session;
      }
      if (scope === 'stale_head') a.churnHead();
      if (scope === 'original_identity')
        proof = f.originalIdentity() as unknown as typeof proof;
      if (scope === 'original_acknowledgement')
        proof = f.originalAck() as unknown as typeof proof;
      if (scope === 'unreviewed') review = 'reviewed_inbox_probe_network';
      const selected = await window.pt.adoptRecoveredInboxProbe(
        repository,
        session,
        access,
        policy,
        operator,
        proof!,
        review
      );
      other?.close();
      if (session !== a.session) window.pt.closePrivateSession(session);
      return {
        refused: !('acknowledgement' in selected),
        before,
        after: f.host.counts()
      };
    }, scope);
    expect(result.refused).toBe(true);
    expect(result.after).toEqual(result.before);
    expect(await nativeWire(page)).toBe(wire);
    expect(privateFrames()).toHaveLength(0);
  });

for (const scope of [
  'foreign_repository',
  'same_owner_repository',
  'copied_policy',
  'original_acknowledgement',
  'proof',
  'unreviewed',
  'stale_head'
] as const)
  test(`recovered Retry capture refuses ${scope} before any provider or traffic`, async ({
    page
  }) => {
    await recover(page);
    await adopt(page);
    const wire = await nativeWire(page);
    const result = await page.evaluate(async (scope) => {
      const f = window.ptFixture,
        a = f.actors(),
        before = f.host.counts();
      let repository = f.repository,
        policy = f.host.policy,
        acknowledgement = f.acknowledgement(),
        review: unknown = 'reviewed_original_inbox_probe_retry';
      if (scope === 'foreign_repository')
        repository = f.host.foreignRepository();
      if (scope === 'same_owner_repository') repository = a.repository;
      if (scope === 'copied_policy') policy = f.host.copiedPolicy();
      if (scope === 'original_acknowledgement')
        acknowledgement = f.originalAck();
      if (scope === 'proof')
        acknowledgement = f.proof() as unknown as typeof acknowledgement;
      if (scope === 'unreviewed') review = 'reviewed_inbox_probe_network';
      if (scope === 'stale_head') a.churnHead();
      const selected = await window.pt.captureRecoveredInboxProbeRetryAction(
        repository,
        a.session,
        policy,
        acknowledgement!,
        review
      );
      return { refused: !selected, before, after: f.host.counts() };
    }, scope);
    expect(result.refused).toBe(true);
    expect(result.after).toEqual(result.before);
    expect(await nativeWire(page)).toBe(wire);
    expect(privateFrames()).toHaveLength(0);
  });

for (const phase of ['adoption', 'action'] as const)
  for (const mutation of [
    'r0',
    'unknown_tail',
    'foreign_tail',
    'missing',
    'corrupt'
  ] as const)
    test(`${phase} observes complete physical owner scope with ${mutation}`, async ({
      page
    }) => {
      await recover(page);
      if (phase === 'action') await adopt(page);
      const before = await page.evaluate(() => window.ptFixture.host.counts());
      await page.evaluate(async (mutation) => {
        const f = window.ptFixture,
          row = (await f.host.stored()).find((x) => x.id === f.command())!;
        if (mutation === 'missing') await f.removeFixtureRow();
        else if (mutation === 'corrupt')
          await f.host.inject([{ ...row, wire: '{}' }]);
        else if (mutation === 'r0') {
          const record = JSON.parse(row.wire) as {
            self: unknown;
            revision: number;
          };
          record.self = null;
          record.revision = 0;
          await f.host.inject([{ ...row, wire: JSON.stringify(record) }]);
        } else
          await f.host.inject([
            {
              owner: mutation === 'foreign_tail' ? 'f'.repeat(64) : row.owner,
              id: crypto.randomUUID(),
              wire: '{"unrecognized":true}'
            }
          ]);
      }, mutation);
      const raw = await page.evaluate(() => window.ptFixture.host.stored());
      const accepted = await page.evaluate(async (phase) => {
        const f = window.ptFixture;
        return phase === 'adoption'
          ? 'acknowledgement' in (await f.adopt())
          : !!(await f.captureAction());
      }, phase);
      expect(accepted).toBe(mutation === 'foreign_tail');
      expect(await page.evaluate(() => window.ptFixture.host.stored())).toEqual(
        raw
      );
      expect(await page.evaluate(() => window.ptFixture.host.counts())).toEqual(
        before
      );
      expect(privateFrames()).toHaveLength(0);
    });

for (const phase of ['adoption', 'action'] as const)
  for (const hit of phase === 'action' ? [1, 2] : [1])
    for (const fault of ['before', 'after'] as const)
      test(`${phase} refuses actual readonly${hit} ${fault} native abort`, async ({
        page
      }) => {
        await recover(page);
        if (phase === 'action') await adopt(page);
        const wire = await nativeWire(page);
        const result = await page.evaluate(
          async ({ phase, hit, fault }) => {
            const f = window.ptFixture,
              before = f.host.counts(),
              original = Object.getOwnPropertyDescriptor(
                IDBIndex.prototype,
                'openCursor'
              )!.value as IDBIndex['openCursor'];
            let selected = 0,
              aborts = 0;
            IDBIndex.prototype.openCursor = function (
              ...args: Parameters<IDBIndex['openCursor']>
            ) {
              const request = original.apply(this, args);
              if (
                this.objectStore.name === 'private_sends' &&
                this.objectStore.transaction.mode === 'readonly' &&
                ++selected === hit
              ) {
                const tx = this.objectStore.transaction;
                if (fault === 'before') {
                  aborts++;
                  tx.abort();
                } else
                  request.addEventListener(
                    'success',
                    () => {
                      aborts++;
                      tx.abort();
                    },
                    { once: true }
                  );
              }
              return request;
            };
            window.ptRestore = () => {
              IDBIndex.prototype.openCursor = original;
            };
            const refused =
              phase === 'adoption'
                ? !('acknowledgement' in (await f.adopt()))
                : !(await f.captureAction());
            window.ptRestore();
            return { refused, aborts, before, after: f.host.counts() };
          },
          { phase, hit, fault }
        );
        expect(result.refused).toBe(true);
        expect(result.aborts).toBe(1);
        expect(result.after).toEqual(result.before);
        expect(await nativeWire(page)).toBe(wire);
        expect(privateFrames()).toHaveLength(0);
      });

for (const phase of ['adoption', 'action'] as const)
  test(`${phase} actual owner lock contention is inert and permits later explicit admission`, async ({
    page
  }) => {
    await recover(page);
    if (phase === 'action') await adopt(page);
    await page.evaluate(() => {
      const f = window.ptFixture;
      window.ptPending = navigator.locks.request(
        'harvestcircle:owner:' + f.host.owner,
        async () => {
          await new Promise<void>((resolve) => {
            window.ptRelease = resolve;
          });
        }
      );
    });
    await expect.poll(() => page.evaluate(() => !!window.ptRelease)).toBe(true);
    const result = await page.evaluate(async (phase) => {
      const f = window.ptFixture,
        before = f.host.counts();
      const refused =
        phase === 'adoption'
          ? !('acknowledgement' in (await f.adopt()))
          : !(await f.captureAction());
      return { refused, before, after: f.host.counts() };
    }, phase);
    expect(result.refused).toBe(true);
    expect(result.after).toEqual(result.before);
    expect(privateFrames()).toHaveLength(0);
    await page.evaluate(async () => {
      window.ptRelease!();
      await window.ptPending;
    });
    if (phase === 'adoption') await adopt(page);
    else await action(page);
  });

test('concurrent readonly adoption and Retry capture return only one actual token without regeneration', async ({
  page
}) => {
  await recover(page);
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      before = f.host.counts();
    const first = f.adopt(),
      second = f.adopt();
    const [one, two] = await Promise.all([first, second]);
    if (!('acknowledgement' in one)) throw Error('actual first adoption');
    const [action, reentry] = await Promise.all([
      f.captureAction(),
      f.captureAction()
    ]);
    return {
      first: one.status,
      second: two.status,
      action: !!action,
      reentry: !reentry,
      before,
      after: f.host.counts()
    };
  });
  expect(result.first).toBe('adopted');
  expect(result.second).toBe('busy');
  expect(result.action).toBe(true);
  expect(result.reentry).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(privateFrames()).toHaveLength(0);
});

for (const callback of ['reentry', 'native_mutation', 'stale_head'] as const)
  test(`actual action UUID ${callback} callback cannot mint competing or stale Retry authority`, async ({
    page
  }) => {
    await recover(page);
    await adopt(page);
    const result = await page.evaluate(async (callback) => {
      const f = window.ptFixture,
        before = f.host.counts(),
        original = crypto.randomUUID.bind(crypto);
      let callbacks = 0;
      let other: Promise<unknown> | undefined;
      const row = (await f.host.stored()).find((x) => x.id === f.command())!;
      crypto.randomUUID = () => {
        callbacks++;
        if (callback === 'reentry') other = f.captureAction();
        else if (callback === 'native_mutation')
          other = f.host.inject([{ ...row, wire: ' ' + row.wire }]);
        else f.actors().churnHead();
        return original();
      };
      window.ptRestore = () => {
        crypto.randomUUID = original;
      };
      const selected = await f.captureAction();
      window.ptRestore();
      const reentered = await other;
      return {
        callbacks,
        selected: !!selected,
        reentered: !!reentered,
        before,
        after: f.host.counts()
      };
    }, callback);
    expect(result.callbacks).toBe(1);
    expect(result.selected).toBe(callback === 'reentry');
    expect(result.reentered).toBe(false);
    expect(result.after).toEqual(result.before);
    expect(privateFrames()).toHaveLength(0);
  });

for (const pending of ['EVENT', 'REQ'] as const)
  test(`actual pending ${pending} blocks new Retry action and pure pool observation until original settles`, async ({
    page
  }) => {
    await recover(page);
    await adopt(page);
    if (pending === 'EVENT') {
      holdEvent = true;
      await page.evaluate(() => {
        window.ptPending = window.ptFixture.originalPublish();
      });
    } else {
      expect(
        await page.evaluate(
          async () => (await window.ptFixture.originalPublish())?.status
        )
      ).toBe('accepted');
      holdRead = true;
      await page.evaluate(() => {
        window.ptPending = window.ptFixture.originalRead();
      });
    }
    await expect
      .poll(() => privateFrames().filter((x) => x.frame[0] === pending).length)
      .toBe(1);
    const result = await page.evaluate(async () => {
      const f = window.ptFixture,
        before = f.host.counts(),
        idle = f.idle(),
        selected = await f.captureAction();
      return { idle, refused: !selected, before, after: f.host.counts() };
    });
    expect(result.idle).toBe(false);
    expect(result.refused).toBe(true);
    expect(result.after).toEqual(result.before);
    if (pending === 'EVENT') {
      const id = (events()[0].frame[1] as { id: string }).id;
      for (const socket of physical)
        socket.send(JSON.stringify(['OK', id, true, '']));
    } else {
      const frame = privateFrames().find((x) => x.frame[0] === 'REQ')!;
      const id = (frame.frame[2] as { ids: string[] }).ids[0];
      for (const socket of physical) {
        socket.send(
          JSON.stringify([
            'EVENT',
            frame.frame[1],
            archived.get(JSON.stringify([frame.origin, id]))
          ])
        );
        socket.send(JSON.stringify(['EOSE', frame.frame[1]]));
      }
    }
    expect(
      ((await page.evaluate(() => window.ptPending)) as { status: string })
        .status
    ).toBe(pending === 'EVENT' ? 'accepted' : 'readback');
    await action(page);
  });

test('genuine same-scope controller recapture retains the original controller and action meter', async ({
  page
}) => {
  const first = await page.evaluate(() => window.ptFixture.originalPublish());
  expect(first?.status).toBe('accepted');
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      a = f.actors(),
      other = await f.alternateController();
    const reader = window.pt.captureInboxProbeRecoveryReader(
      f.repository,
      a.identity,
      a.session,
      f.host.policy,
      other.access,
      f.host.qualification,
      f.command(),
      'reviewed_inbox_probe_recovery'
    );
    if (!reader) throw Error('actual alternate recovery reader');
    const proof = await window.pt.recoverInboxProbe(reader);
    if (!('proof' in proof)) throw Error('actual alternate proof');
    const adopted = await window.pt.adoptRecoveredInboxProbe(
      f.repository,
      a.session,
      other.access,
      f.host.policy,
      f.host.qualification,
      proof.proof,
      'reviewed_original_inbox_probe_retry'
    );
    if (!('acknowledgement' in adopted))
      throw Error('actual alternate readonly adoption');
    const before = f.host.counts(),
      selected = await window.pt.captureRecoveredInboxProbeRetryAction(
        f.repository,
        a.session,
        f.host.policy,
        adopted.acknowledgement,
        'reviewed_original_inbox_probe_retry'
      );
    const permission =
      selected &&
      window.pt.captureInboxProbePublication(
        f.repository,
        a.session,
        f.host.policy,
        adopted.acknowledgement,
        selected,
        'wss://inbox.example.org',
        'reviewed_inbox_probe_publication'
      );
    const effect =
      permission &&
      (await window.pt.publishInboxProbeArchiveAttempt(
        f.pool()!,
        permission,
        new AbortController().signal
      ));
    window.pt.stopInboxProbeRecoveryReader(reader);
    other.close();
    return {
      sameController: other.access === a.access,
      effect,
      providerCalls: f.host.counts().keys - before.keys
    };
  });
  expect(result.sameController).toBe(true);
  expect(result.effect).toMatchObject({
    status: 'accepted',
    attempt: 2,
    actionId: first!.actionId
  });
  expect(result.providerCalls).toBe(1);
  expect(events()).toHaveLength(2);
});

test('same action has independent bounded three-attempt counters across exactly three frozen origins', async ({
  page
}) => {
  await recover(page);
  await adopt(page);
  await action(page);
  const result = await page.evaluate(async (origins) => {
    const f = window.ptFixture,
      effects = [];
    for (const origin of origins) {
      for (let i = 0; i < 3; i++) {
        const effect = await f.publish(origin);
        if (!effect) throw Error('actual permitted original artifact attempt');
        await f.append(effect);
        effects.push({
          status: effect.status,
          attempt: effect.attempt,
          actionId: effect.actionId
        });
      }
      if (await f.read(origin))
        throw Error('exhausted combined archive/read origin admitted');
    }
    const before = f.host.counts(),
      foreign = await f.publish('wss://foreign.example.org');
    return { effects, foreign: !foreign, before, after: f.host.counts() };
  }, origins);
  expect(result.effects.map((x) => x.attempt)).toEqual([
    1, 2, 3, 1, 2, 3, 1, 2, 3
  ]);
  expect(new Set(result.effects.map((x) => x.actionId)).size).toBe(1);
  expect(result.effects.every((x) => x.status === 'accepted')).toBe(true);
  expect(result.foreign).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(events()).toHaveLength(9);
  expect(physical.size).toBeLessThanOrEqual(4); // Three private plus public discovery.
});

test('100 genuine command slots remain protected at source registry capacity without fixture deletion granting authority', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      a = f.actors(),
      saved = [];
    for (let i = 0; i < 100; i++) {
      if (i) {
        if (!(await f.replaceController()))
          throw Error('actual next source controller');
        if ((await f.prepareOriginal()).status !== 'prepared')
          throw Error('actual next original S3/R1');
      }
      const action = window.pt.captureInboxProbePublicationAction(
        f.repository,
        a.session,
        f.host.policy,
        f.originalAck()!,
        'reviewed_inbox_probe_network'
      );
      if (!action) throw Error('actual bounded command slot ' + i);
      saved.push(action);
      await f.removeFixtureRow();
    }
    if (!(await f.replaceController()))
      throw Error('actual capacity controller');
    if ((await f.prepareOriginal()).status !== 'prepared')
      throw Error('actual capacity original');
    const row = (await f.host.stored()).find((x) => x.id === f.command())!;
    const before = f.host.counts();
    const refused = !window.pt.captureInboxProbePublicationAction(
      f.repository,
      a.session,
      f.host.policy,
      f.originalAck()!,
      'reviewed_inbox_probe_network'
    );
    if ((await f.recover())?.status !== 'recovered')
      throw Error('actual capacity recovery');
    if (!('acknowledgement' in (await f.adopt())))
      throw Error('actual capacity readonly adoption');
    const afterRecovery = f.host.counts(),
      retryRefused = !(await f.captureAction());
    return {
      slots: saved.length,
      refused,
      retryRefused,
      before,
      afterRecovery,
      after: f.host.counts(),
      wire: row.wire,
      current: (await f.host.stored()).find((x) => x.id === f.command())!.wire
    };
  });
  expect(result.slots).toBe(100);
  expect(result.refused).toBe(true);
  expect(result.retryRefused).toBe(true);
  expect(result.after).toEqual(result.afterRecovery);
  expect(result.current).toBe(result.wire);
  expect(privateFrames()).toHaveLength(0);
});

for (const lane of ['page', 'live'] as const)
  test(`actual foreground ${lane} owner blocks new Retry without disturbing its subscription`, async ({
    page
  }) => {
    await recover(page);
    await adopt(page);
    holdRead = true;
    await page.evaluate((lane) => {
      const f = window.ptFixture,
        a = f.actors();
      const pool = window.pt.getPrivatePool(
        a.session,
        f.host.policy,
        ['wss://inbox.example.org'],
        Object.freeze({}),
        'receive'
      );
      if (!pool) throw Error('actual foreground pool scope');
      const stop =
        lane === 'page'
          ? window.pt.subscribePrivatePage(pool, 1, () => {})
          : window.pt.subscribePrivateLive(pool, () => {});
      window.ptRelease = () => {
        stop();
      };
    }, lane);
    await expect
      .poll(() => privateFrames().filter((x) => x.frame[0] === 'REQ').length)
      .toBe(1);
    const framesBefore = privateFrames().slice();
    const result = await page.evaluate(async () => {
      const f = window.ptFixture,
        before = f.host.counts(),
        idle = f.idle(),
        selected = await f.captureAction();
      return { idle, refused: !selected, before, after: f.host.counts() };
    });
    expect(result.idle).toBe(false);
    expect(result.refused).toBe(true);
    expect(result.after).toEqual(result.before);
    expect(privateFrames()).toEqual(framesBefore);
    await page.evaluate(() => window.ptRelease!());
    await expect
      .poll(() => page.evaluate(() => window.ptFixture.idle()))
      .toBe(true);
    await action(page);
  });

test('actual B1 B2 predecessor facts survive fresh-session recovery and never skip new named effects', async ({
  page
}) => {
  const prior = await page.evaluate(async () => {
    const f = window.ptFixture,
      pub = await f.originalPublish();
    if (!pub) throw Error('actual old B1');
    await f.append(pub);
    const read = await f.originalRead();
    if (!read) throw Error('actual old B2');
    await f.appendRead(read);
    return { pub, read };
  });
  const predecessor = await nativeWire(page);
  await recover(page, true);
  await adopt(page);
  await action(page);
  expect(await nativeWire(page)).toBe(predecessor);
  expect(events()).toHaveLength(1);
  const fresh = await page.evaluate(async () => {
    const f = window.ptFixture,
      pub = await f.publish();
    if (!pub) throw Error('actual fresh B1');
    await f.append(pub);
    const read = await f.read();
    if (!read) throw Error('actual fresh B2');
    await f.appendRead(read);
    return {
      pub,
      read,
      row: f.snapshot()?.record,
      readiness: window.pt.inboxAccessControllerSnapshot(f.actors().access)
        ?.readiness
    };
  });
  expect(fresh.pub.status).toBe('accepted');
  expect(fresh.read.status).toBe('readback');
  expect(fresh.pub.actionId).not.toBe(prior.pub.actionId);
  expect(fresh.row?.receipts.slice(0, 2)).toEqual(
    (JSON.parse(predecessor) as { receipts: unknown[] }).receipts
  );
  expect(fresh.row?.receipts).toHaveLength(4);
  expect(fresh.readiness).toBe('unavailable');
  expect(events()).toHaveLength(2);
});

test('historical unknown labels remain unknown through adoption and require a new genuine observed effect', async ({
  page
}) => {
  await page.evaluate(async () => {
    const f = window.ptFixture,
      effect = await f.originalPublish();
    if (!effect) throw Error('actual predecessor fixture');
    await f.append(effect);
    const row = (await f.host.stored()).find((x) => x.id === f.command())!,
      record = JSON.parse(row.wire) as { receipts: { status: string }[] };
    record.receipts[0].status = 'unknown';
    await f.host.inject([{ ...row, wire: JSON.stringify(record) }]);
  });
  const wire = await nativeWire(page);
  await recover(page, true);
  await adopt(page);
  await action(page);
  expect(await nativeWire(page)).toBe(wire);
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      effect = await f.publish();
    if (!effect) throw Error('actual fresh named effect');
    await f.append(effect);
    return { effect, row: f.snapshot()?.record };
  });
  expect(result.effect.status).toBe('accepted');
  expect(result.row?.receipts.map((x) => x.status)).toEqual([
    'unknown',
    'accepted'
  ]);
  expect(events()).toHaveLength(2);
});

test('genuine original C1 nonce capability is isolated from recovered Retry and recovered readback cannot issue C1 proof', async ({
  page
}) => {
  await page.evaluate(async () => {
    const f = window.ptFixture,
      a = f.actors();
    if (!(await f.originalPublish()))
      throw Error('actual original publication');
    const read = await f.originalRead();
    if (!read) throw Error('actual original read');
    const reader = window.pt.captureInboxProbeNonceReader(
      f.repository,
      a.identity,
      a.session,
      f.host.policy,
      read,
      'reviewed_inbox_probe_nonce_decryption'
    );
    if (!reader) throw Error('actual original C1 reader');
    const proof = await window.pt.readInboxProbeNonce(reader);
    if (!('proof' in proof)) throw Error('actual original C1 proof');
    const before = f.host.counts();
    const selected = await window.pt.adoptRecoveredInboxProbe(
      f.repository,
      a.session,
      a.access,
      f.host.policy,
      f.host.qualification,
      proof.proof as unknown as Parameters<
        typeof window.pt.adoptRecoveredInboxProbe
      >[5],
      'reviewed_original_inbox_probe_retry'
    );
    if (
      'acknowledgement' in selected ||
      JSON.stringify(before) !== JSON.stringify(f.host.counts())
    )
      throw Error('C1 crossed recovered Retry authority');
    window.pt.stopInboxProbeNonceReader(reader);
  });
  await recover(page);
  await adopt(page);
  await action(page);
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      a = f.actors(),
      read = await f.read();
    if (!read) throw Error('actual recovered readback');
    const before = f.host.counts();
    const reader = window.pt.captureInboxProbeNonceReader(
      f.repository,
      a.identity,
      a.session,
      f.host.policy,
      read,
      'reviewed_inbox_probe_nonce_decryption'
    );
    return { refused: !reader, before, after: f.host.counts(), read };
  });
  expect(result.refused).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(result.read.status).toBe('readback');
  expect(result.read.attempt).toBe(3);
});

test('actual measured cumulative 45-second network allowance stays exhausted across original recovery', async ({
  page
}) => {
  await page.evaluate(() => {
    const f = window.ptFixture;
    window.ptClock = f.installNetworkClock();
    window.ptRestore = () => window.ptClock!.restore();
  });
  holdEvent = true;
  await page.evaluate(() => {
    window.ptPending = window.ptFixture.originalPublish();
  });
  await expect.poll(() => events().length).toBe(1);
  await page.evaluate(() => window.ptClock!.advance(30000));
  const id = (events()[0].frame[1] as { id: string }).id;
  for (const socket of physical)
    socket.send(JSON.stringify(['OK', id, true, '']));
  expect(
    ((await page.evaluate(() => window.ptPending)) as { status: string }).status
  ).toBe('accepted');
  await recover(page);
  await adopt(page);
  await action(page);
  holdRead = true;
  await page.evaluate(() => {
    window.ptPending = window.ptFixture.read();
  });
  await expect
    .poll(() => privateFrames().filter((x) => x.frame[0] === 'REQ').length)
    .toBe(1);
  await page.evaluate(() => window.ptClock!.advance(15001));
  const timed = (await page.evaluate(() => window.ptPending)) as {
    status: string;
    attempt: number;
  };
  expect(timed).toMatchObject({ status: 'timed_out', attempt: 2 });
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      before = f.host.counts(),
      refused = !(await f.publish());
    return {
      refused,
      before,
      after: f.host.counts(),
      observations: window.ptClock!.observations()
    };
  });
  expect(result.refused).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(result.observations).toBeGreaterThan(0);
  expect(events()).toHaveLength(1);
});

for (const fault of ['held', 'failed'] as const)
  test(`actual original ${fault} physical cleanup blocks recovered authority without retrying cleanup implicitly`, async ({
    page
  }) => {
    await recover(page);
    await adopt(page);
    holdEvent = true;
    await page.evaluate(() => {
      window.ptAbort = new AbortController();
      window.ptPending = window.ptFixture.originalPublish(
        undefined,
        window.ptAbort.signal
      );
    });
    await expect.poll(() => events().length).toBe(1);
    const result = await page.evaluate(async (fault) => {
      const f = window.ptFixture,
        acknowledgement = f.acknowledgement()!;
      window.ptDelayClose = fault === 'held';
      window.ptFailClose = fault === 'failed';
      window.ptAbort!.abort();
      let failed = false;
      try {
        f.stop();
      } catch {
        failed = true;
      }
      const effect = await window.ptPending,
        before = f.host.counts(),
        held = window.ptHeldClose?.length ?? 0;
      const idle = f.idle(),
        selected = await window.pt.captureRecoveredInboxProbeRetryAction(
          f.repository,
          f.actors().session,
          f.host.policy,
          acknowledgement,
          'reviewed_original_inbox_probe_retry'
        );
      return {
        effect,
        failed,
        idle,
        refused: !selected,
        held,
        afterHeld: window.ptHeldClose?.length ?? 0,
        before,
        after: f.host.counts()
      };
    }, fault);
    expect(result.effect).toMatchObject({ status: 'stopped', attempt: 1 });
    expect(result.idle).toBe(false);
    expect(result.refused).toBe(true);
    expect(result.afterHeld).toBe(result.held);
    expect(result.after).toEqual(result.before);
    if (fault === 'failed') expect(result.failed).toBe(true);
    else expect(result.held).toBeGreaterThan(0);
    const observed = await page.evaluate(async () => {
      const before = window.ptCloseCalls ?? 0,
        replacement = await window.ptFixture.replacementWhilePending();
      return { replacement, before, after: window.ptCloseCalls ?? 0 };
    });
    const replacement = observed.replacement;
    console.info(
      JSON.stringify({
        fixture: 'C2B_ACTUAL_FRESH_REPLACEMENT_PHYSICAL_GATE',
        fault,
        gate: replacement.gate,
        genuineCurrentBinding:
          replacement.gate === 'actual_retry_admission'
            ? replacement.genuineCurrentBinding
            : null,
        uuidCalls:
          replacement.gate === 'actual_retry_admission'
            ? replacement.uuids
            : null
      })
    );
    expect(replacement.gate).toBe('actual_retry_admission');
    if (replacement.gate !== 'actual_retry_admission')
      throw Error('genuine fresh Retry must reach the original physical gate');
    expect(replacement.genuineCurrentBinding).toBe(true);
    expect(replacement.refused).toBe(true);
    expect(replacement.uuids).toBe(0);
    expect(replacement.after).toEqual(replacement.before);
    expect(replacement.closesAfter).toBe(replacement.closesBefore);
    expect(events()).toHaveLength(1);
    await page.evaluate(() => {
      window.ptDelayClose = false;
      window.ptFailClose = false;
      for (const release of window.ptHeldClose ?? []) release();
      // A real failed close has no held callback to release: explicitly retry
      // its ORIGINAL Stop before waiting for the resulting native close.
      try {
        window.ptFixture.stop();
      } catch (error) {
        if (
          !(error instanceof Error) ||
          !['private_pool_close_pending', 'private_pool_close_failed'].includes(
            error.message
          )
        )
          throw error;
      }
    });
    await expect.poll(() => events().length).toBe(1);
    await expect.poll(() => physical.size).toBeLessThanOrEqual(1);
    await page.evaluate(() => window.ptFixture.stop());
    holdEvent = false;
    expect(
      await page.evaluate(
        async () => !!(await window.ptFixture.replaceController())
      )
    ).toBe(true);
    await recover(page);
    await adopt(page);
    await action(page);
    const next = await page.evaluate(() => window.ptFixture.publish());
    expect(next).toMatchObject({ status: 'accepted', attempt: 1 });
    expect(next!.actionId).not.toBe(
      (result.effect as { actionId: string }).actionId
    );
    expect(events()).toHaveLength(2);
    expect(events()[1].frame[1]).toEqual(events()[0].frame[1]);
  });

test('actual failed original SDK teardown stays protected without implicit Retry cleanup', async ({
  page
}) => {
  await recover(page);
  await adopt(page);
  holdEvent = true;
  await page.evaluate(() => {
    const f = window.ptFixture,
      fault = f.installAttemptTeardownFailure();
    window.ptRestore = fault.restore;
    window.ptAbort = new AbortController();
    window.ptPending = f.originalPublish(undefined, window.ptAbort.signal);
  });
  await expect.poll(() => events().length).toBe(1);
  const result = await page.evaluate(async () => {
    const f = window.ptFixture;
    window.ptAbort!.abort();
    const effect = await window.ptPending;
    const before = f.host.counts(),
      idle = f.idle(),
      selected = await f.captureAction();
    return { effect, idle, refused: !selected, before, after: f.host.counts() };
  });
  expect(result.effect).toMatchObject({ status: 'unknown', attempt: 1 });
  expect(result.idle).toBe(false);
  expect(result.refused).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(events()).toHaveLength(1);
  await page.evaluate(() => {
    window.ptRestore!();
    window.ptFixture.stop();
  });
});

for (const fault of ['aborted', 'uncertain', 'changed_successor'] as const)
  test(`actual recovered native append ${fault} preserves strict original successor evidence`, async ({
    page
  }) => {
    await recover(page);
    await adopt(page);
    await action(page);
    const result = await page.evaluate(async (fault) => {
      const f = window.ptFixture,
        effect = await f.publish();
      if (!effect) throw Error('actual recovered SDK effect');
      const before = await f.host.stored(),
        control = f.installCommitFault(fault);
      window.ptRestore = control.restore;
      const first = await f.append(effect);
      control.restore();
      const afterFirst = await f.host.stored(),
        second = await f.append(effect);
      return { first, second, before, afterFirst, native: control.counts() };
    }, fault);
    if (fault === 'aborted') {
      expect(result.first.status).toBe('aborted');
      expect(result.afterFirst).toEqual(result.before);
      expect(result.second.status).toBe('saved');
      expect(result.native).toMatchObject({
        putSuccess: 1,
        complete: 0,
        aborted: 1
      });
    } else {
      expect(result.native).toMatchObject({
        putSuccess: 1,
        complete: 1,
        aborted: 0
      });
      expect(result.native.abortRefused).toBeGreaterThan(0);
      if (fault === 'uncertain') {
        expect(result.first.status).toBe('reconciled');
        expect(result.second.status).toBe('existing');
      } else {
        expect(result.native.changed).toBe(1);
        expect(['conflict', 'unknown_completion']).toContain(
          result.first.status
        );
        expect(result.second.status).not.toBe('saved');
      }
    }
    expect(events()).toHaveLength(1);
  });

test('actual recovered effect bookkeeping remains eligible after controller loss without restoring authority', async ({
  page
}) => {
  await recover(page);
  await adopt(page);
  await action(page);
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      effect = await f.publish();
    if (!effect) throw Error('actual recovered accepted effect');
    window.pt.closeInboxAccessController(f.actors().access);
    const before = f.host.counts(),
      appended = await f.append(effect),
      refused = !(await f.captureAction());
    const row = (await f.host.stored()).find((x) => x.id === f.command())!;
    return {
      appended,
      refused,
      before,
      after: f.host.counts(),
      row: JSON.parse(row.wire) as { receipts: { status: string }[] }
    };
  });
  expect(result.appended.status).toBe('saved');
  expect(result.refused).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(result.row.receipts[0].status).toBe('accepted');
  expect(events()).toHaveLength(1);
});

test('genuine ordinary recovery capability cannot supply original-probe Retry provenance', async ({
  page
}) => {
  await recover(page);
  const result = await page.evaluate(async () => {
    const f = window.ptFixture,
      a = f.actors(),
      provider = Object.getOwnPropertyDescriptor(window, 'nostr')!;
    const ordinary = await window.pt.makeOrdinaryRecoveryFixture();
    const token = ordinary.capture();
    Object.defineProperty(window, 'nostr', provider);
    const before = f.host.counts(),
      raw = await f.host.stored();
    try {
      const selected = await window.pt.adoptRecoveredInboxProbe(
        f.repository,
        a.session,
        a.access,
        f.host.policy,
        f.host.qualification,
        token as unknown as Parameters<
          typeof window.pt.adoptRecoveredInboxProbe
        >[5],
        'reviewed_original_inbox_probe_retry'
      );
      const action = await window.pt.captureRecoveredInboxProbeRetryAction(
        f.repository,
        a.session,
        f.host.policy,
        token as unknown as Parameters<
          typeof window.pt.captureRecoveredInboxProbeRetryAction
        >[3],
        'reviewed_original_inbox_probe_retry'
      );
      return {
        refused: !('acknowledgement' in selected) && !action,
        before,
        after: f.host.counts(),
        unchanged: JSON.stringify(raw) === JSON.stringify(await f.host.stored())
      };
    } finally {
      ordinary.close();
      Object.defineProperty(window, 'nostr', provider);
    }
  });
  expect(result.refused).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(result.unchanged).toBe(true);
  expect(privateFrames()).toHaveLength(0);
});
