import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-access-check.ts';
import type { PrivateAccessProbe } from '../../src/lib/persistence/private-records.ts';
function probeRow(wire: string) {
  return JSON.parse(wire) as PrivateAccessProbe;
}
declare global {
  interface Window {
    hcCheck: typeof Fixture;
    hcCheckFixture: Awaited<ReturnType<typeof Fixture.makeCheckFixture>>;
    hcCheckPending: ReturnType<typeof Fixture.runInboxAccessCheck>;
    hcCheckCloseCalls: number;
    hcHostedCheck: Awaited<ReturnType<typeof Fixture.openHostedCheck>>;
    hcCheckRelease?: () => Promise<void>;
    hcCheckLockReady?: Promise<void>;
    hcCheckAuthPending?: Promise<string>;
    hcCheckRestore?: () => void;
    hcCheckOther?: Awaited<
      ReturnType<Window['hcCheckFixture']['alternateActors']>
    >;
    hcCheckCloseMode?: 'hold' | 'fail';
    hcCheckHeldClose?: (() => void)[];
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string,
  head = '';
let holdRead = false;
let wireFault:
  'normal' | 'refused' | 'wrong_ack' | 'disconnect' | 'missing' | 'wrong_read' =
  'normal';
let holdArchive = false;
let releaseArchive: (() => void) | undefined;
let challenge = false;
let authMode: 'accept' | 'refuse' | 'hold' = 'accept';
let releaseAuth: (() => void) | undefined;
const physical = new Set<WebSocket>(),
  physicalOrigins = new WeakMap<WebSocket, string>(),
  archived = new Map<string, unknown>();
const frames: { origin: string; type: string; id?: string }[] = [];
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
    socket.on('close', () => physical.delete(socket));
    const origin = new URL(request.url!, 'http://127.0.0.1').searchParams.get(
      'origin'
    )!;
    physicalOrigins.set(socket, origin);
    if (challenge && origin === 'wss://inbox.example.org')
      socket.send(JSON.stringify(['AUTH', 's5c-actual-source-challenge']));
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
      if (frames.length >= 10000)
        throw Error('bounded source fixture frame log');
      if (frame[0] === 'REQ') {
        const filter = frame[2] as { kinds?: number[]; ids?: string[] };
        frames.push({ origin, type: 'REQ', id: filter.ids?.[0] });
        if (filter.kinds?.includes(10050) && head)
          socket.send(JSON.stringify(['EVENT', frame[1], JSON.parse(head)]));
        if (filter.kinds?.includes(1059) && filter.ids) {
          if (holdRead) return;
          const event = archived.get(JSON.stringify([origin, filter.ids[0]]));
          if (event && wireFault !== 'missing')
            socket.send(
              JSON.stringify([
                'EVENT',
                frame[1],
                wireFault === 'wrong_read'
                  ? { ...event, id: 'f'.repeat(64) }
                  : event
              ])
            );
        }
        if (filter.kinds?.includes(1059) && !filter.ids) {
          for (const [key, event] of archived) {
            if ((JSON.parse(key) as [string, string])[0] === origin)
              socket.send(JSON.stringify(['EVENT', frame[1], event]));
          }
        }
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      } else if (frame[0] === 'AUTH') {
        const event = frame[1] as { id: string };
        frames.push({ origin, type: 'AUTH', id: event.id });
        const acknowledge = () =>
          socket.send(
            JSON.stringify([
              'OK',
              event.id,
              authMode !== 'refuse',
              'controlled actual auth response'
            ])
          );
        if (authMode === 'hold') releaseAuth = acknowledge;
        else acknowledge();
      } else if (frame[0] === 'EVENT') {
        const event = frame[1] as { id: string };
        frames.push({ origin, type: 'EVENT', id: event.id });
        archived.set(JSON.stringify([origin, event.id]), event);
        const acknowledge = () => {
          if (wireFault === 'disconnect') {
            socket.close();
            return;
          }
          socket.send(
            JSON.stringify([
              'OK',
              wireFault === 'wrong_ack' ? 'f'.repeat(64) : event.id,
              wireFault !== 'refused',
              'controlled actual named acceptance'
            ])
          );
          if (wireFault === 'wrong_ack') socket.close();
        };
        if (holdArchive) releaseArchive = acknowledge;
        else acknowledge();
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
          new URL('./harness/inbox-access-check.ts', import.meta.url)
        ),
        name: 'hcCheck',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('actual compiled check harness');
  const chunks = output.output.filter((item) => item.type === 'chunk');
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
  head = '';
  archived.clear();
  frames.length = 0;
  holdRead = false;
  wireFault = 'normal';
  holdArchive = false;
  releaseArchive = undefined;
  challenge = false;
  authMode = 'accept';
  releaseAuth = undefined;
  await page.addInitScript(
    ({ endpoint, origins }) => {
      const Native = window.WebSocket;
      window.hcCheckCloseCalls = 0;
      const close = Object.getOwnPropertyDescriptor(Native.prototype, 'close')!
        .value as WebSocket['close'];
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const origin = new URL(String(url)).origin;
          if (!['wss://discovery.example.org', ...origins].includes(origin))
            throw Error('unapproved fixture origin');
          super(endpoint + '/?origin=' + encodeURIComponent(origin), protocols);
        }
        close(...args: Parameters<WebSocket['close']>) {
          window.hcCheckCloseCalls++;
          if (window.hcCheckCloseMode === 'fail')
            throw Error('actual controlled physical close refusal');
          if (window.hcCheckCloseMode === 'hold') {
            (window.hcCheckHeldClose ??= []).push(() =>
              close.apply(this, args)
            );
            return;
          }
          return close.apply(this, args);
        }
      };
    },
    { endpoint, origins }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(
    async ({ origins, authRequired }) => {
      window.hcCheckFixture =
        await window.hcCheck.makeCheckFixture(authRequired);
      return window.hcCheckFixture.host.preferenceWire(origins);
    },
    { origins, authRequired: testInfo.title.includes('required AUTH') }
  );
  await page.evaluate(() => window.hcCheckFixture.open());
});
test.afterEach(async ({ page }) => {
  if (page.isClosed()) return;
  const closed = await page.evaluate(async () => {
    await window.hcCheckRelease?.();
    window.hcCheckRelease = undefined;
    window.hcCheckRestore?.();
    window.hcCheckRestore = undefined;
    window.hcCheckCloseMode = undefined;
    for (const release of window.hcCheckHeldClose ?? []) release();
    window.hcCheckHeldClose = [];
    try {
      window.hcCheckFixture.close();
      return true;
    } catch (error) {
      if (
        error instanceof Error &&
        [
          'inbox_access_check_cleanup_required',
          'private_inbox_cleanup_required',
          'private_history_cleanup_required',
          'private_auth_cleanup_required',
          'private_pool_close_pending',
          'private_pool_close_failed'
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
          [...physical].filter(
            (socket) =>
              physicalOrigins.get(socket) !== 'wss://discovery.example.org'
          ).length
      )
      .toBe(0);
    await page.evaluate(() => window.hcCheckFixture.close());
  }
});
test('cold original check shows source disclosure with no preparation or provider work before review', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture,
      before = f.host.counts(),
      rowsBefore = await f.host.stored();
    const disclosure = f.select();
    const invalid = await f.run('not_reviewed');
    return {
      before,
      after: f.host.counts(),
      rowsBefore: rowsBefore.length,
      rowsAfter: (await f.host.stored()).length,
      disclosure,
      invalid: invalid.status,
      state: f.snapshot()
    };
  });
  expect(result.rowsBefore).toBe(0);
  expect(result.rowsAfter).toBe(0);
  expect(result.after).toEqual(result.before);
  expect(result.invalid).toBe('invalid');
  expect(JSON.stringify(result.disclosure)).toMatch(/note to self|self.note/i);
  expect(JSON.stringify(result.disclosure)).toMatch(
    /remote retention|relay.*retain/i
  );
  expect(JSON.stringify(result.disclosure)).toMatch(/receive|decrypt/i);
  expect(result.state?.sourceOnly).toBe(true);
  expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(0);
});
test('cold original coordinator performs actual journal archive read nonce and source qualification', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture;
    f.select();
    const before = f.host.counts(),
      result = await f.run();
    return {
      status: result.status,
      before,
      after: f.host.counts(),
      snapshot: f.snapshot(),
      rows: await f.host.stored(),
      qualification:
        f.qualification() &&
        window.hcCheck.inboxAccessQualificationSnapshot(f.qualification()!)
    };
  });
  expect(result.status).toBe('qualified');
  expect(result.rows).toHaveLength(1);
  const row = JSON.parse(result.rows[0].wire) as {
    family: string;
    self: { eventId: string };
    receipts: { role: string; status: string }[];
  };
  expect(row.family).toBe('private_access_probe');
  expect(
    row.receipts.filter(
      (fact) => fact.role === 'access_archive' && fact.status === 'accepted'
    )
  ).toHaveLength(3);
  expect(row.receipts.some((fact) => fact.status === 'nonce_verified')).toBe(
    true
  );
  expect(
    frames.filter((frame) => frame.type === 'EVENT').map((frame) => frame.id)
  ).toEqual([row.self.eventId, row.self.eventId, row.self.eventId]);
  expect(
    new Set(
      frames
        .filter((frame) => frame.type === 'EVENT')
        .map((frame) => frame.origin)
    ).size
  ).toBe(3);
  expect(result.qualification?.sourceOnly).toBe(true);
  expect(result.after.signs - result.before.signs).toBe(1);
});
test('copied review never authorizes work and a consumed review cannot replay', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture;
    f.select();
    const before = f.host.counts(),
      copied = await window.hcCheck.runInboxAccessCheck(
        { ...f.review()! },
        'reviewed_inbox_access_check'
      );
    const afterCopy = f.host.counts(),
      actual = await f.run(),
      replay = await f.run();
    return {
      before,
      afterCopy,
      copied: copied.status,
      actual: actual.status,
      replay: replay.status
    };
  });
  expect(result.copied).toBe('invalid');
  expect(result.afterCopy).toEqual(result.before);
  expect(result.actual).toBe('qualified');
  expect(result.replay).toBe('invalid');
});
test('fresh explicit successful check review reuses genuine current qualification without new effects', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture;
    f.select();
    const first = await f.run(),
      qualification = f.qualification(),
      before = f.host.counts();
    f.select();
    const second = await f.run();
    return {
      first: first.status,
      second: second.status,
      same: qualification === f.qualification(),
      before,
      after: f.host.counts(),
      rows: (await f.host.stored()).length
    };
  });
  expect(result.first).toBe('qualified');
  expect(result.second).toBe('qualified');
  expect(result.same).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(result.rows).toBe(1);
  expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(3);
});

test('source review regression: another requested read origin cannot inherit cached qualification', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture;
    f.select();
    const first = await f.run(),
      before = f.host.counts();
    const changed = f.select('original', 'wss://archive.example.org');
    return { first: first.status, changed, before, after: f.host.counts() };
  });
  expect(result.first).toBe('qualified');
  expect(result.changed).toBeUndefined();
  expect(result.after).toEqual(result.before);
});

test('source review regression: bare coordinator cannot bypass reviewed cleanup selection', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture,
      api = window.hcCheck;
    f.select();
    const first = await f.run(),
      before = f.host.counts();
    const cleanup = await api.cleanupInboxAccessCheck(
      f.check()! as unknown as Parameters<
        typeof api.cleanupInboxAccessCheck
      >[0],
      'reviewed_delete_local_inbox_probe'
    );
    return {
      first: first.status,
      cleanup: cleanup.status,
      rows: (await f.host.stored()).length,
      before,
      after: f.host.counts()
    };
  });
  expect(result.first).toBe('qualified');
  expect(result.cleanup).toBe('invalid');
  expect(result.rows).toBe(1);
  expect(result.after).toEqual(result.before);
});

test('source review regression: bare coordinator cannot bypass reviewed reconciliation selection', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture,
      api = window.hcCheck,
      before = f.host.counts();
    const reconciled = await api.reconcileInboxAccessCheck(
      f.check()! as unknown as Parameters<
        typeof api.reconcileInboxAccessCheck
      >[0],
      'reviewed_inbox_access_check_reconciliation'
    );
    return {
      status: reconciled.status,
      before,
      after: f.host.counts(),
      rows: (await f.host.stored()).length
    };
  });
  expect(result.status).toBe('invalid');
  expect(result.rows).toBe(0);
  expect(result.after).toEqual(result.before);
});

test('source review regression: actual head loss closes the owned retained transport scope', async ({
  page
}) => {
  holdRead = true;
  await page.evaluate(() => {
    const f = window.hcCheckFixture;
    f.select();
    window.hcCheckPending = f.run();
  });
  await expect
    .poll(() => frames.some((frame) => frame.type === 'REQ' && !!frame.id))
    .toBe(true);
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture,
      before = window.hcCheckCloseCalls;
    f.actors().churnHead();
    const snapshot = f.snapshot(),
      after = window.hcCheckCloseCalls;
    const completed = await window.hcCheckPending;
    return {
      before,
      after,
      snapshot,
      status: completed.status,
      qualified: !!f.qualification()
    };
  });
  expect(result.status).toBe('stopped');
  expect(result.qualified).toBe(false);
  expect(result.snapshot?.state).toBe('stopped');
  expect(result.after).toBeGreaterThan(result.before);
  await expect
    .poll(
      () =>
        [...physical].filter(
          (socket) =>
            physicalOrigins.get(socket) !== 'wss://discovery.example.org'
        ).length
    )
    .toBe(0);
});

test('source review regression: explicit retry after actual nonce owner contention makes a later genuine read', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture,
      held = window.hcCheck.holdCheckNonceOwnerLock();
    try {
      f.select();
      const first = await f.run(),
        command = f.snapshot()?.command;
      await held.release();
      const before = f.host.counts();
      f.select();
      const second = await f.run();
      return {
        first: first.status,
        second: second.status,
        command,
        afterCommand: f.snapshot()?.command,
        hits: held.hits(),
        before,
        after: f.host.counts(),
        rows: (await f.host.stored()).length
      };
    } finally {
      await held.release();
      held.restore();
    }
  });
  expect(result.hits).toBe(1);
  expect(result.first).toBe('busy');
  expect(result.second).toBe('qualified');
  expect(result.afterCommand).toBe(result.command);
  expect(result.rows).toBe(1);
  expect(result.after.signs).toBe(result.before.signs);
  expect(result.after.encrypts).toBe(result.before.encrypts);
  expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(3);
  expect(
    frames.filter((frame) => frame.type === 'REQ' && !!frame.id)
  ).toHaveLength(2);
});

async function seedRecovery(page: Page, freshSession = false) {
  const original = await page.evaluate(async () => {
    const f = window.hcCheckFixture;
    f.select();
    const completed = await f.run(),
      command = f.snapshot()?.command,
      row = (await f.host.stored()).find(
        (row) => row.id === command && row.owner === f.host.owner
      );
    if (completed.status !== 'qualified' || !row)
      throw Error('actual coordinator seed failed');
    if (!command) throw Error('actual coordinator command missing');
    f.stop();
    return { command, row };
  });
  await expect
    .poll(
      () =>
        [...physical].filter(
          (socket) =>
            physicalOrigins.get(socket) !== 'wss://discovery.example.org'
        ).length
    )
    .toBe(0);
  await page.evaluate(async (freshSession) => {
    const f = window.hcCheckFixture;
    if (!f.retryCleanup()) throw Error('actual ended scope cleanup incomplete');
    await f.replaceCheck(freshSession);
  }, freshSession);
  return original;
}

for (const freshSession of [false, true]) {
  test(`persisted recovery through ${freshSession ? 'fresh session' : 'replacement controller'} uses exact original artifact without new signing or encryption`, async ({
    page
  }) => {
    const original = await seedRecovery(page, freshSession);
    const result = await page.evaluate(async ({ command }) => {
      const f = window.hcCheckFixture,
        before = f.host.counts();
      const disclosure = f.select(
        'recover',
        'wss://inbox.example.org',
        command
      );
      const result = await f.run();
      return {
        disclosure,
        status: result.status,
        before,
        after: f.host.counts(),
        row: (await f.host.stored())[0],
        snapshot: f.snapshot(),
        qualified: !!f.qualification()
      };
    }, original);
    expect(result.disclosure?.kind).toBe('check');
    if (!result.disclosure || !('mode' in result.disclosure))
      throw Error('actual check review metadata missing');
    expect(result.disclosure.mode).toBe('recover');
    expect(result.status).toBe('qualified');
    expect(result.qualified).toBe(true);
    expect(result.snapshot?.command).toBe(original.command);
    const before = probeRow(original.row.wire),
      after = probeRow(result.row.wire);
    expect(after.self).toEqual(before.self);
    expect(after.rumorHash).toBe(before.rumorHash);
    expect(after.id).toBe(before.id);
    expect(result.after.signs).toBe(result.before.signs);
    expect(result.after.encrypts).toBe(result.before.encrypts);
    expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(6);
    expect(
      new Set(
        frames
          .filter((frame) => frame.type === 'EVENT')
          .map((frame) => frame.id)
      ).size
    ).toBe(1);
  });
}

test('persisted recovery explicit retry after actual nonce owner contention spends a later real read under the same command', async ({
  page
}) => {
  const original = await seedRecovery(page, true);
  const result = await page.evaluate(async ({ command }) => {
    const f = window.hcCheckFixture,
      held = window.hcCheck.holdCheckNonceOwnerLock();
    try {
      f.select('recover', 'wss://inbox.example.org', command);
      const first = await f.run();
      await held.release();
      const before = f.host.counts();
      f.select('recover', 'wss://inbox.example.org', command);
      const second = await f.run();
      return {
        first: first.status,
        second: second.status,
        hits: held.hits(),
        before,
        after: f.host.counts(),
        command: f.snapshot()?.command,
        row: (await f.host.stored())[0]
      };
    } finally {
      await held.release();
      held.restore();
    }
  }, original);
  expect(result.hits).toBe(1);
  expect(result.first).toBe('busy');
  expect(result.second).toBe('qualified');
  expect(result.command).toBe(original.command);
  expect(probeRow(result.row.wire).self).toEqual(
    probeRow(original.row.wire).self
  );
  expect(result.after.signs).toBe(result.before.signs);
  expect(result.after.encrypts).toBe(result.before.encrypts);
  expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(6);
  expect(
    frames.filter((frame) => frame.type === 'REQ' && !!frame.id)
  ).toHaveLength(3);
});

for (const lane of ['original', 'recover'] as const) {
  test(`${lane} optional cleanup requires its own genuine one-use review and yields independently covered removed qualification`, async ({
    page
  }) => {
    const seed =
      lane === 'recover' ? await seedRecovery(page, true) : undefined;
    const result = await page.evaluate(
      async ({ lane, command }) => {
        const f = window.hcCheckFixture,
          api = window.hcCheck;
        f.select(lane, 'wss://inbox.example.org', command);
        const checked = await f.run(),
          before = f.host.counts();
        const disclosure = f.selectCleanup(),
          review = f.review()!;
        const copied = await api.cleanupInboxAccessCheck(
          { ...review },
          'reviewed_delete_local_inbox_probe'
        );
        const wrongKind = await api.runInboxAccessCheck(
          review,
          'reviewed_inbox_access_check'
        );
        const wrongSentinel = await f.cleanup('not_reviewed');
        const rowsBefore = (await f.host.stored()).length;
        const removed = await f.cleanup(),
          replay = await f.cleanup();
        return {
          checked: checked.status,
          disclosure,
          copied: copied.status,
          wrongKind: wrongKind.status,
          wrongSentinel: wrongSentinel.status,
          rowsBefore,
          removed: removed.status,
          replay: replay.status,
          rowsAfter: (await f.host.stored()).length,
          qualified: !!f.qualification(),
          qualification:
            f.qualification() &&
            api.inboxAccessQualificationSnapshot(f.qualification()!),
          before,
          after: f.host.counts()
        };
      },
      { lane, command: seed?.command }
    );
    expect(result.checked).toBe('qualified');
    expect(result.disclosure?.kind).toBe('cleanup');
    expect(result.copied).toBe('invalid');
    expect(result.wrongKind).toBe('invalid');
    expect(result.wrongSentinel).toBe('invalid');
    expect(result.rowsBefore).toBe(1);
    expect(result.removed).toBe('removed');
    expect(result.replay).toBe('invalid');
    expect(result.rowsAfter).toBe(0);
    expect(result.qualified).toBe(true);
    expect(result.qualification?.sourceOnly).toBe(true);
    expect(result.after.signs).toBe(result.before.signs);
    expect(result.after.encrypts).toBe(result.before.encrypts);
    expect(result.after.decrypts).toBe(result.before.decrypts);
  });
}

test('genuine coordinator capture rejects copied policy, operator, C7, identity, session and repository before effects', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture,
      api = window.hcCheck,
      a = f.actors(),
      before = f.host.counts(),
      rows = await f.host.stored();
    type Args = Parameters<typeof api.createInboxAccessCheck>;
    const actual: Args = [
      f.repository,
      a.identity,
      a.session,
      f.host.policy,
      a.access,
      f.host.qualification
    ];
    const copied = actual.map((_, index) => {
      const args = actual.slice() as Args;
      args[index] = { ...args[index] };
      return !!api.createInboxAccessCheck(...args);
    });
    const policy = api.createInboxAccessCheck(
      f.repository,
      a.identity,
      a.session,
      f.host.copiedPolicy(),
      a.access,
      f.host.qualification
    );
    const unsupported = api.createInboxAccessCheck(
      f.repository,
      a.identity,
      a.session,
      f.host.policy,
      a.access,
      f.host.operator(true)
    );
    const unconfigured = api.createInboxAccessCheck(
      f.repository,
      a.identity,
      a.session,
      f.host.policy,
      a.access,
      f.host.operator(false, true)
    );
    const foreignRepository = api.createInboxAccessCheck(
      f.host.foreignRepository(),
      a.identity,
      a.session,
      f.host.policy,
      a.access,
      f.host.qualification
    );
    const otherOperator = api.createInboxAccessCheck(
      f.repository,
      a.identity,
      a.session,
      f.host.policy,
      a.access,
      f.host.operator()
    );
    return {
      copied,
      policy: !!policy,
      unsupported: !!unsupported,
      unconfigured: !!unconfigured,
      foreignRepository: !!foreignRepository,
      otherOperator: !!otherOperator,
      same: api.createInboxAccessCheck(...actual) === f.check(),
      before,
      after: f.host.counts(),
      rows,
      afterRows: await f.host.stored()
    };
  });
  expect(result.copied).toEqual([false, false, false, false, false, false]);
  expect(result.policy).toBe(false);
  expect(result.unsupported).toBe(false);
  expect(result.unconfigured).toBe(false);
  expect(result.foreignRepository).toBe(false);
  expect(result.otherOperator).toBe(false);
  expect(result.same).toBe(true);
  expect(result.after).toEqual(result.before);
  expect(result.afterRows).toEqual(result.rows);
  expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(0);
});

test('unsupported and malformed review targets and recovery ids remain inert before a valid selection', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture,
      before = f.host.counts();
    const rejected = [
      f.select('other'),
      f.select('original', 'wss://unapproved.example.org'),
      f.select('recover', 'wss://inbox.example.org'),
      f.select('recover', 'wss://inbox.example.org', 'not-a-command'),
      f.select('original', 'wss://inbox.example.org', crypto.randomUUID())
    ];
    const original = f.select(),
      stale = f.review()!;
    f.select('original', 'wss://peer.example.org');
    const replaced = await window.hcCheck.runInboxAccessCheck(
      stale,
      'reviewed_inbox_access_check'
    );
    return {
      rejected: rejected.map(Boolean),
      original: !!original,
      replaced: replaced.status,
      before,
      after: f.host.counts(),
      rows: (await f.host.stored()).length
    };
  });
  expect(result.rejected).toEqual([false, false, false, false, false]);
  expect(result.original).toBe(true);
  expect(result.replaced).toBe('invalid');
  expect(result.after).toEqual(result.before);
  expect(result.rows).toBe(0);
  expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(0);
});

for (const lane of ['original', 'recover'] as const) {
  for (const contention of ['lock', 'SDK'] as const) {
    for (const holds of [1, 2] as const) {
      test(`${lane} nonce ${contention} contention ${holds === 1 ? 'retries with later real read' : 'exhausts the combined bound without renewal'}`, async ({
        page
      }) => {
        const seed =
          lane === 'recover' ? await seedRecovery(page, true) : undefined;
        const result = await page.evaluate(
          async ({ lane, command, contention, holds }) => {
            const f = window.hcCheckFixture,
              api = window.hcCheck;
            const held =
              contention === 'lock'
                ? api.holdCheckNonceOwnerLock(holds)
                : api.holdCheckNonceSdk(f.actors().session, holds);
            const outcomes: string[] = [];
            try {
              const before = f.host.counts();
              for (let n = 0; n < holds + 1; n++) {
                f.select(lane, 'wss://inbox.example.org', command);
                outcomes.push((await f.run()).status);
                await held.release();
              }
              const row = (await f.host.stored())[0];
              return {
                outcomes,
                hits: held.hits(),
                before,
                after: f.host.counts(),
                command: f.snapshot()?.command,
                row,
                qualified: !!f.qualification()
              };
            } finally {
              await held.release();
              held.restore();
            }
          },
          { lane, command: seed?.command, contention, holds }
        );
        expect(result.hits).toBe(holds);
        expect(result.outcomes.slice(0, holds)).toEqual(
          Array(holds).fill('busy')
        );
        expect(result.outcomes.at(-1)).toBe(
          holds === 1
            ? 'qualified'
            : lane === 'original'
              ? 'stopped'
              : 'unavailable'
        );
        expect(result.qualified).toBe(holds === 1);
        const row = JSON.parse(result.row.wire) as {
          id: string;
          self: { eventId: string };
          receipts: {
            actionId: string;
            eventId: string;
            origin: string;
            role: string;
            status: string;
            attempt: number;
          }[];
        };
        const read = row.receipts
          .filter(
            (fact) =>
              fact.role === 'access_readback' && fact.status === 'readback'
          )
          .at(-1)!;
        const action = row.receipts.filter(
          (fact) => fact.actionId === read.actionId
        );
        expect(new Set(action.map((fact) => fact.eventId))).toEqual(
          new Set([row.self.eventId])
        );
        expect(
          action
            .filter(
              (fact) =>
                fact.origin === 'wss://inbox.example.org' &&
                fact.status !== 'nonce_verified'
            )
            .map((fact) => [fact.role, fact.attempt])
        ).toEqual([
          ['access_archive', 1],
          ['access_readback', 2],
          ['access_readback', 3]
        ]);
        expect(
          action
            .filter((fact) => fact.status === 'nonce_verified')
            .map((fact) => fact.attempt)
        ).toEqual(holds === 1 ? [3] : []);
        expect(result.command).toBe(row.id);
        if (seed) expect(row.self).toEqual(probeRow(seed.row.wire).self);
        expect(result.after.signs - result.before.signs).toBe(
          lane === 'original' ? 1 : 0
        );
        expect(result.after.encrypts - result.before.encrypts).toBe(
          lane === 'original' ? 1 : 0
        );
        expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(
          lane === 'original' ? 3 : 6
        );
        expect(
          frames.filter((frame) => frame.type === 'REQ' && !!frame.id)
        ).toHaveLength(lane === 'original' ? 2 : 3);
      });
    }
  }
}

test('actual child page reload recovers and qualifies original stored ciphertext in two fresh source realms', async ({
  page
}) => {
  const setup = await page.evaluate(async () => {
    const f = window.hcCheckFixture,
      api = window.hcCheck;
    f.select();
    if ((await f.run()).status !== 'qualified')
      throw Error('actual original coordinator qualification');
    const row = (await f.host.stored())[0];
    const result = {
      owner: f.host.owner,
      command: f.snapshot()!.command!,
      policyWire: JSON.stringify(api.readRelayPolicy(f.host.policy)),
      operatorWire: api.inboxOperatorQualificationWire(
        f.host.qualification,
        f.host.policy
      )!,
      row
    };
    f.stop();
    return result;
  });
  await expect
    .poll(
      () =>
        [...physical].filter(
          (socket) =>
            physicalOrigins.get(socket) !== 'wss://discovery.example.org'
        ).length
    )
    .toBe(0);
  expect(await page.evaluate(() => window.hcCheckFixture.retryCleanup())).toBe(
    true
  );
  await page.evaluate((url) => {
    const iframe = document.createElement('iframe');
    iframe.id = 'coordinator-reload';
    iframe.src = url;
    document.body.append(iframe);
  }, server.url + '/search');
  const frame = await (
    await page.locator('#coordinator-reload').elementHandle()
  ).contentFrame();
  if (!frame) throw Error('actual child source realm missing');
  await frame.waitForLoadState();
  try {
    for (let cycle = 0; cycle < 2; cycle++) {
      if (cycle) await frame.goto(server.url + '/search');
      await frame.addScriptTag({ content: bundle });
      await frame.evaluate(async (setup) => {
        window.hcHostedCheck = await window.hcCheck.openHostedCheck(setup);
      }, setup);
      const before = await page.evaluate(() =>
        window.hcCheckFixture.host.counts()
      );
      const result = await frame.evaluate(async (setup) => {
        const api = window.hcCheck,
          { check } = window.hcHostedCheck;
        const selected = api.reviewInboxAccessCheck(
          check,
          'recover',
          'wss://inbox.example.org',
          setup.command
        );
        if (!selected) throw Error('actual hosted recovery review missing');
        const result = await api.runInboxAccessCheck(
          selected,
          'reviewed_inbox_access_check'
        );
        const output = {
          status: result.status,
          command: api.inboxAccessCheckSnapshot(check)?.command,
          qualified: !!api.inboxAccessCheckQualification(check)
        };
        api.stopInboxAccessCheck(check);
        return output;
      }, setup);
      expect(result).toEqual({
        status: 'qualified',
        command: setup.command,
        qualified: true
      });
      await expect
        .poll(
          () =>
            [...physical].filter(
              (socket) =>
                physicalOrigins.get(socket) !== 'wss://discovery.example.org'
            ).length
        )
        .toBe(0);
      await frame.evaluate(() => {
        const { check, context } = window.hcHostedCheck;
        if (!window.hcCheck.stopInboxAccessCheck(check))
          throw Error('actual hosted physical cleanup incomplete');
        context.close();
      });
      const after = await page.evaluate(() =>
        window.hcCheckFixture.host.counts()
      );
      expect(after.signs).toBe(before.signs);
      expect(after.encrypts).toBe(before.encrypts);
      const row = await page.evaluate(
        async () => (await window.hcCheckFixture.host.stored())[0]
      );
      expect(probeRow(row.wire).self).toEqual(probeRow(setup.row.wire).self);
      expect(probeRow(row.wire).rumorHash).toBe(
        probeRow(setup.row.wire).rumorHash
      );
    }
  } finally {
    await frame.evaluate(() => {
      if (window.hcHostedCheck) {
        window.hcCheck.stopInboxAccessCheck(window.hcHostedCheck.check);
        window.hcHostedCheck.context.close();
      }
    });
    await page
      .locator('#coordinator-reload')
      .evaluate((element) => element.remove());
  }
  expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(9);
  expect(
    new Set(
      frames.filter((frame) => frame.type === 'EVENT').map((frame) => frame.id)
    ).size
  ).toBe(1);
});

for (const lane of ['original', 'recover'] as const) {
  for (const fault of [
    'refused',
    'wrong_ack',
    'disconnect',
    'missing',
    'wrong_read'
  ] as const) {
    test(`${lane} actual ${fault} wire outcome cannot qualify or fabricate successful readback`, async ({
      page
    }) => {
      const seed =
        lane === 'recover' ? await seedRecovery(page, true) : undefined;
      const priorEvents = frames.filter(
        (frame) => frame.type === 'EVENT'
      ).length;
      wireFault = fault;
      const result = await page.evaluate(
        async ({ lane, command }) => {
          const f = window.hcCheckFixture;
          f.select(lane, 'wss://inbox.example.org', command);
          const completed = await f.run();
          return {
            status: completed.status,
            qualification: !!f.qualification(),
            snapshot: f.snapshot(),
            rows: await f.host.stored()
          };
        },
        { lane, command: seed?.command }
      );
      expect(result.status).not.toBe('qualified');
      expect(result.qualification).toBe(false);
      expect(result.rows).toHaveLength(1);
      const row = JSON.parse(result.rows[0].wire) as {
        self: { eventId: string };
        receipts: { status: string; actionId: string }[];
      };
      const added = row.receipts.slice(
        seed ? probeRow(seed.row.wire).receipts.length : 0
      );
      expect(added.some((fact) => fact.status === 'nonce_verified')).toBe(
        false
      );
      if (fault === 'missing' || fault === 'wrong_read') {
        expect(added.filter((fact) => fact.status === 'accepted')).toHaveLength(
          3
        );
        expect(added.some((fact) => fact.status === 'readback')).toBe(false);
      } else {
        expect(added.some((fact) => fact.status === 'accepted')).toBe(false);
        expect(
          frames.filter((frame) => frame.type === 'EVENT').length - priorEvents
        ).toBe(1);
      }
      if (seed) expect(row.self).toEqual(probeRow(seed.row.wire).self);
    });
  }
  for (const fault of ['stop', 'head', 'session'] as const) {
    for (const callback of ['key', 'cipher'] as const) {
      test(`${lane} actual ${callback} callback ${fault} loss cannot continue effects or issue qualification`, async ({
        page
      }) => {
        const seed =
          lane === 'recover' ? await seedRecovery(page, true) : undefined;
        const priorEvents = frames.filter(
          (frame) => frame.type === 'EVENT'
        ).length;
        const result = await page.evaluate(
          async ({ lane, command, fault, callback }) => {
            const f = window.hcCheckFixture,
              api = window.hcCheck;
            let hits = 0;
            const lose = () => {
              if (hits++) return;
              if (fault === 'stop') f.stop();
              else if (fault === 'head') f.actors().churnHead();
              else api.closePrivateSession(f.actors().session);
            };
            const restore =
              callback === 'key'
                ? f.host.keyCallback(1, lose)
                : f.host.cipherGetter(lose);
            try {
              f.select(lane, 'wss://inbox.example.org', command);
              const result = await f.run();
              return {
                status: result.status,
                hits,
                qualified: !!f.qualification(),
                snapshot: f.snapshot(),
                rows: await f.host.stored()
              };
            } finally {
              restore();
            }
          },
          { lane, command: seed?.command, fault, callback }
        );
        expect(result.hits).toBeGreaterThan(0);
        expect(result.status).not.toBe('qualified');
        expect(result.qualified).toBe(false);
        expect(result.snapshot?.state).toBe('stopped');
        expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(
          priorEvents
        );
        if (seed) expect(result.rows).toEqual([seed.row]);
      });
    }
  }
  for (const closeMode of ['hold', 'fail'] as const) {
    test(`${lane} actual ${closeMode} physical close retains cleanup requirement until explicit retry`, async ({
      page
    }) => {
      const seed =
        lane === 'recover' ? await seedRecovery(page, true) : undefined;
      const result = await page.evaluate(
        async ({ lane, command, closeMode }) => {
          const f = window.hcCheckFixture;
          f.select(lane, 'wss://inbox.example.org', command);
          const checked = await f.run();
          window.hcCheckCloseMode = closeMode;
          const stopped = f.stop(),
            snapshot = f.snapshot(),
            closes = window.hcCheckCloseCalls;
          const selection = f.select(lane, 'wss://inbox.example.org', command);
          const observed = f.snapshot();
          return {
            checked: checked.status,
            stopped,
            snapshot,
            selection,
            observed,
            closes,
            afterCloses: window.hcCheckCloseCalls,
            qualified: !!f.qualification(),
            held: window.hcCheckHeldClose?.length ?? 0
          };
        },
        { lane, command: seed?.command, closeMode }
      );
      expect(result.checked).toBe('qualified');
      expect(result.stopped).toBe(false);
      expect(result.snapshot?.cleanupRequired).toBe(true);
      expect(result.observed?.cleanupRequired).toBe(true);
      expect(result.selection).toBeUndefined();
      expect(result.afterCloses).toBe(result.closes);
      expect(result.qualified).toBe(false);
      if (closeMode === 'hold') expect(result.held).toBeGreaterThan(0);
      await page.evaluate(() => {
        window.hcCheckCloseMode = undefined;
        for (const release of window.hcCheckHeldClose ?? []) release();
        window.hcCheckHeldClose = [];
        window.hcCheckFixture.retryCleanup();
      });
      await expect
        .poll(
          () =>
            [...physical].filter(
              (socket) =>
                physicalOrigins.get(socket) !== 'wss://discovery.example.org'
            ).length
        )
        .toBe(0);
      expect(
        await page.evaluate(() => window.hcCheckFixture.retryCleanup())
      ).toBe(true);
    });
  }
}

test('authentic pending archive survives Stop and requires a distinct reviewed historical reconciliation without new effects', async ({
  page
}) => {
  holdArchive = true;
  await page.evaluate(() => {
    const f = window.hcCheckFixture;
    f.select();
    window.hcCheckPending = f.run();
  });
  await expect.poll(() => !!releaseArchive).toBe(true);
  await page.evaluate(() => {
    // Publication correctly owns the real lock through its network result.
    // Queue the competing holder, then release the actual ACK so it acquires
    // before append's separate ifAvailable request; never wait out the relay.
    window.hcCheckLockReady = window.hcCheck
      .holdCheckOwnerLock(window.hcCheckFixture.host.owner)
      .then((release) => {
        window.hcCheckRelease = release;
      });
  });
  holdArchive = false;
  releaseArchive!();
  const pending = await page.evaluate(async () => {
    await window.hcCheckLockReady;
    const f = window.hcCheckFixture,
      completed = await window.hcCheckPending;
    const snapshot = f.snapshot();
    f.stop();
    await window.hcCheckRelease!();
    window.hcCheckRelease = undefined;
    return { status: completed.status, snapshot };
  });
  expect(pending.status).toBe('busy');
  expect(pending.snapshot?.pendingSettlement).toBe(true);
  const reconciled = await page.evaluate(async () => {
    const f = window.hcCheckFixture,
      api = window.hcCheck,
      before = f.host.counts();
    const disclosure = f.selectReconciliation(),
      selected = f.review()!;
    const copied = await api.reconcileInboxAccessCheck(
      { ...selected },
      'reviewed_inbox_access_check_reconciliation'
    );
    const wrongKind = await api.cleanupInboxAccessCheck(
      selected,
      'reviewed_delete_local_inbox_probe'
    );
    const result = await f.reconcile(),
      replay = await f.reconcile();
    return {
      disclosure,
      copied: copied.status,
      wrongKind: wrongKind.status,
      status: result.status,
      replay: replay.status,
      before,
      after: f.host.counts(),
      snapshot: f.snapshot(),
      qualified: !!f.qualification(),
      rows: await f.host.stored()
    };
  });
  expect(reconciled.disclosure?.kind).toBe('reconcile');
  expect(reconciled.copied).toBe('invalid');
  expect(reconciled.wrongKind).toBe('invalid');
  expect(['saved', 'existing', 'reconciled']).toContain(reconciled.status);
  expect(reconciled.replay).toBe('invalid');
  expect(reconciled.after).toEqual(reconciled.before);
  expect(reconciled.snapshot?.pendingSettlement).toBe(false);
  expect(reconciled.qualified).toBe(false);
  expect(reconciled.snapshot?.state).toBe('stopped');
  expect(
    probeRow(reconciled.rows[0].wire).receipts.filter(
      (fact: { status: string }) => fact.status === 'accepted'
    )
  ).toHaveLength(1);
  expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(1);
});

async function beginActualAuth(page: Page, mode: 'accept' | 'refuse' | 'hold') {
  challenge = true;
  authMode = mode;
  const before = frames.filter((frame) => frame.type === 'AUTH').length;
  // On a cold socket this API initially observes stopped until actual open;
  // the following real challenge observation is required before responding.
  expect(['waiting', 'challenge', 'stopped']).toContain(
    await page.evaluate(() =>
      window.hcCheck.authenticateInboxAccess(
        window.hcCheckFixture.actors().access,
        'wss://inbox.example.org',
        'reviewed_inbox_connection_auth'
      )
    )
  );
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.hcCheck
            .inboxAccessControllerSourceSnapshot(
              window.hcCheckFixture.actors().access
            )
            ?.auth.find((row) => row.origin === 'wss://inbox.example.org')
            ?.state
      )
    )
    .toBe('challenge');
  await page.evaluate(() => {
    window.hcCheckAuthPending = window.hcCheck.respondInboxAccessAuthentication(
      window.hcCheckFixture.actors().access,
      'wss://inbox.example.org',
      'reviewed_inbox_auth_response'
    );
  });
  await expect
    .poll(() => frames.filter((frame) => frame.type === 'AUTH').length)
    .toBe(before + 1);
  if (mode !== 'hold')
    expect(await page.evaluate(() => window.hcCheckAuthPending)).toBe(
      mode === 'accept' ? 'accepted' : 'refused'
    );
}

test('required AUTH pending response refuses coordinator work without a second SDK job or diagnostic effect', async ({
  page
}) => {
  await beginActualAuth(page, 'hold');
  try {
    const result = await page.evaluate(async () => {
      const f = window.hcCheckFixture,
        before = f.host.counts();
      f.select();
      const result = await f.run();
      return {
        status: result.status,
        before,
        after: f.host.counts(),
        rows: await f.host.stored(),
        qualified: !!f.qualification()
      };
    });
    expect(result.status).not.toBe('qualified');
    expect(result.qualified).toBe(false);
    expect(result.after).toEqual(result.before);
    expect(result.rows).toHaveLength(0);
    expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(0);
    expect(frames.filter((frame) => frame.type === 'AUTH')).toHaveLength(1);
  } finally {
    releaseAuth!();
    await page.evaluate(() => window.hcCheckAuthPending);
  }
});

for (const lane of ['original', 'recover'] as const) {
  for (const auth of ['accept', 'refuse', 'hold'] as const) {
    test(`${lane} required AUTH ${auth} uses genuine C7 response and preserves source-only presentation`, async ({
      page
    }) => {
      let seed: Awaited<ReturnType<typeof seedRecovery>> | undefined;
      if (lane === 'recover') {
        await beginActualAuth(page, 'accept');
        seed = await seedRecovery(page, true);
      }
      await beginActualAuth(page, auth);
      const inert = await page.evaluate(() => ({
        qualified: !!window.hcCheckFixture.qualification(),
        readiness: window.hcCheck.inboxAccessControllerSnapshot(
          window.hcCheckFixture.actors().access
        )?.readiness
      }));
      expect(inert).toEqual({ qualified: false, readiness: 'unavailable' });
      if (auth === 'hold') {
        expect(
          await page.evaluate(
            () =>
              window.hcCheck
                .inboxAccessControllerSourceSnapshot(
                  window.hcCheckFixture.actors().access
                )
                ?.auth.find((row) => row.origin === 'wss://inbox.example.org')
                ?.state
          )
        ).toBe('signing');
        expect(releaseAuth).toBeDefined();
        releaseAuth!();
        expect(await page.evaluate(() => window.hcCheckAuthPending)).toBe(
          'accepted'
        );
      }
      const result = await page.evaluate(
        async ({ lane, command }) => {
          const f = window.hcCheckFixture;
          f.select(lane, 'wss://inbox.example.org', command);
          const result = await f.run();
          return {
            status: result.status,
            qualified: !!f.qualification(),
            readiness: window.hcCheck.inboxAccessControllerSnapshot(
              f.actors().access
            )?.readiness,
            sourceOnly: f.snapshot()?.sourceOnly
          };
        },
        { lane, command: seed?.command }
      );
      expect(result.readiness).toBe('unavailable');
      expect(result.sourceOnly).toBe(true);
      if (auth === 'refuse') {
        expect(result.status).not.toBe('qualified');
        expect(result.qualified).toBe(false);
      } else {
        expect(result.status).toBe('qualified');
        expect(result.qualified).toBe(true);
        const before = await page.evaluate(() =>
          window.hcCheckFixture.host.counts()
        );
        for (const socket of physical)
          if (physicalOrigins.get(socket) === 'wss://inbox.example.org')
            socket.send(JSON.stringify(['AUTH', 's5c-genuine-new-challenge']));
        await expect
          .poll(() =>
            page.evaluate(
              () =>
                window.hcCheck
                  .inboxAccessControllerSourceSnapshot(
                    window.hcCheckFixture.actors().access
                  )
                  ?.auth.find((row) => row.origin === 'wss://inbox.example.org')
                  ?.state
            )
          )
          .toBe('challenge');
        expect(
          await page.evaluate(() => !!window.hcCheckFixture.qualification())
        ).toBe(false);
        expect(
          await page.evaluate(() => window.hcCheckFixture.host.counts())
        ).toEqual(before);
      }
    });
  }
}

for (const lane of ['original', 'recover'] as const) {
  for (const mode of ['missing', 'corrupt', 'alias', 'foreign'] as const) {
    test(`${lane} actual nonce-stage ${mode} native journal observation preserves source qualification rules`, async ({
      page
    }) => {
      const seed =
        lane === 'recover' ? await seedRecovery(page, true) : undefined;
      const result = await page.evaluate(
        async ({ lane, command, mode }) => {
          const f = window.hcCheckFixture,
            api = window.hcCheck;
          const hook = api.atCheckNonce(async () => {
            const row = (await f.host.stored()).find(
              (row) =>
                row.owner === f.host.owner && row.id === f.snapshot()?.command
            );
            if (!row)
              throw Error(
                'actual selected native journal missing before mutation'
              );
            await api.changeCheckNativeRow(row, mode);
          });
          try {
            f.select(lane, 'wss://inbox.example.org', command);
            const result = await f.run();
            return {
              status: result.status,
              hits: hook.hits(),
              qualified: !!f.qualification(),
              rows: await f.host.stored()
            };
          } finally {
            hook.restore();
          }
        },
        { lane, command: seed?.command, mode }
      );
      expect(result.hits).toBe(1);
      if (mode === 'foreign') {
        expect(result.status).toBe('qualified');
        expect(result.qualified).toBe(true);
        expect(result.rows).toHaveLength(2);
        expect(new Set(result.rows.map((row) => row.owner)).size).toBe(2);
      } else {
        expect(result.status).not.toBe('qualified');
        expect(result.qualified).toBe(false);
      }
    });
  }
  test(`${lane} actual native admission abort cannot become qualification`, async ({
    page
  }) => {
    const seed =
      lane === 'recover' ? await seedRecovery(page, true) : undefined;
    const result = await page.evaluate(
      async ({ lane, command }) => {
        const f = window.hcCheckFixture,
          fault = f.installCommitFault('aborted');
        try {
          f.select(lane, 'wss://inbox.example.org', command);
          const result = await f.run();
          const rows = await f.host.stored();
          return {
            status: result.status,
            counts: fault.counts(),
            qualified: !!f.qualification(),
            rows
          };
        } finally {
          fault.restore();
        }
      },
      { lane, command: seed?.command }
    );
    expect(result.counts.aborted).toBe(1);
    expect(result.status).not.toBe('qualified');
    expect(result.qualified).toBe(false);
    if (lane === 'recover') {
      const reconciled = await page.evaluate(async () => {
        const f = window.hcCheckFixture;
        if (!f.snapshot()?.pendingSettlement) return 'no_pending';
        if (!f.selectReconciliation())
          throw Error('actual aborted attempt review missing');
        return (await f.reconcile()).status;
      });
      expect(['saved', 'existing', 'reconciled']).toContain(reconciled);
    }
    if (seed) expect(result.rows).toEqual([seed.row]);
    else {
      expect(result.rows).toHaveLength(0);
      expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(0);
    }
  });
  test(`${lane} actual reviewed cleanup abort preserves the journal and permanently invalidates the old journal qualification`, async ({
    page
  }) => {
    const seed =
      lane === 'recover' ? await seedRecovery(page, true) : undefined;
    const result = await page.evaluate(
      async ({ lane, command }) => {
        const f = window.hcCheckFixture,
          api = window.hcCheck;
        f.select(lane, 'wss://inbox.example.org', command);
        const checked = await f.run(),
          old = f.qualification()!,
          before = await f.host.stored(),
          fault = api.abortCheckCleanup();
        try {
          f.selectCleanup();
          const removed = await f.cleanup();
          const after = await f.host.stored();
          return {
            checked: checked.status,
            status: removed.status,
            aborts: fault.aborts(),
            before,
            after,
            qualified: !!f.qualification(),
            oldSnapshot: api.inboxAccessQualificationSnapshot(old),
            afterSnapshot: api.inboxAccessQualificationSnapshot(old)
          };
        } finally {
          fault.restore();
        }
      },
      { lane, command: seed?.command }
    );
    expect(result.checked).toBe('qualified');
    expect(result.aborts).toBe(1);
    expect(result.status).not.toBe('removed');
    expect(result.after).toEqual(result.before);
    expect(result.qualified).toBe(false);
    expect(result.oldSnapshot).toBeUndefined();
    expect(result.afterSnapshot).toBeUndefined();
  });
  for (const cleanup of [false, true]) {
    test(`${lane} actual own InboxSync ${cleanup ? 'after reviewed cleanup retains then rejects the self message' : 'excludes the live genuine diagnostic journal'} without losing qualification`, async ({
      page
    }) => {
      const seed =
        lane === 'recover' ? await seedRecovery(page, true) : undefined;
      const started = await page.evaluate(
        async ({ lane, command, cleanup }) => {
          const f = window.hcCheckFixture;
          f.select(lane, 'wss://inbox.example.org', command);
          if ((await f.run()).status !== 'qualified')
            throw Error('actual coordinator qualification required');
          if (cleanup) {
            f.selectCleanup();
            if ((await f.cleanup()).status !== 'removed')
              throw Error('actual optional removal required');
          }
          return f.startOwnInbox();
        },
        { lane, command: seed?.command, cleanup }
      );
      expect(started).toBe(true);
      await expect
        .poll(() =>
          page.evaluate(() => {
            const s = window.hcCheckFixture.syncSnapshot();
            return (
              !!s && s.pending === 0 && (s.excludedProbes > 0 || s.retained > 0)
            );
          })
        )
        .toBe(true);
      const result = await page.evaluate(async (cleanup) => {
        const f = window.hcCheckFixture,
          before = f.host.counts();
        const decrypted = cleanup ? await f.decryptReceived() : undefined;
        return {
          snapshot: f.syncSnapshot(),
          qualified: !!f.qualification(),
          decrypted,
          before,
          after: f.host.counts(),
          rows: (await f.host.stored()).length
        };
      }, cleanup);
      expect(result.qualified).toBe(true);
      if (cleanup) {
        expect(result.snapshot?.retained).toBe(1);
        expect(result.snapshot?.excludedProbes).toBe(0);
        expect(result.decrypted?.rooms).toBe(0);
        expect(result.decrypted?.snapshot.invalid).toBe(1);
        expect(result.after.decrypts - result.before.decrypts).toBe(2);
        expect(result.rows).toBe(0);
      } else {
        expect(result.snapshot?.retained).toBe(0);
        expect(result.snapshot?.excludedProbes).toBeGreaterThan(0);
        expect(result.after).toEqual(result.before);
        expect(result.rows).toBe(1);
      }
    });
  }
}

for (const lane of ['original', 'recover'] as const) {
  for (const mode of [
    'wrong_key',
    'refuse_decrypt',
    'wrong_decrypt'
  ] as const) {
    test(`${lane} actual nonce provider ${mode} cannot qualify or silently repeat rejected crypto`, async ({
      page
    }) => {
      const seed =
        lane === 'recover' ? await seedRecovery(page, true) : undefined;
      const result = await page.evaluate(
        async ({ lane, command, mode }) => {
          const f = window.hcCheckFixture,
            hook = window.hcCheck.atCheckNonce(async () => {
              f.host.mode(mode);
              await Promise.resolve();
            });
          try {
            f.select(lane, 'wss://inbox.example.org', command);
            const first = await f.run(),
              before = f.host.counts();
            f.host.mode('normal');
            const selection = f.select(
              lane,
              'wss://inbox.example.org',
              command
            );
            const second = selection ? await f.run() : undefined;
            return {
              first: first.status,
              second: second?.status,
              hits: hook.hits(),
              before,
              after: f.host.counts(),
              rows: await f.host.stored(),
              qualified: !!f.qualification()
            };
          } finally {
            hook.restore();
            f.host.mode('normal');
          }
        },
        { lane, command: seed?.command, mode }
      );
      expect(result.hits).toBe(1);
      expect(result.first).not.toBe('qualified');
      expect(result.second).not.toBe('qualified');
      expect(result.qualified).toBe(false);
      expect(result.after).toEqual(result.before);
      const oldFacts = seed ? probeRow(seed.row.wire).receipts.length : 0;
      expect(
        probeRow(result.rows[0].wire)
          .receipts.slice(oldFacts)
          .some((fact) => fact.status === 'nonce_verified')
      ).toBe(false);
    });
  }
  for (const loss of ['stop', 'head', 'session'] as const) {
    test(`${lane} actual held nonce decrypt ${loss} loss retains the admitted promise until real settlement`, async ({
      page
    }) => {
      const seed =
        lane === 'recover' ? await seedRecovery(page, true) : undefined;
      await page.evaluate(
        ({ lane, command }) => {
          const f = window.hcCheckFixture;
          window.hcCheckRestore = window.hcCheck.atCheckNonce(async () => {
            f.host.mode('hold_decrypt');
            await Promise.resolve();
          }).restore;
          f.select(lane, 'wss://inbox.example.org', command);
          window.hcCheckPending = f.run();
        },
        { lane, command: seed?.command }
      );
      await expect
        .poll(() => page.evaluate(() => window.hcCheckFixture.host.waiting()))
        .toBe('decrypt');
      const during = await page.evaluate((loss) => {
        const f = window.hcCheckFixture;
        if (loss === 'stop') f.stop();
        else if (loss === 'head') f.actors().churnHead();
        else window.hcCheck.closePrivateSession(f.actors().session);
        const result = {
          snapshot: f.snapshot(),
          qualified: !!f.qualification(),
          counts: f.host.counts(),
          waiting: f.host.waiting()
        };
        f.host.settle();
        return result;
      }, loss);
      expect(during.snapshot?.state).toBe('stopped');
      expect(during.snapshot?.busy).toBe(true);
      expect(during.qualified).toBe(false);
      expect(during.waiting).toBe('decrypt');
      const after = await page.evaluate(async () => {
        const f = window.hcCheckFixture,
          result = await window.hcCheckPending;
        f.host.mode('normal');
        return {
          status: result.status,
          snapshot: f.snapshot(),
          counts: f.host.counts(),
          qualified: !!f.qualification(),
          rows: await f.host.stored()
        };
      });
      expect(after.status).toBe('stopped');
      expect(after.snapshot?.busy).toBe(false);
      expect(after.qualified).toBe(false);
      expect(after.counts).toEqual(during.counts);
      const oldFacts = seed ? probeRow(seed.row.wire).receipts.length : 0;
      expect(
        probeRow(after.rows[0].wire)
          .receipts.slice(oldFacts)
          .some((fact) => fact.status === 'nonce_verified')
      ).toBe(false);
    });
  }
  test(`${lane} real held first key admits no reentry and Stop protects unfinished journal without a replacement artifact`, async ({
    page
  }) => {
    const seed =
      lane === 'recover' ? await seedRecovery(page, true) : undefined;
    const priorEvents = frames.filter((frame) => frame.type === 'EVENT').length;
    await page.evaluate(
      ({ lane, command }) => {
        const f = window.hcCheckFixture;
        f.host.mode('hold_key');
        f.select(lane, 'wss://inbox.example.org', command);
        window.hcCheckPending = f.run();
      },
      { lane, command: seed?.command }
    );
    await expect
      .poll(() => page.evaluate(() => window.hcCheckFixture.host.waiting()))
      .toBe('key');
    const during = await page.evaluate(
      async ({ lane, command }) => {
        const f = window.hcCheckFixture,
          api = window.hcCheck,
          before = f.host.counts(),
          consumed = f.review()!;
        const reentry = await api.runInboxAccessCheck(
          consumed,
          'reviewed_inbox_access_check'
        );
        const selection = f.select(lane, 'wss://inbox.example.org', command);
        f.stop();
        const snapshot = f.snapshot(),
          counts = f.host.counts();
        f.host.settle();
        return { before, counts, reentry: reentry.status, selection, snapshot };
      },
      { lane, command: seed?.command }
    );
    expect(during.reentry).toBe('invalid');
    expect(during.selection).toBeUndefined();
    expect(during.counts).toEqual(during.before);
    expect(during.snapshot?.state).toBe('stopped');
    expect(during.snapshot?.busy).toBe(true);
    const after = await page.evaluate(async () => {
      const f = window.hcCheckFixture,
        result = await window.hcCheckPending;
      f.host.mode('normal');
      return {
        status: result.status,
        counts: f.host.counts(),
        rows: await f.host.stored(),
        qualified: !!f.qualification()
      };
    });
    expect(after.status).toBe('stopped');
    expect(after.qualified).toBe(false);
    expect(after.counts).toEqual(during.counts);
    expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(
      priorEvents
    );
    if (seed) expect(after.rows).toEqual([seed.row]);
    else expect(probeRow(after.rows[0].wire).self).toBeNull();
  });
}

async function seedOrdinaryRow(page: Page, foreign = false) {
  return page.evaluate(async (foreign) => {
    const f = window.hcCheckFixture;
    const owner = foreign ? window.hcCheck.otherCheckOwner() : f.host.owner;
    const id = crypto.randomUUID(),
      peer = window.hcCheck.otherCheckOwner();
    if (owner === peer || (foreign && owner === f.host.owner))
      throw Error('actual distinct public fixture owners required');
    const wire = JSON.stringify({
      schema: 1,
      family: 'private_send_reservation',
      owner,
      id,
      revision: 0,
      peer,
      rumorHash: 'a'.repeat(64),
      createdAt: Math.floor(Date.now() / 1000)
    });
    if (!window.hcCheck.decodePrivateRecord(wire, owner, id).ok)
      throw Error('actual canonical schema1 ordinary reservation fixture');
    const row = { owner, id, wire };
    await f.host.inject([row]);
    return row;
  }, foreign);
}

for (const selected of ['absent', 'ordinary', 'foreign'] as const) {
  test(`cold recovery rejects an actual ${selected} stored selection without fallback construction or mutation`, async ({
    page
  }) => {
    const row =
      selected === 'absent'
        ? undefined
        : await seedOrdinaryRow(page, selected === 'foreign');
    const result = await page.evaluate(async (id) => {
      const f = window.hcCheckFixture,
        before = f.host.counts(),
        beforeRows = await f.host.stored();
      const command = id ?? crypto.randomUUID();
      f.select('recover', 'wss://inbox.example.org', command);
      const completed = await f.run();
      return {
        status: completed.status,
        before,
        after: f.host.counts(),
        beforeRows,
        afterRows: await f.host.stored(),
        qualified: !!f.qualification(),
        command: f.snapshot()?.command,
        selected: command
      };
    }, row?.id);
    expect(result.status).not.toBe('qualified');
    expect(result.qualified).toBe(false);
    expect(result.after).toEqual(result.before);
    expect(result.afterRows).toEqual(result.beforeRows);
    expect(result.command).toBe(result.selected);
    expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(0);
  });
}

for (const lane of ['original', 'recover'] as const) {
  for (const cleanup of [false, true]) {
    test(`${lane} ${cleanup ? 'reviewed optional cleanup' : 'qualification'} preserves exact preexisting schema1 ordinary reservation bytes`, async ({
      page
    }) => {
      const row = await seedOrdinaryRow(page);
      const seed =
        lane === 'recover' ? await seedRecovery(page, true) : undefined;
      const result = await page.evaluate(
        async ({ lane, command, cleanup, id }) => {
          const f = window.hcCheckFixture;
          f.select(lane, 'wss://inbox.example.org', command);
          const completed = await f.run();
          let removed: string | undefined;
          if (cleanup) {
            f.selectCleanup();
            removed = (await f.cleanup()).status;
          }
          const rows = await f.host.stored();
          return {
            status: completed.status,
            removed,
            ordinary: rows.find((row) => row.id === id),
            rows: rows.length,
            qualified: !!f.qualification()
          };
        },
        { lane, command: seed?.command, cleanup, id: row.id }
      );
      expect(result.status).toBe('qualified');
      if (cleanup) expect(result.removed).toBe('removed');
      expect(result.qualified).toBe(true);
      expect(result.ordinary).toEqual(row);
      expect(result.rows).toBe(cleanup ? 1 : 2);
    });
  }
}

test('mismatched genuine same-owner identity private-session and C7 tuples cannot authorize effects or close either scope', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.hcCheckFixture,
      api = window.hcCheck,
      original = f.actors(),
      other = await f.alternateActors();
    const before = f.host.counts(),
      closes = window.hcCheckCloseCalls;
    try {
      const attempts = [
        api.createInboxAccessCheck(
          f.repository,
          other.identity,
          original.session,
          f.host.policy,
          original.access,
          f.host.qualification
        ),
        api.createInboxAccessCheck(
          f.repository,
          original.identity,
          other.session,
          f.host.policy,
          other.access,
          f.host.qualification
        ),
        api.createInboxAccessCheck(
          f.repository,
          other.identity,
          original.session,
          f.host.policy,
          other.access,
          f.host.qualification
        ),
        api.createInboxAccessCheck(
          f.repository,
          original.identity,
          other.session,
          f.host.policy,
          original.access,
          f.host.qualification
        )
      ];
      return {
        attempts: attempts.map(Boolean),
        before,
        after: f.host.counts(),
        closes,
        afterCloses: window.hcCheckCloseCalls,
        original: api.inboxAccessControllerSourceSnapshot(original.access),
        other: api.inboxAccessControllerSourceSnapshot(other.access),
        same:
          api.createInboxAccessCheck(
            f.repository,
            original.identity,
            original.session,
            f.host.policy,
            original.access,
            f.host.qualification
          ) === f.check(),
        rows: await f.host.stored()
      };
    } finally {
      other.close();
    }
  });
  expect(result.attempts).toEqual([false, false, false, false]);
  expect(result.after).toEqual(result.before);
  expect(result.afterCloses).toBe(result.closes);
  expect(result.original?.state).not.toBe('stopped');
  expect(result.other?.state).not.toBe('stopped');
  expect(result.same).toBe(true);
  expect(result.rows).toHaveLength(0);
  expect(
    frames.filter((frame) => ['EVENT', 'AUTH'].includes(frame.type))
  ).toHaveLength(0);
});

test('coordinator Stop preserves a genuine alternate actor failed AUTH cleanup for its own explicit retry', async ({
  page
}) => {
  await page.evaluate(async () => {
    const f = window.hcCheckFixture;
    window.hcCheckOther = await f.alternateActors();
    await window.hcCheck.authenticateInboxAccess(
      window.hcCheckOther.access,
      'wss://inbox.example.org',
      'reviewed_inbox_connection_auth'
    );
  });
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.hcCheck
            .inboxAccessControllerSourceSnapshot(window.hcCheckOther!.access)
            ?.auth.find((row) => row.origin === 'wss://inbox.example.org')
            ?.state
      )
    )
    .toBe('waiting');
  try {
    const result = await page.evaluate(() => {
      const f = window.hcCheckFixture,
        api = window.hcCheck,
        other = window.hcCheckOther!;
      window.hcCheckCloseMode = 'fail';
      const foreignClosed = api.closeInboxAccessController(other.access),
        closes = window.hcCheckCloseCalls;
      const bad = api.createInboxAccessCheck(
        f.repository,
        f.actors().identity,
        other.session,
        f.host.policy,
        other.access,
        f.host.qualification
      );
      const ownStopped = f.stop();
      return {
        foreignClosed,
        bad: !!bad,
        closes,
        afterCloses: window.hcCheckCloseCalls,
        ownStopped,
        foreignSnapshot: api.inboxAccessControllerSourceSnapshot(other.access)
      };
    });
    expect(result.foreignClosed).toBe(false);
    expect(result.bad).toBe(false);
    expect(result.ownStopped).toBe(true);
    expect(result.afterCloses).toBe(result.closes);
    expect(result.foreignSnapshot?.cleanupRequired).toBe(true);
    expect(
      [...physical].some(
        (socket) =>
          physicalOrigins.get(socket) !== 'wss://discovery.example.org'
      )
    ).toBe(true);
  } finally {
    await page.evaluate(() => {
      window.hcCheckCloseMode = undefined;
      window.hcCheckOther!.close();
    });
    await expect
      .poll(
        () =>
          [...physical].filter(
            (socket) =>
              physicalOrigins.get(socket) !== 'wss://discovery.example.org'
          ).length
      )
      .toBe(0);
    expect(await page.evaluate(() => window.hcCheckOther!.close())).toBe(true);
  }
});

for (const lane of ['original', 'recover'] as const) {
  for (const phase of ['append', 'nonce', 'qualification'] as const) {
    for (const loss of ['head', 'session', 'C7'] as const) {
      test(`${lane} actual native ${phase} callback ${loss} loss revokes before subsequent effects and preserves admitted bookkeeping`, async ({
        page
      }) => {
        const seed =
          lane === 'recover' ? await seedRecovery(page, true) : undefined;
        const result = await page.evaluate(
          async ({ lane, command, phase, loss }) => {
            const f = window.hcCheckFixture,
              api = window.hcCheck;
            let atLoss: ReturnType<typeof f.snapshot> | undefined,
              counts: ReturnType<typeof f.host.counts> | undefined;
            const hook = api.checkNativeLoss(phase, () => {
              if (loss === 'head') f.actors().churnHead();
              else if (loss === 'session')
                api.closePrivateSession(f.actors().session);
              else api.closeInboxAccessController(f.actors().access);
              atLoss = f.snapshot();
              counts = f.host.counts();
            });
            try {
              f.select(lane, 'wss://inbox.example.org', command);
              const completed = await f.run(),
                rows = await f.host.stored();
              return {
                hits: hook.hits(),
                status: completed.status,
                atLoss,
                counts,
                after: f.host.counts(),
                qualified: !!f.qualification(),
                snapshot: f.snapshot(),
                rows
              };
            } finally {
              hook.restore();
            }
          },
          { lane, command: seed?.command, phase, loss }
        );
        expect(result.hits).toBe(1);
        expect(result.atLoss?.state).toBe('stopped');
        expect(result.atLoss?.busy).toBe(true);
        expect(result.status).toBe('stopped');
        expect(result.qualified).toBe(false);
        expect(result.snapshot?.state).toBe('stopped');
        expect(result.after).toEqual(result.counts);
        expect(result.rows).toHaveLength(1);
        const row = probeRow(result.rows[0].wire),
          oldCount = seed ? probeRow(seed.row.wire).receipts.length : 0;
        expect(
          row.receipts
            .slice(oldCount)
            .filter((fact) => fact.status === 'accepted')
        ).toHaveLength(phase === 'append' ? 1 : 3);
        if (seed) expect(row.self).toEqual(probeRow(seed.row.wire).self);
        expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(
          (seed ? 3 : 0) + (phase === 'append' ? 1 : 3)
        );
      });
    }
  }
}

for (const mode of ['readback', 'uncertain_readback'] as const) {
  test(`actual durable R0 ${mode} uncertainty protects one original reservation and explicit Retry creates no replacement identity`, async ({
    page
  }) => {
    const result = await page.evaluate(async (mode) => {
      const f = window.hcCheckFixture,
        api = window.hcCheck,
        hook = api.failCheckPreparationNative('R0', mode);
      const uuid = crypto.randomUUID.bind(crypto);
      let uuids = 0;
      crypto.randomUUID = () => {
        uuids++;
        return uuid();
      };
      try {
        f.select();
        const first = await f.run(),
          firstRows = await f.host.stored(),
          counts = hook.counts(),
          firstUuids = uuids,
          before = f.host.counts();
        hook.restore();
        f.select();
        const second = await f.run(),
          rows = await f.host.stored();
        return {
          first: first.status,
          second: second.status,
          counts,
          firstRows,
          rows,
          firstUuids,
          uuids,
          before,
          after: f.host.counts(),
          qualified: !!f.qualification(),
          candidate: hook.candidate()
        };
      } finally {
        hook.restore();
        crypto.randomUUID = uuid;
      }
    }, mode);
    expect(result.counts.puts).toBe(1);
    expect(result.counts.completes).toBe(1);
    expect(result.counts.readbackAborts).toBe(mode === 'readback' ? 1 : 0);
    if (mode === 'uncertain_readback')
      expect(result.counts.abortRefusals).toBeGreaterThan(0);
    expect(result.first).not.toBe('qualified');
    expect(result.second).not.toBe('qualified');
    expect(result.firstRows).toHaveLength(1);
    expect(result.rows).toEqual(result.firstRows);
    expect(result.rows[0]).toEqual(result.candidate);
    expect(result.uuids).toBe(result.firstUuids);
    expect(result.after).toEqual(result.before);
    expect(result.qualified).toBe(false);
    expect(probeRow(result.rows[0].wire).self).toBeNull();
    expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(0);
  });
}

for (const mode of ['abort', 'readback', 'uncertain_readback'] as const) {
  test(`actual R1 ${mode} uncertainty sends no EVENT before genuine ACK and explicit Retry reuses exact original wrap and command`, async ({
    page
  }) => {
    const first = await page.evaluate(async (mode) => {
      const f = window.hcCheckFixture,
        hook = window.hcCheck.failCheckPreparationNative('R1', mode);
      try {
        f.select();
        const completed = await f.run(),
          rows = await f.host.stored();
        return {
          status: completed.status,
          counts: hook.counts(),
          candidate: hook.candidate(),
          command: f.snapshot()?.command,
          before: f.host.counts(),
          rows,
          qualified: !!f.qualification()
        };
      } finally {
        hook.restore();
      }
    }, mode);
    expect(first.status).not.toBe('qualified');
    expect(first.qualified).toBe(false);
    expect(first.counts.puts).toBe(1);
    expect(first.rows).toHaveLength(1);
    expect(first.command).toBe(first.candidate?.id);
    expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(0);
    if (mode === 'abort') {
      expect(first.counts.aborts).toBe(1);
      expect(probeRow(first.rows[0].wire).self).toBeNull();
    } else {
      expect(first.counts.completes).toBe(1);
      expect(first.counts.readbackAborts).toBe(1);
      expect(first.rows[0]).toEqual(first.candidate);
    }
    if (mode === 'uncertain_readback')
      expect(first.counts.abortRefusals).toBeGreaterThan(0);
    const second = await page.evaluate(async () => {
      const f = window.hcCheckFixture;
      f.select();
      const completed = await f.run();
      return {
        status: completed.status,
        after: f.host.counts(),
        command: f.snapshot()?.command,
        rows: await f.host.stored(),
        qualified: !!f.qualification()
      };
    });
    expect(second.status).toBe('qualified');
    expect(second.qualified).toBe(true);
    expect(second.command).toBe(first.command);
    expect(second.after.signs).toBe(first.before.signs);
    expect(second.after.encrypts).toBe(first.before.encrypts);
    expect(probeRow(second.rows[0].wire).self).toEqual(
      probeRow(first.candidate!.wire).self
    );
    expect(frames.filter((frame) => frame.type === 'EVENT')).toHaveLength(3);
    expect(
      new Set(
        frames
          .filter((frame) => frame.type === 'EVENT')
          .map((frame) => frame.id)
      )
    ).toEqual(new Set([probeRow(first.candidate!.wire).self!.eventId]));
  });
}
