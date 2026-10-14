import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-probe-publication.ts';
declare global {
  interface Window {
    pb: typeof Fixture;
    pbFixture: Awaited<ReturnType<typeof Fixture.makePublicationFixture>>;
    pbPending?: Promise<unknown>;
    pbAbort?: AbortController;
    pbOtherPool?: Fixture.PrivatePool;
    pbRestore?: () => void;
    pbClock?: number;
    pbNetworkClock?: ReturnType<Window['pbFixture']['installNetworkClock']>;
    pbDelayClose?: boolean;
    pbFailClose?: boolean;
    pbInvalidateOnSocket?: boolean;
    pbNestedCleanup?: number;
    pbNestedFailure?: number;
    pbHeldClose?: (() => void)[];
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let head = '',
  response: 'accepted' | 'refused' | 'unknown' | 'wrong' | 'auth_required' =
    'accepted',
  auth = false;
const frames: { origin: string; frame: unknown[] }[] = [],
  physical = new Set<WebSocket>();
let peak = 0,
  privatePeak = 0;
const privatePhysical = new Set<WebSocket>();
const privateConnections = new Map<string, number>();
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
    throw Error('actual loopback fixture');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket, request) => {
    physical.add(socket);
    peak = Math.max(peak, physical.size);
    socket.on('close', () => physical.delete(socket));
    const origin = new URL(request.url!, 'http://127.0.0.1').searchParams.get(
      'origin'
    )!;
    if (origin !== 'wss://discovery.example.org') {
      privatePhysical.add(socket);
      privateConnections.set(origin, (privateConnections.get(origin) ?? 0) + 1);
      privatePeak = Math.max(privatePeak, privatePhysical.size);
      socket.on('close', () => privatePhysical.delete(socket));
    }
    if (auth && origin === 'wss://inbox.example.org')
      socket.send(JSON.stringify(['AUTH', 'probe-challenge']));
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
      frames.push({ origin, frame });
      if (frame[0] === 'REQ') {
        const filter = frame[2] as { kinds?: number[] };
        if (filter.kinds?.includes(10050) && head)
          socket.send(JSON.stringify(['EVENT', frame[1], JSON.parse(head)]));
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      } else if (frame[0] === 'AUTH') {
        socket.send(
          JSON.stringify(['OK', (frame[1] as { id: string }).id, true, ''])
        );
      } else if (frame[0] === 'EVENT' && response !== 'unknown') {
        socket.send(
          JSON.stringify([
            'OK',
            response === 'wrong'
              ? 'f'.repeat(64)
              : (frame[1] as { id: string }).id,
            response !== 'refused' && response !== 'auth_required',
            response === 'auth_required'
              ? 'auth-required: source fixture'
              : response === 'refused'
                ? 'blocked: source fixture'
                : ''
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
          new URL('./harness/inbox-probe-publication.ts', import.meta.url)
        ),
        name: 'pb',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('compiled fixture');
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
  head = '';
  response = 'accepted';
  auth = false;
  peak = 0;
  privatePeak = 0;
  privateConnections.clear();
  frames.length = 0;
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          const origin = new URL(String(url)).origin;
          if (
            ![
              'wss://discovery.example.org',
              'wss://inbox.example.org',
              'wss://peer.example.org',
              'wss://archive.example.org'
            ].includes(origin)
          )
            throw Error('unapproved fixture origin');
          super(endpoint + '/?origin=' + encodeURIComponent(origin), protocols);
          if (
            window.pbInvalidateOnSocket &&
            origin === 'wss://inbox.example.org'
          )
            window.pb.closeInboxAccessController(
              window.pbFixture.controller()!
            );
        }
        override close(code?: number, reason?: string) {
          if (window.pbFailClose)
            throw Error('actual controlled disposal failure');
          if (window.pbDelayClose)
            (window.pbHeldClose ??= []).push(() => super.close(code, reason));
          else super.close(code, reason);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(async (multi) => {
    window.pbFixture = await window.pb.makePublicationFixture();
    return window.pbFixture.preferenceWire(
      multi
        ? [
            'wss://inbox.example.org',
            'wss://peer.example.org',
            'wss://archive.example.org'
          ]
        : ['wss://inbox.example.org']
    );
  }, testInfo.title.includes('three-origin'));
  await page.evaluate(async () => {
    const f = window.pbFixture;
    await f.capture();
    if (
      (await f.reserve()).status !== 'reserved' ||
      (await f.prepare()) !== 'wrapped' ||
      (await f.commitOriginal()).status !== 'prepared'
    )
      throw Error('genuine cold native original setup');
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => {
      window.pbRestore?.();
      window.pbFailClose = false;
      window.pbDelayClose = false;
      for (const release of window.pbHeldClose ?? []) release();
      window.pbFixture.close();
    });
});
function privateEvents() {
  return frames.filter(
    (x) => x.origin !== 'wss://discovery.example.org' && x.frame[0] === 'EVENT'
  );
}
function noPrivateReads() {
  expect(
    frames.filter(
      (x) => x.origin !== 'wss://discovery.example.org' && x.frame[0] === 'REQ'
    )
  ).toHaveLength(0);
}

test('cold original artifact is published exactly; genuine receipts and new ACKs keep one action and three attempts', async ({
  page
}) => {
  const before = await page.evaluate(() => window.pbFixture.stored());
  expect(privateEvents()).toHaveLength(0);
  noPrivateReads();
  const observed = await page.evaluate(async () => {
    const f = window.pbFixture,
      api = window.pb,
      action = f.action()!;
    const first = await f.publish();
    const clone = await f.append({ ...first });
    const saved = await f.append(first),
      duplicate = await f.append(first);
    const afterFirst = f.snapshot()!.record;
    const recommit = await f.commitOriginal(),
      sameAction = f.action() === action;
    const second = await f.publish();
    await f.append(second);
    const third = await f.publish();
    await f.append(third);
    const fourth = await f.publish();
    return {
      first,
      clone,
      saved,
      duplicate,
      afterFirst,
      recommit: recommit.status,
      sameAction,
      second,
      third,
      fourth,
      row: f.snapshot()!.record,
      evidence: !!api.inboxProbeArchiveAttemptEvidence(first),
      ordinaryEvidence: !!api.privateGiftWrapAttemptEvidence(
        first as unknown as Parameters<
          typeof api.privateGiftWrapAttemptEvidence
        >[0]
      ),
      ready: api.inboxAccessControllerSnapshot(f.controller()!)?.readiness
    };
  });
  expect(observed.first).toMatchObject({
    status: 'accepted',
    role: 'access_archive',
    origin: 'wss://inbox.example.org',
    attempt: 1
  });
  expect(observed.clone.status).toBe('invalid');
  expect(observed.saved.status).toBe('saved');
  expect(observed.duplicate.status).toBe('existing');
  expect(observed.afterFirst.revision).toBe(2);
  expect(observed.afterFirst.receipts).toHaveLength(1);
  expect(observed.recommit).toBe('existing');
  expect(observed.sameAction).toBe(true);
  expect(observed.second).toMatchObject({
    status: 'accepted',
    actionId: observed.first.actionId,
    attempt: 2
  });
  expect(observed.third).toMatchObject({
    status: 'accepted',
    actionId: observed.first.actionId,
    attempt: 3
  });
  expect(observed.fourth).toEqual({ status: 'stopped' });
  expect(observed.row.revision).toBe(4);
  expect(observed.row.receipts).toHaveLength(3);
  expect(observed.evidence).toBe(true);
  expect(observed.ordinaryEvidence).toBe(false);
  expect(observed.ready).toBe('unavailable');
  expect(privateEvents()).toHaveLength(3);
  expect(privateEvents().map((x) => JSON.stringify(x.frame[1]))).toEqual(
    Array(3).fill(
      (JSON.parse(before[0].wire) as { self: { wire: string } }).self.wire
    )
  );
  noPrivateReads();
});

test('forged/cross-family permission and genuine foreign repository/session/policy/head cannot send', async ({
  page
}) => {
  const observations = await page.evaluate(async () => {
    const f = window.pbFixture,
      api = window.pb,
      ack = f.acknowledgement()!,
      action = f.action()!,
      permission = f.permission()!;
    const copiedPolicy = f.copiedPolicy(),
      otherSession = await f.foreignSession();
    const invalidActions = [
      api.captureInboxProbePublicationAction(
        f.foreignRepository(),
        f.session,
        f.policy,
        ack,
        'reviewed_inbox_probe_network'
      ),
      api.captureInboxProbePublicationAction(
        f.repository,
        otherSession,
        f.policy,
        ack,
        'reviewed_inbox_probe_network'
      ),
      api.captureInboxProbePublicationAction(
        f.repository,
        f.session,
        copiedPolicy,
        ack,
        'reviewed_inbox_probe_network'
      ),
      api.captureInboxProbePublicationAction(
        f.repository,
        f.session,
        f.policy,
        { ...f.snapshot() } as typeof ack,
        'reviewed_inbox_probe_network'
      )
    ].map(Boolean);
    const invalidPermissions = [
      api.captureInboxProbePublication(
        f.repository,
        f.session,
        copiedPolicy,
        ack,
        action,
        'wss://inbox.example.org',
        'reviewed_inbox_probe_publication'
      ),
      f.permission('wss://peer.example.org'),
      f.permission('wss://inbox.example.org', {})
    ].map(Boolean);
    const cross = await api.publishPrivateGiftWrapAttempt(
      f.pool(),
      permission as unknown as Parameters<
        typeof api.publishPrivateGiftWrapAttempt
      >[1],
      new AbortController().signal
    );
    const fake = await api.publishInboxProbeArchiveAttempt(
      f.pool(),
      {} as typeof permission,
      new AbortController().signal
    );
    f.churnHead();
    return {
      invalidActions,
      invalidPermissions,
      cross,
      fake,
      stale: await f.publish(permission)
    };
  });
  expect(observations.invalidActions).toEqual([false, false, false, false]);
  expect(observations.invalidPermissions).toEqual([false, false, false]);
  expect(observations.cross).toEqual({ status: 'stopped' });
  expect(observations.fake).toEqual({ status: 'stopped' });
  expect(observations.stale).toEqual({ status: 'stopped' });
  expect(privateEvents()).toHaveLength(0);
  noPrivateReads();
});
for (const mutation of [
  'alias',
  'forged_fact',
  'corrupt_tail',
  'removed'
] as const) {
  test(`fresh native ${mutation} refuses before EVENT and cannot advance archive provenance`, async ({
    page
  }) => {
    const result = await page.evaluate(async (mutation) => {
      const f = window.pbFixture,
        api = window.pb,
        token = f.permission()!;
      const rows = await f.stored();
      if (mutation === 'removed') await f.removeOriginal();
      else if (mutation === 'corrupt_tail')
        await f.inject([
          { owner: f.owner, id: crypto.randomUUID(), wire: 'bad' }
        ]);
      else {
        if (mutation === 'alias') rows[0].wire += ' ';
        else {
          const row = JSON.parse(rows[0].wire) as {
            revision: number;
            self: { eventId: string };
            receipts: unknown[];
          };
          row.revision++;
          row.receipts.push({
            actionId: crypto.randomUUID(),
            role: 'access_archive',
            origin: 'wss://inbox.example.org',
            eventId: row.self.eventId,
            attempt: 1,
            status: 'accepted',
            observedAtMilliseconds: 1,
            readbackWire: null
          });
          rows[0].wire = JSON.stringify(row);
        }
        await f.inject(rows);
      }
      const before = await f.stored(),
        result = await f.publish(token);
      return {
        before,
        after: await f.stored(),
        result,
        evidence: !!api.inboxProbeArchiveAttemptEvidence(result),
        recommit: await f.commitOriginal()
      };
    }, mutation);
    expect(result.result).toEqual({ status: 'stopped' });
    expect(result.evidence).toBe(false);
    expect(result.after).toEqual(result.before);
    expect(result.recommit.status).not.toBe('existing');
    expect(privateEvents()).toHaveLength(0);
    noPrivateReads();
  });
}

test('handwritten valid receipt after actual EVENT cannot be adopted by append or refreshed ACK', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.pbFixture,
      actual = await f.publish(),
      rows = await f.stored();
    const row = JSON.parse(rows[0].wire) as {
      revision: number;
      self: { eventId: string };
      receipts: unknown[];
    };
    row.revision++;
    row.receipts.push({
      actionId: crypto.randomUUID(),
      origin: 'wss://inbox.example.org',
      role: 'access_archive',
      eventId: row.self.eventId,
      attempt: 1,
      status: 'accepted',
      observedAtMilliseconds: 1,
      readbackWire: null
    });
    rows[0].wire = JSON.stringify(row);
    await f.inject(rows);
    return {
      append: await f.append(actual),
      recommit: await f.commitOriginal(),
      before: rows,
      after: await f.stored(),
      snapshot: f.snapshot()!.record
    };
  });
  expect(result.append.status).toBe('conflict');
  expect(result.recommit.status).toBe('conflict');
  expect(result.after).toEqual(result.before);
  expect(result.snapshot.receipts).toHaveLength(0);
  expect(privateEvents()).toHaveLength(1);
  noPrivateReads();
});

test('actual named relay refusal remains a genuine bounded archive observation', async ({
  page
}) => {
  response = 'refused';
  const result = await page.evaluate(async () => {
    const f = window.pbFixture,
      actual = await f.publish();
    return {
      actual,
      append: await f.append(actual),
      row: f.snapshot()!.record
    };
  });
  expect(result.actual).toMatchObject({ status: 'refused', attempt: 1 });
  expect(result.append.status).toBe('saved');
  expect(result.row.receipts[0].status).toBe('refused');
  expect(privateEvents()).toHaveLength(1);
  noPrivateReads();
});
for (const disposition of ['stop', 'wrong_ack', 'clock_expired'] as const) {
  test(`actual EVENT ${disposition} is truthful and cannot become acceptance`, async ({
    page
  }) => {
    response = disposition === 'wrong_ack' ? 'wrong' : 'unknown';
    await page.evaluate((disposition) => {
      const f = window.pbFixture;
      if (disposition === 'clock_expired') {
        window.pbNetworkClock = f.installNetworkClock();
        window.pbRestore = () => window.pbNetworkClock!.restore();
      }
      window.pbAbort = new AbortController();
      window.pbPending = f.publish(f.permission(), window.pbAbort.signal);
    }, disposition);
    await expect.poll(() => privateEvents().length).toBe(1);
    if (disposition === 'clock_expired')
      await page.evaluate(() => {
        window.pbNetworkClock!.advance(46000);
      });
    else await page.evaluate(() => window.pbAbort!.abort());
    const result = await page.evaluate(async () => {
      const actual = (await window.pbPending) as NonNullable<
        ReturnType<typeof window.pbFixture.last>
      >;
      return {
        actual,
        append: await window.pbFixture.append(actual),
        row: window.pbFixture.snapshot()!.record
      };
    });
    expect(result.actual.status).toBe(
      disposition === 'clock_expired' ? 'timed_out' : 'stopped'
    );
    expect(result.append.status).toBe('saved');
    expect(result.row.receipts[0].status).toBe(result.actual.status);
    noPrivateReads();
  });
}

test('pending fresh owner approval and SDK acquisition failure consume no attempt; original starts at one', async ({
  page
}) => {
  const stopped = await page.evaluate(async () => {
    const f = window.pbFixture;
    f.pool();
    const restore = f.installAcquisitionFailure();
    try {
      return await f.publish();
    } finally {
      restore();
    }
  });
  expect(stopped).toEqual({ status: 'stopped' });
  expect(privateEvents()).toHaveLength(0);
  await page.evaluate(() => {
    const f = window.pbFixture;
    window.pbNetworkClock = f.installNetworkClock();
    window.pbRestore = () => window.pbNetworkClock!.restore();
    f.mode('hold_key');
    window.pbPending = f.publish();
  });
  await expect
    .poll(() => page.evaluate(() => window.pbFixture.waiting()))
    .toBe('key');
  expect(privateEvents()).toHaveLength(0);
  await page.evaluate(() => {
    window.pbNetworkClock!.advance(90000);
    window.pbFixture.mode('normal');
    window.pbFixture.settle();
  });
  const first = await page.evaluate(async () => window.pbPending);
  expect(first).toMatchObject({ status: 'accepted', attempt: 1 });
  const next = await page.evaluate(async () => {
    await window.pbFixture.append();
    return window.pbFixture.publish();
  });
  expect(next).toMatchObject({ status: 'accepted', attempt: 2 });
  expect(privateEvents()).toHaveLength(2);
  noPrivateReads();
});

test('known AUTH with pending original signer consumes no probe attempt, then only original artifact sends', async ({
  page
}) => {
  auth = true;
  await page.evaluate(() => {
    const f = window.pbFixture;
    window.pbNetworkClock = f.installNetworkClock();
    window.pbRestore = () => window.pbNetworkClock!.restore();
    window.pbPending = window.pb.authenticateInboxAccess(
      f.controller()!,
      'wss://inbox.example.org',
      'reviewed_inbox_connection_auth'
    );
  });
  await page.evaluate(async () => window.pbPending);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.pb.inboxAccessControllerSnapshot(
            window.pbFixture.controller()!
          )?.auth[0]?.state
      )
    )
    .toBe('challenge');
  await page.evaluate(() => {
    const f = window.pbFixture;
    f.mode('hold_sign');
    window.pbPending = window.pb.respondInboxAccessAuthentication(
      f.controller()!,
      'wss://inbox.example.org',
      'reviewed_inbox_auth_response'
    );
  });
  await expect
    .poll(() => page.evaluate(() => window.pbFixture.waiting()))
    .toBe('sign');
  const stopped = await page.evaluate(() => window.pbFixture.publish());
  expect(stopped).toEqual({ status: 'stopped' });
  expect(privateEvents()).toHaveLength(0);
  expect(
    await page.evaluate(() => window.pbFixture.snapshot()!.record.receipts)
  ).toHaveLength(0);
  await page.evaluate(() => {
    window.pbNetworkClock!.advance(90000);
    window.pbFixture.mode('normal');
    window.pbFixture.settle();
  });
  expect(await page.evaluate(async () => window.pbPending)).toBe('accepted');
  const original = await page.evaluate(() => window.pbFixture.publish());
  expect(original).toMatchObject({ status: 'accepted', attempt: 1 });
  expect(privateEvents()).toHaveLength(1);
  expect(frames.filter((x) => x.frame[0] === 'AUTH')).toHaveLength(1);
  noPrivateReads();
});

for (const mode of ['aborted', 'uncertain'] as const) {
  test(`actual native archive ${mode} commit retains original evidence for exact replay`, async ({
    page
  }) => {
    const result = await page.evaluate(async (mode) => {
      const f = window.pbFixture,
        actual = await f.publish(),
        before = await f.stored();
      const uncertain =
        mode === 'uncertain' ? f.installUncertainCommit() : undefined;
      const restore =
        mode === 'aborted'
          ? f.installNativeAbort()
          : () => uncertain!.restore();
      let first: Awaited<ReturnType<typeof f.append>>;
      try {
        first = await f.append(actual);
      } finally {
        restore();
      }
      const afterFirst = await f.stored(),
        second = await f.append(actual),
        third = await f.append(actual);
      return {
        before,
        afterFirst,
        first,
        second,
        third,
        row: f.snapshot()!.record,
        native: uncertain?.counts()
      };
    }, mode);
    expect(result.first.status).toBe(
      mode === 'aborted' ? 'aborted' : 'reconciled'
    );
    if (mode === 'aborted') expect(result.afterFirst).toEqual(result.before);
    expect(result.second.status).toBe(
      mode === 'aborted' ? 'saved' : 'existing'
    );
    expect(result.third.status).toBe('existing');
    expect(result.row.revision).toBe(2);
    expect(result.row.receipts).toHaveLength(1);
    expect(privateEvents()).toHaveLength(1);
    noPrivateReads();
    if (mode === 'uncertain')
      expect(result.native).toMatchObject({
        putSuccess: 1,
        nativeComplete: 1,
        nativeAbort: 0,
        abortRefused: 1
      });
  });
}

test('original attempt metadata settles after owner loss, without restoring effect or readiness', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.pbFixture,
      actual = await f.publish();
    f.closeSession();
    const appended = await f.append(actual);
    return {
      actual,
      appended,
      rows: await f.stored(),
      ack: f.snapshot(),
      next: await f.publish()
    };
  });
  expect(result.appended.status).toBe('saved');
  expect(
    (JSON.parse(result.rows[0].wire) as { receipts: unknown[] }).receipts
  ).toHaveLength(1);
  expect(result.ack).toBeUndefined();
  expect(result.next).toEqual({ status: 'stopped' });
  expect(privateEvents()).toHaveLength(1);
  noPrivateReads();
});

test('held physical close retains original cleanup and prevents replacement traffic', async ({
  page
}) => {
  response = 'unknown';
  await page.evaluate(() => {
    window.pbDelayClose = true;
    window.pbAbort = new AbortController();
    window.pbPending = window.pbFixture.publish(
      window.pbFixture.permission(),
      window.pbAbort.signal
    );
  });
  await expect.poll(() => privateEvents().length).toBe(1);
  await page.evaluate(() => {
    window.pbAbort!.abort();
    window.pbFixture.stop();
  });
  expect(await page.evaluate(() => window.pbFixture.publish())).toEqual({
    status: 'stopped'
  });
  expect(privateEvents()).toHaveLength(1);
  expect(peak).toBeLessThanOrEqual(3);
  await page.evaluate(() => {
    window.pbDelayClose = false;
    for (const release of window.pbHeldClose ?? []) release();
    window.pbFixture.stop();
  });
  await expect.poll(() => physical.size).toBe(0);
  noPrivateReads();
});

test('reentrant genuine UUID capture reserves one original action before callbacks', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.pbFixture,
      original = crypto.randomUUID.bind(crypto);
    let nested: ReturnType<typeof f.action>,
      entered = false;
    crypto.randomUUID = () => {
      if (!entered) {
        entered = true;
        nested = f.action();
      }
      return original();
    };
    let first: ReturnType<typeof f.action>;
    try {
      first = f.action();
    } finally {
      crypto.randomUUID = original;
    }
    const outcomes = [];
    for (let n = 0; n < 4; n++) {
      const actual = await f.publish();
      outcomes.push(actual);
      if (n < 3) await f.append(actual);
    }
    return {
      first: !!first,
      nested: !!nested,
      same: first === f.action(),
      outcomes
    };
  });
  expect(result.first).toBe(true);
  expect(result.nested).toBe(false);
  expect(result.same).toBe(true);
  expect(result.outcomes.slice(0, 3).map((x) => x.attempt)).toEqual([1, 2, 3]);
  expect(result.outcomes[3]).toEqual({ status: 'stopped' });
  expect(privateEvents()).toHaveLength(3);
  noPrivateReads();
});

for (const boundary of ['wrong_key', 'head_during_key', 'lock_busy'] as const) {
  test(`actual ${boundary} refuses with no EVENT or attempted archive receipt`, async ({
    page
  }) => {
    if (boundary === 'lock_busy') {
      await page.evaluate(() => {
        window.pbPending = navigator.locks.request(
          'harvestcircle:owner:' + window.pbFixture.owner,
          async () => {
            window.pbClock = 1;
            await new Promise<void>((resolve) => {
              window.pbRestore = resolve;
            });
          }
        );
      });
      await expect.poll(() => page.evaluate(() => window.pbClock)).toBe(1);
    } else {
      await page.evaluate((boundary) => {
        window.pbFixture.mode(
          boundary === 'wrong_key' ? 'wrong_key' : 'hold_key'
        );
      }, boundary);
    }
    await page.evaluate(() => {
      window.pbPending = window.pbFixture.publish();
    });
    if (boundary === 'head_during_key') {
      await expect
        .poll(() => page.evaluate(() => window.pbFixture.waiting()))
        .toBe('key');
      await page.evaluate(() => {
        window.pbFixture.churnHead();
        window.pbFixture.mode('normal');
        window.pbFixture.settle();
      });
    }
    const result = await page.evaluate(async () => {
      const actual = (await window.pbPending) as NonNullable<
        ReturnType<typeof window.pbFixture.last>
      >;
      return {
        actual,
        evidence: !!window.pb.inboxProbeArchiveAttemptEvidence(actual),
        rows: await window.pbFixture.stored()
      };
    });
    expect(result.actual.status).toBe('stopped');
    expect(result.evidence).toBe(false);
    expect(
      (JSON.parse(result.rows[0].wire) as { receipts: unknown[] }).receipts
    ).toHaveLength(0);
    expect(privateEvents()).toHaveLength(0);
    noPrivateReads();
    if (boundary === 'lock_busy')
      await page.evaluate(() => window.pbRestore?.());
  });
}
for (const clock of ['backwards', 'nonfinite'] as const) {
  test(`actual ${clock} network clock conservatively fences original action`, async ({
    page
  }) => {
    response = 'unknown';
    await page.evaluate(() => {
      window.pbNetworkClock = window.pbFixture.installNetworkClock();
      window.pbRestore = () => window.pbNetworkClock!.restore();
      window.pbPending = window.pbFixture.publish();
    });
    await expect.poll(() => privateEvents().length).toBe(1);
    await page.evaluate((clock) => {
      window.pbNetworkClock!.set(clock === 'backwards' ? 0 : NaN);
    }, clock);
    const result = await page.evaluate(async () => {
      const actual = (await window.pbPending) as NonNullable<
        ReturnType<typeof window.pbFixture.last>
      >;
      const appended = await window.pbFixture.append(actual);
      return {
        actual,
        appended,
        action: !!window.pbFixture.action(),
        next: await window.pbFixture.publish()
      };
    });
    expect(result.actual.status).toBe('timed_out');
    expect(result.appended.status).toBe('saved');
    expect(result.action).toBe(false);
    expect(result.next).toEqual({ status: 'stopped' });
    expect(privateEvents()).toHaveLength(1);
    noPrivateReads();
  });
}

test('actual late acknowledgement after head loss cannot become accepted', async ({
  page
}) => {
  response = 'unknown';
  await page.evaluate(() => {
    window.pbPending = window.pbFixture.publish();
  });
  await expect.poll(() => privateEvents().length).toBe(1);
  await page.evaluate(() => window.pbFixture.churnHead());
  const result = await page.evaluate(async () => window.pbPending);
  expect(result).toMatchObject({ status: 'stopped', attempt: 1 });
  for (const socket of sockets.clients)
    if (socket.readyState === 1)
      socket.send(
        JSON.stringify([
          'OK',
          (privateEvents()[0].frame[1] as { id: string }).id,
          true,
          'late'
        ])
      );
  const settlement = await page.evaluate(async () => {
    const f = window.pbFixture;
    return {
      append: await f.append(),
      row: (await f.stored())[0].wire,
      next: await f.publish()
    };
  });
  expect(settlement.append.status).toBe('saved');
  expect(
    (JSON.parse(settlement.row) as { receipts: { status: string }[] })
      .receipts[0].status
  ).toBe('stopped');
  expect(settlement.next).toEqual({ status: 'stopped' });
  noPrivateReads();
});

test('later physical disposal failure preserves prior stopped attempt and original Stop retries cleanup', async ({
  page
}) => {
  response = 'unknown';
  await page.evaluate(() => {
    window.pbAbort = new AbortController();
    window.pbPending = window.pbFixture.publish(
      window.pbFixture.permission(),
      window.pbAbort.signal
    );
  });
  await expect.poll(() => privateEvents().length).toBe(1);
  const result = await page.evaluate(async () => {
    window.pbFailClose = true;
    window.pbAbort!.abort();
    let failed = false;
    try {
      window.pbFixture.stop();
    } catch {
      failed = true;
    }
    const actual = await window.pbPending;
    const cleanup = window.pb.privatePoolCleanupRequired(
      window.pbFixture.pool()
    );
    window.pbFailClose = false;
    window.pbFixture.stop();
    return { actual, failed, cleanup, next: await window.pbFixture.publish() };
  });
  expect(result.actual).toMatchObject({ status: 'stopped', attempt: 1 });
  expect(result.failed).toBe(true);
  expect(result.cleanup).toBe(true);
  expect(result.next).toEqual({ status: 'stopped' });
  await expect.poll(() => physical.size).toBe(0);
  expect(privateEvents()).toHaveLength(1);
  noPrivateReads();
});

test('archive append remains eligible in a full hundred-row namespace; concurrent replay adds one fact', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.pbFixture,
      actual = await f.publish(),
      rows = await f.stored();
    const template = JSON.parse(rows[0].wire) as { id: string };
    const fillers = Array.from({ length: 99 }, () => {
      const id = crypto.randomUUID();
      return { owner: f.owner, id, wire: JSON.stringify({ ...template, id }) };
    });
    await f.inject(fillers);
    const pair = await Promise.all([f.append(actual), f.append(actual)]);
    const repeat = await f.append(actual);
    return {
      pair,
      repeat,
      row: f.snapshot()!.record,
      count: (await f.stored()).length
    };
  });
  expect(result.pair.map((x) => x.status).sort()).toEqual(['busy', 'saved']);
  expect(result.repeat.status).toBe('existing');
  expect(result.row.revision).toBe(2);
  expect(result.row.receipts).toHaveLength(1);
  expect(result.count).toBe(100);
  expect(privateEvents()).toHaveLength(1);
  noPrivateReads();
});

test('actual eight-MiB joint namespace refuses archive growth without losing original ciphertext or evidence', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.pbFixture,
      actual = await f.publish(),
      original = await f.stored();
    const template = JSON.parse(original[0].wire) as {
      id: string;
      self: { eventId: string };
      receipts: unknown[];
    };
    const id = crypto.randomUUID(),
      fact = {
        actionId: crypto.randomUUID(),
        origin: 'wss://inbox.example.org',
        role: 'access_archive',
        eventId: template.self.eventId,
        attempt: 1,
        status: 'accepted',
        observedAtMilliseconds: 1,
        readbackWire: null
      };
    const row = (count: number) => ({
      owner: f.owner,
      id,
      wire: JSON.stringify({
        ...template,
        id,
        receipts: Array.from({ length: count }, () => fact)
      })
    });
    const bytes = (rows: typeof original) =>
      rows.reduce(
        (sum, x) => sum + new TextEncoder().encode(JSON.stringify(x)).length,
        0
      );
    const empty = bytes([...original, row(0)]),
      step = bytes([row(1)]) - bytes([row(0)]),
      limit = 8 * 1024 * 1024;
    const count = Math.floor((limit - empty + 1) / (step + 1));
    await f.inject([row(count)]);
    const before = await f.stored(),
      append = await f.append(actual),
      after = await f.stored();
    return {
      append,
      exact: JSON.stringify(before) === JSON.stringify(after),
      headroom: limit - bytes(before),
      count: after.length,
      row: f.snapshot()!.record,
      evidence: !!window.pb.inboxProbeArchiveAttemptEvidence(actual)
    };
  });
  expect(result.headroom).toBeGreaterThanOrEqual(0);
  expect(result.headroom).toBeLessThan(350);
  expect(result.append.status).toBe('capacity');
  expect(result.exact).toBe(true);
  expect(result.count).toBe(2);
  expect(result.row.revision).toBe(1);
  expect(result.row.receipts).toHaveLength(0);
  expect(result.evidence).toBe(true);
  expect(privateEvents()).toHaveLength(1);
  noPrivateReads();
});

test('actual auth-required refusal without C7 setup prevents any subsequent probe key prompt or charge', async ({
  page
}) => {
  response = 'auth_required';
  auth = true;
  const first = await page.evaluate(() => window.pbFixture.publish());
  expect(first).toMatchObject({ status: 'refused', attempt: 1 });
  expect(privateEvents()).toHaveLength(1);
  const refused = await page.evaluate(async () => {
    const f = window.pbFixture;
    await f.append();
    const before = f.counts();
    f.mode('hold_key');
    const second = await f.publish();
    return { second, before, after: f.counts(), waiting: f.waiting() };
  });
  expect(refused.second).toEqual({ status: 'stopped' });
  expect(refused.after).toEqual(refused.before);
  expect(refused.waiting).toBe('');
  expect(privateEvents()).toHaveLength(1);
  response = 'accepted';
  await page.evaluate(() => {
    window.pbFixture.mode('normal');
    return window.pb.authenticateInboxAccess(
      window.pbFixture.controller()!,
      'wss://inbox.example.org',
      'reviewed_inbox_connection_auth'
    );
  });
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.pb.inboxAccessControllerSnapshot(
            window.pbFixture.controller()!
          )?.auth[0]?.state
      )
    )
    .toBe('challenge');
  expect(
    await page.evaluate(() =>
      window.pb.respondInboxAccessAuthentication(
        window.pbFixture.controller()!,
        'wss://inbox.example.org',
        'reviewed_inbox_auth_response'
      )
    )
  ).toBe('accepted');
  const next = await page.evaluate(() => window.pbFixture.publish());
  expect(next).toMatchObject({
    status: 'accepted',
    attempt: 2,
    actionId: first.actionId
  });
  expect(privateEvents()).toHaveLength(2);
  expect(frames.filter((x) => x.frame[0] === 'AUTH')).toHaveLength(1);
  noPrivateReads();
});

test('three-origin original action has three admissions per target, nine genuine facts and bounded held physical sockets', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.pbFixture,
      originalAction = f.action(),
      outcomes = [];
    for (const origin of [
      'wss://inbox.example.org',
      'wss://peer.example.org',
      'wss://archive.example.org'
    ]) {
      for (let n = 0; n < 3; n++) {
        const actual = await f.publish(f.permission(origin));
        outcomes.push(actual);
        await f.append(actual);
        await f.commitOriginal();
      }
      outcomes.push(await f.publish(f.permission(origin)));
    }
    return {
      outcomes,
      same: f.action() === originalAction,
      row: f.snapshot()!.record
    };
  });
  expect(result.same).toBe(true);
  expect(result.row.receipts).toHaveLength(9);
  expect(result.row.revision).toBe(10);
  const attempted = result.outcomes.filter((x) => x.actionId);
  expect(new Set(attempted.map((x) => x.actionId)).size).toBe(1);
  expect(attempted.map((x) => x.attempt)).toEqual([1, 2, 3, 1, 2, 3, 1, 2, 3]);
  expect(result.outcomes.filter((x) => !x.actionId)).toEqual(
    Array(3).fill({ status: 'stopped' })
  );
  expect(privateEvents()).toHaveLength(9);
  expect(privatePeak).toBe(3);
  expect(privatePhysical.size).toBe(3);
  noPrivateReads();
  await page.evaluate(() => {
    window.pbDelayClose = true;
    window.pbFixture.stop();
  });
  expect(privatePhysical.size).toBe(3);
  expect(
    await page.evaluate(() =>
      window.pb.privatePoolCleanupRequired(window.pbFixture.pool())
    )
  ).toBe(true);
  expect(
    await page.evaluate(() =>
      window.pbFixture.publish(
        window.pbFixture.permission('wss://peer.example.org')
      )
    )
  ).toEqual({ status: 'stopped' });
  expect(privateEvents()).toHaveLength(9);
  await page.evaluate(() => {
    window.pbDelayClose = false;
    for (const release of window.pbHeldClose ?? []) release();
    window.pbFixture.stop();
  });
  await expect.poll(() => privatePhysical.size).toBe(0);
  expect(privatePeak).toBe(3);
});

test('three-origin original action shares total forty-five seconds across targets and refreshed ACKs', async ({
  page
}) => {
  response = 'unknown';
  await page.evaluate(() => {
    const f = window.pbFixture;
    window.pbNetworkClock = f.installNetworkClock();
    window.pbRestore = () => window.pbNetworkClock!.restore();
    window.pbPending = f.publish();
  });
  await expect.poll(() => privateEvents().length).toBe(1);
  await page.evaluate(() => window.pbNetworkClock!.advance(46000));
  const result = await page.evaluate(async () => {
    const f = window.pbFixture,
      actual = (await window.pbPending) as NonNullable<
        ReturnType<typeof f.last>
      >;
    await f.append(actual);
    const recommit = await f.commitOriginal();
    return {
      actual,
      recommit: recommit.status,
      others: await Promise.all(
        ['wss://peer.example.org', 'wss://archive.example.org'].map((origin) =>
          f.publish(f.permission(origin))
        )
      ),
      observations: window.pbNetworkClock!.observations(),
      row: f.snapshot()!.record
    };
  });
  expect(result.actual).toMatchObject({ status: 'timed_out', attempt: 1 });
  expect(result.recommit).toBe('existing');
  expect(result.others).toEqual([{ status: 'stopped' }, { status: 'stopped' }]);
  expect(result.observations).toBeGreaterThan(2);
  expect(result.row.receipts).toHaveLength(1);
  expect(privateEvents()).toHaveLength(1);
  expect(privatePeak).toBeLessThanOrEqual(3);
  noPrivateReads();
});

for (const nested of [false, true])
  test(`three-origin actual SDK teardown ${nested ? 'reentrant Stop' : 'failure'} retries original scope while another origin remains live`, async ({
    page
  }) => {
    await page.evaluate(async () => {
      const f = window.pbFixture,
        other = f.scope(['wss://archive.example.org'], 'delivery');
      window.pbOtherPool = other;
      const result = await window.pb.publishInboxProbeArchiveAttempt(
        other,
        f.permission('wss://archive.example.org')!,
        new AbortController().signal
      );
      if (result.status !== 'accepted')
        throw Error('actual other scope publication');
      await f.append(result);
    });
    expect(privatePhysical.size).toBe(1);
    response = 'unknown';
    await page.evaluate((nested) => {
      const f = window.pbFixture;
      f.pool();
      const fault = f.installAttemptTeardownFailure(
        nested
          ? () => {
              window.pbNestedCleanup = (window.pbNestedCleanup ?? 0) + 1;
              try {
                f.stop();
              } catch {
                window.pbNestedFailure = (window.pbNestedFailure ?? 0) + 1;
              }
            }
          : undefined
      );
      window.pbRestore = () => fault.restore();
      window.pbAbort = new AbortController();
      window.pbPending = f.publish(f.permission(), window.pbAbort.signal);
    }, nested);
    await expect.poll(() => privateEvents().length).toBe(2);
    const first = await page.evaluate(async () => {
      const f = window.pbFixture;
      window.pbAbort!.abort();
      const actual = (await window.pbPending) as NonNullable<
          ReturnType<typeof f.last>
        >,
        evidence = window.pb.inboxProbeArchiveAttemptEvidence(actual);
      const cleanup = window.pb.privatePoolCleanupRequired(f.pool());
      window.pbRestore?.();
      const append = await f.append(actual);
      f.stop();
      f.stop();
      return {
        actual,
        evidence: !!evidence,
        cleanup,
        append,
        cleanAfter: window.pb.privatePoolCleanupRequired(f.pool()),
        row: f.snapshot()!.record,
        next: await f.publish()
      };
    });
    expect(first.actual).toMatchObject({ status: 'unknown', attempt: 1 });
    expect(first.evidence).toBe(true);
    expect(first.cleanup).toBe(true);
    expect(first.append.status).toBe('saved');
    expect(first.row.receipts[1].status).toBe('unknown');
    expect(first.next).toEqual({ status: 'stopped' });
    await expect.poll(() => privatePhysical.size).toBe(1);
    await expect
      .poll(() =>
        page.evaluate(() =>
          window.pb.privatePoolCleanupRequired(window.pbFixture.pool())
        )
      )
      .toBe(false);
    expect(privateConnections.get('wss://archive.example.org')).toBe(1);
    response = 'accepted';
    const other = await page.evaluate(async () => {
      const f = window.pbFixture,
        pool = window.pbOtherPool!;
      return window.pb.publishInboxProbeArchiveAttempt(
        pool,
        f.permission('wss://archive.example.org')!,
        new AbortController().signal
      );
    });
    expect(other).toMatchObject({ status: 'accepted', attempt: 2 });
    expect(privateConnections.get('wss://archive.example.org')).toBe(1);
    expect(privateEvents()).toHaveLength(3);
    expect(privatePeak).toBeLessThanOrEqual(3);
    if (nested)
      expect(
        await page.evaluate(() => ({
          nested: window.pbNestedCleanup,
          failed: window.pbNestedFailure
        }))
      ).toEqual({ nested: 1, failed: 1 });
    noPrivateReads();
  });

test('genuine ordinary paired publication cannot enter the separate probe facade or evidence family', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    window.pbFixture.close();
    const ordinary = await window.pb.makeOrdinaryPublicationFixture();
    try {
      await ordinary.prepare();
      const permission = ordinary.permission('peer');
      if (!permission)
        throw Error('genuine original ordinary permission missing');
      const result = await window.pb.publishInboxProbeArchiveAttempt(
        {} as Parameters<typeof window.pb.publishInboxProbeArchiveAttempt>[0],
        permission as unknown as Parameters<
          typeof window.pb.publishInboxProbeArchiveAttempt
        >[1],
        new AbortController().signal
      );
      return {
        result,
        evidence: !!window.pb.inboxProbeArchiveAttemptEvidence(result)
      };
    } finally {
      ordinary.close();
    }
  });
  expect(result.result).toEqual({ status: 'stopped' });
  expect(result.evidence).toBe(false);
  expect(privateEvents()).toHaveLength(0);
  noPrivateReads();
});

test('actual original SDK socket construction closing the genuine controller is fenced before EVENT and attempted evidence', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.pbFixture;
    window.pbInvalidateOnSocket = true;
    const actual = await f.publish();
    window.pbInvalidateOnSocket = false;
    return {
      actual,
      evidence: !!window.pb.inboxProbeArchiveAttemptEvidence(actual),
      rows: await f.stored(),
      ready: window.pb.inboxAccessControllerSnapshot(f.controller()!)?.readiness
    };
  });
  expect(result.actual.status).toBe('stopped');
  expect(result.actual.actionId).toBeUndefined();
  expect(result.actual.attempt).toBeUndefined();
  expect(result.evidence).toBe(false);
  expect(
    (JSON.parse(result.rows[0].wire) as { receipts: unknown[] }).receipts
  ).toHaveLength(0);
  expect(result.ready).toBe('unavailable');
  expect(privateEvents()).toHaveLength(0);
  noPrivateReads();
});
