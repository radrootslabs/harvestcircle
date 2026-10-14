import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-probe-readback.ts';
declare global {
  interface Window {
    rb: typeof Fixture;
    rbFixture: Awaited<ReturnType<typeof Fixture.makeReadbackFixture>>;
    rbPending?: Promise<unknown>;
    rbAbort?: AbortController;
    rbOtherPool?: Fixture.PrivatePool;
    rbRestore?: () => void;
    rbAdvancePage?: () => void;
    rbAdvanceOnSocket?: 'network' | 'page';
    rbClock?: number;
    rbNetworkClock?: ReturnType<Window['rbFixture']['installNetworkClock']>;
    rbDelayClose?: boolean;
    rbFailClose?: boolean;
    rbInvalidateOnSocket?: boolean;
    rbNestedCleanup?: number;
    rbNestedFailure?: number;
    rbHeldClose?: (() => void)[];
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let readMode = 'exact';
const archived = new Map<string, Record<string, unknown>>();
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
    throw Error('actual loorback fixture');
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
        const filter = frame[2] as { kinds?: number[]; ids?: string[] };
        if (filter.kinds?.includes(10050) && head)
          socket.send(JSON.stringify(['EVENT', frame[1], JSON.parse(head)]));
        if (filter.kinds?.includes(1059)) {
          if (readMode === 'refused' || readMode === 'auth_required') {
            socket.send(
              JSON.stringify([
                'CLOSED',
                frame[1],
                readMode === 'auth_required'
                  ? 'auth-required: source fixture'
                  : 'blocked source fixture'
              ])
            );
            return;
          }
          if (readMode === 'unknown') return;
          const original = archived.get(filter.ids?.[0] ?? '');
          if (original && readMode !== 'empty') {
            const event: Record<string, unknown> =
              readMode === 'metadata'
                ? {
                    sig: original.sig,
                    content: original.content,
                    tags: original.tags,
                    created_at: original.created_at,
                    kind: original.kind,
                    pubkey: original.pubkey,
                    id: original.id,
                    observation: { fixture: true }
                  }
                : { ...original };
            if (readMode === 'wrong_id') event.id = 'f'.repeat(64);
            if (readMode === 'wrong_sig') {
              event.sig = '0'.repeat(128);
              event.verified = true;
            }
            if (readMode === 'wrong_recipient')
              event.tags = [['p', 'f'.repeat(64)]];
            if (readMode === 'wrong_content')
              event.content = String(event.content) + 'a';
            if (readMode === 'oversized' || readMode === 'flood_bytes')
              event.content = 'x'.repeat(
                readMode === 'oversized' ? 40000 : 31000
              );
            if (readMode === 'flood_deliveries')
              event.content = String(event.content) + 'a';
            let wire = JSON.stringify(['EVENT', frame[1], event]);
            if (readMode === 'sdk_duplicate')
              wire = wire.replace('"kind":1059', '"kind":1059,"kind":1059');
            if (readMode === 'sdk_numeric_alias')
              wire = wire.replace('"kind":1059', '"kind":1.059e3');
            for (let n = 0; n < (readMode.startsWith('flood_') ? 501 : 1); n++)
              socket.send(wire);
          }
        }
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      } else if (frame[0] === 'AUTH') {
        socket.send(
          JSON.stringify(['OK', (frame[1] as { id: string }).id, true, ''])
        );
      } else if (frame[0] === 'EVENT' && response !== 'unknown') {
        archived.set(
          (frame[1] as { id: string }).id,
          frame[1] as Record<string, unknown>
        );
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
          new URL('./harness/inbox-probe-readback.ts', import.meta.url)
        ),
        name: 'rb',
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
  readMode = 'exact';
  archived.clear();
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
            origin === 'wss://inbox.example.org' &&
            window.rbAdvanceOnSocket
          ) {
            if (window.rbAdvanceOnSocket === 'network')
              window.rbNetworkClock!.advance(45001);
            else window.rbAdvancePage!();
          }
          if (
            window.rbInvalidateOnSocket &&
            origin === 'wss://inbox.example.org'
          )
            window.rb.closeInboxAccessController(
              window.rbFixture.controller()!
            );
        }
        override close(code?: number, reason?: string) {
          if (window.rbFailClose)
            throw Error('actual controlled disposal failure');
          if (window.rbDelayClose)
            (window.rbHeldClose ??= []).push(() => super.close(code, reason));
          else super.close(code, reason);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(async (multi) => {
    window.rbFixture = await window.rb.makeReadbackFixture();
    return window.rbFixture.preferenceWire(
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
    const f = window.rbFixture;
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
      window.rbRestore?.();
      window.rbFailClose = false;
      window.rbDelayClose = false;
      for (const release of window.rbHeldClose ?? []) release();
      window.rbFixture.close();
    });
});
function privateEvents() {
  return frames.filter(
    (x) => x.origin !== 'wss://discovery.example.org' && x.frame[0] === 'EVENT'
  );
}
function privateReads() {
  return frames.filter(
    (x) => x.origin !== 'wss://discovery.example.org' && x.frame[0] === 'REQ'
  );
}

async function publishOriginal(page: import('@playwright/test').Page) {
  return page.evaluate(async () => {
    const f = window.rbFixture,
      result = await f.publish();
    await f.append(result);
    return result;
  });
}
test('cold actual archived artifact is fetched by exact recipient REQ; original PUB and readback share action and immutable native lineage', async ({
  page
}) => {
  expect(privateReads()).toHaveLength(0);
  const published = await publishOriginal(page);
  readMode = 'metadata';
  const observed = await page.evaluate(async () => {
    const f = window.rbFixture,
      before = f.snapshot()!.record,
      action = f.action(),
      token = f.readPermission()!,
      result = await f.read(token);
    const forged = await f.appendRead({ ...result }),
      saved = await f.appendRead(result),
      duplicate = await f.appendRead(result),
      recommit = await f.commitOriginal();
    const second = await f.read();
    await f.appendRead(second);
    const fourth = await f.publish();
    return {
      before,
      result,
      forged,
      saved,
      duplicate,
      recommit: recommit.status,
      second,
      fourth,
      same: f.action() === action,
      row: f.snapshot()!.record,
      readEvidence: !!window.rb.inboxProbeReadbackAttemptEvidence(result),
      archiveEvidence: !!window.rb.inboxProbeArchiveAttemptEvidence(
        result as unknown as Parameters<
          typeof window.rb.inboxProbeArchiveAttemptEvidence
        >[0]
      ),
      readiness: window.rb.inboxAccessControllerSnapshot(f.controller()!)
        ?.readiness
    };
  });
  expect(observed.result).toMatchObject({
    status: 'readback',
    attempt: 2,
    actionId: published.actionId,
    deliveries: 1
  });
  expect(observed.second).toMatchObject({
    status: 'readback',
    attempt: 3,
    actionId: published.actionId
  });
  expect(observed.fourth).toEqual({ status: 'stopped' });
  expect(observed.same).toBe(true);
  expect(observed.forged.status).toBe('invalid');
  expect(observed.saved.status).toBe('saved');
  expect(observed.duplicate.status).toBe('existing');
  expect(observed.recommit).toBe('existing');
  expect(observed.readEvidence).toBe(true);
  expect(observed.archiveEvidence).toBe(false);
  expect(observed.readiness).toBe('unavailable');
  expect(observed.row.self).toEqual(observed.before.self);
  expect(observed.row.receipts).toHaveLength(3);
  const receipt = observed.row.receipts[1];
  expect(receipt.status).toBe('readback');
  expect(receipt.readbackWire).not.toBe(observed.before.self!.wire);
  expect(JSON.parse(receipt.readbackWire!)).toMatchObject({
    ...(JSON.parse(observed.before.self!.wire) as Record<string, unknown>),
    observation: { fixture: true }
  });
  for (const read of privateReads())
    expect(read.frame[2]).toEqual({
      kinds: [1059],
      ids: [observed.row.self!.eventId],
      '#p': [observed.row.owner],
      limit: 1
    });
  expect(privateEvents()).toHaveLength(1);
  expect(privateReads()).toHaveLength(2);
  expect(privateConnections.get('wss://inbox.example.org')).toBe(1);
  expect(privatePeak).toBe(1);
  expect(peak).toBeGreaterThan(0);
});
for (const mode of [
  'empty',
  'refused',
  'wrong_id',
  'wrong_sig',
  'wrong_recipient',
  'wrong_content',
  'oversized',
  'flood_deliveries',
  'flood_bytes',
  'sdk_duplicate',
  'sdk_numeric_alias'
])
  test(`actual SDK ${mode} preserves truthful bounded observed readback disposition`, async ({
    page
  }) => {
    await publishOriginal(page);
    readMode = mode;
    const observed = await page.evaluate(async () => {
      const f = window.rbFixture,
        result = await f.read(),
        appended = await f.appendRead(result);
      return {
        result,
        appended,
        row: f.snapshot()!.record,
        proof: !!window.rb.inboxProbeReadbackAttemptEvidence(result)
      };
    });
    const positive = mode.startsWith('sdk_');
    expect(observed.result.status).toBe(
      positive
        ? 'readback'
        : mode.startsWith('flood_')
          ? 'budget'
          : mode === 'refused'
            ? 'refused'
            : 'empty'
    );
    expect(observed.proof).toBe(positive);
    expect(observed.appended.status).toBe(positive ? 'saved' : 'invalid');
    expect(observed.row.receipts).toHaveLength(positive ? 2 : 1);
    if (mode === 'flood_deliveries')
      expect(observed.result.deliveries).toBe(501);
    if (mode === 'flood_bytes') {
      expect(observed.result.deliveries).toBeGreaterThan(200);
      expect(observed.result.deliveries).toBeLessThan(500);
    }
    if (positive) {
      const wire = observed.row.receipts[1].readbackWire!;
      expect(wire).not.toContain('1.059e3');
      expect(wire.match(/"kind"/g)).toHaveLength(1);
    }
  });
test('separate genuine publication/read permissions reject cross-family calls before REQ or extra fresh-key work', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.rbFixture,
      api = window.rb,
      before = f.counts(),
      publication = f.permission()!,
      read = f.readPermission()!;
    const wrongRead = await api.readInboxProbeAttempt(
      f.readPool(),
      publication as unknown as Parameters<typeof api.readInboxProbeAttempt>[1],
      new AbortController().signal
    );
    const wrongWrite = await api.publishInboxProbeArchiveAttempt(
      f.pool(),
      read as unknown as Parameters<
        typeof api.publishInboxProbeArchiveAttempt
      >[1],
      new AbortController().signal
    );
    const copied = await f.read({ ...read }),
      policy = api.captureInboxProbeReadback(
        f.repository,
        f.session,
        f.copiedPolicy(),
        f.acknowledgement()!,
        f.action()!,
        'wss://inbox.example.org',
        'reviewed_inbox_probe_readback'
      );
    return {
      wrongRead,
      wrongWrite,
      copied,
      policy: !!policy,
      before,
      after: f.counts()
    };
  });
  expect(result.wrongRead).toEqual({ status: 'stopped' });
  expect(result.wrongWrite).toEqual({ status: 'stopped' });
  expect(result.copied).toEqual({ status: 'stopped' });
  expect(result.policy).toBe(false);
  expect(result.after).toEqual(result.before);
  expect(privateReads()).toHaveLength(0);
  expect(privateEvents()).toHaveLength(0);
});
for (const mode of ['alias', 'forged_fact', 'removed', 'corrupt_tail'])
  test(`actual native ${mode} blocks read before original REQ`, async ({
    page
  }) => {
    await publishOriginal(page);
    const result = await page.evaluate(async (mode) => {
      const f = window.rbFixture,
        token = f.readPermission()!,
        rows = await f.stored(),
        own = rows[0];
      if (mode === 'removed') await f.removeOriginal();
      else if (mode === 'corrupt_tail')
        await f.inject([
          { owner: f.owner, id: crypto.randomUUID(), wire: '{bad' }
        ]);
      else if (mode === 'alias')
        await f.inject([{ ...own, wire: own.wire + '\n' }]);
      else {
        const row = JSON.parse(
          own.wire
        ) as import('../../src/lib/persistence/private-records.ts').PrivateAccessProbe;
        await f.inject([
          {
            ...own,
            wire: JSON.stringify({
              ...row,
              revision: row.revision + 1,
              receipts: [
                ...row.receipts,
                { ...row.receipts[0], actionId: crypto.randomUUID() }
              ]
            })
          }
        ]);
      }
      const before = await f.stored(),
        read = await f.read(token);
      return {
        read,
        unchanged: JSON.stringify(await f.stored()) === JSON.stringify(before)
      };
    }, mode);
    expect(result.read).toEqual({ status: 'stopped' });
    expect(result.unchanged).toBe(true);
    expect(privateReads()).toHaveLength(0);
  });
test('provider waiting stays outside original network meter and does not regenerate crypto or artifact', async ({
  page
}) => {
  const pub = await publishOriginal(page);
  readMode = 'exact';
  const before = await page.evaluate(() => {
    const f = window.rbFixture;
    window.rbNetworkClock = f.installNetworkClock();
    window.rbRestore = () => window.rbNetworkClock!.restore();
    f.mode('hold_key');
    const before = f.counts();
    window.rbPending = f.read();
    return before;
  });
  await expect
    .poll(() => page.evaluate(() => window.rbFixture.waiting()))
    .toBe('key');
  expect(privateReads()).toHaveLength(0);
  await page.evaluate(() => {
    window.rbNetworkClock!.advance(90000);
    window.rbFixture.mode('normal');
    window.rbFixture.settle();
  });
  expect(await page.evaluate(async () => window.rbPending)).toMatchObject({
    status: 'readback',
    attempt: 2,
    actionId: pub.actionId
  });
  const after = await page.evaluate(() => window.rbFixture.counts());
  expect(after.encrypts).toBe(before.encrypts);
  expect(after.decrypts).toBe(before.decrypts);
  expect(after.signs).toBe(before.signs);
});
test('actual learned read AUTH requirement refuses before subsequent key prompt, then explicit original AUTH permits the same action', async ({
  page
}) => {
  auth = true;
  await publishOriginal(page);
  readMode = 'auth_required';
  const first = await page.evaluate(() => window.rbFixture.read());
  expect(first).toMatchObject({ status: 'refused', attempt: 2 });
  const denied = await page.evaluate(async () => {
    const f = window.rbFixture,
      before = f.counts();
    f.mode('hold_key');
    const result = await f.read();
    return { result, before, after: f.counts(), waiting: f.waiting() };
  });
  expect(denied.result).toEqual({ status: 'stopped' });
  expect(denied.after).toEqual(denied.before);
  expect(denied.waiting).toBe('');
  expect(privateReads()).toHaveLength(1);
  readMode = 'exact';
  await page.evaluate(() => {
    const f = window.rbFixture;
    f.mode('normal');
    return window.rb.authenticateInboxAccess(
      f.controller()!,
      'wss://inbox.example.org',
      'reviewed_inbox_connection_auth'
    );
  });
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.rb.inboxAccessControllerSnapshot(
            window.rbFixture.controller()!
          )?.auth[0]?.state
      )
    )
    .toBe('challenge');
  expect(
    await page.evaluate(() =>
      window.rb.respondInboxAccessAuthentication(
        window.rbFixture.controller()!,
        'wss://inbox.example.org',
        'reviewed_inbox_auth_response'
      )
    )
  ).toBe('accepted');
  expect(await page.evaluate(() => window.rbFixture.read())).toMatchObject({
    status: 'readback',
    attempt: 3,
    actionId: first.actionId
  });
  expect(frames.filter((x) => x.frame[0] === 'AUTH')).toHaveLength(1);
  expect(privateReads()).toHaveLength(2);
});
test('actual shared forty-five-second network allowance spans archive and recipient readback without reset', async ({
  page
}) => {
  await publishOriginal(page);
  readMode = 'unknown';
  await page.evaluate(() => {
    const f = window.rbFixture;
    window.rbNetworkClock = f.installNetworkClock();
    window.rbRestore = () => window.rbNetworkClock!.restore();
    window.rbPending = f.read();
  });
  await expect.poll(() => privateReads().length).toBe(1);
  await page.evaluate(() => window.rbNetworkClock!.advance(45001));
  expect(await page.evaluate(async () => window.rbPending)).toMatchObject({
    status: 'timed_out',
    attempt: 2
  });
  readMode = 'exact';
  expect(await page.evaluate(() => window.rbFixture.read())).toEqual({
    status: 'stopped'
  });
  expect(await page.evaluate(() => window.rbFixture.publish())).toEqual({
    status: 'stopped'
  });
  expect(privateReads()).toHaveLength(1);
  expect(privateEvents()).toHaveLength(1);
});
test('actual Stop sends original CLOSE and retains ciphertext with no readback fact or readiness', async ({
  page
}) => {
  await publishOriginal(page);
  readMode = 'unknown';
  await page.evaluate(() => {
    window.rbAbort = new AbortController();
    window.rbPending = window.rbFixture.read(undefined, window.rbAbort.signal);
  });
  await expect.poll(() => privateReads().length).toBe(1);
  await page.evaluate(() => window.rbAbort!.abort());
  expect(await page.evaluate(async () => window.rbPending)).toMatchObject({
    status: 'stopped',
    attempt: 2
  });
  await expect
    .poll(
      () =>
        frames.filter(
          (x) =>
            x.origin === 'wss://inbox.example.org' && x.frame[0] === 'CLOSE'
        ).length
    )
    .toBeGreaterThan(0);
  expect(
    await page.evaluate(() => window.rbFixture.snapshot()!.record.receipts)
  ).toHaveLength(1);
});
test('actual readback evidence settles only against original lineage after owner loss and cannot restore traffic', async ({
  page
}) => {
  await publishOriginal(page);
  const result = await page.evaluate(async () => {
    const f = window.rbFixture,
      result = await f.read();
    window.rb.closeInboxAccessController(f.controller()!);
    return {
      result,
      append: await f.appendRead(result),
      later: await f.read(),
      rows: await f.stored()
    };
  });
  expect(result.result.status).toBe('readback');
  expect(result.append.status).toBe('saved');
  expect(result.later).toEqual({ status: 'stopped' });
  expect(
    (
      JSON.parse(
        result.rows[0].wire
      ) as import('../../src/lib/persistence/private-records.ts').PrivateAccessProbe
    ).receipts
  ).toHaveLength(2);
  expect(privateReads()).toHaveLength(1);
});
for (const nested of [false, true])
  test(`actual original REQ teardown ${nested ? 'reentrant Stop' : 'failure'} retains retry custody and another original archive scope`, async ({
    page
  }) => {
    await publishOriginal(page);
    const result = await page.evaluate(async (nested) => {
      const f = window.rbFixture;
      window.rbNestedFailure = 0;
      const failure = f.installReadTeardownFailure(
        nested
          ? () => {
              try {
                f.stopRead();
              } catch {
                window.rbNestedFailure!++;
              }
            }
          : undefined
      );
      window.rbRestore = failure.restore;
      const read = await f.read(),
        required = window.rb.privatePoolCleanupRequired(f.readPool()),
        faults = failure.faults();
      failure.restore();
      f.stopRead();
      const retainedArchive = await f.publish();
      return {
        read,
        required,
        faults,
        nested: window.rbNestedFailure,
        retainedArchive
      };
    }, nested);
    expect(result.read).toMatchObject({ status: 'unknown', attempt: 2 });
    expect(result.required).toBe(true);
    expect(result.faults).toBeGreaterThan(0);
    if (nested) expect(result.nested).toBeGreaterThan(0);
    expect(result.retainedArchive).toMatchObject({
      status: 'accepted',
      attempt: 3
    });
    expect(privateConnections.get('wss://inbox.example.org')).toBe(1);
    expect(privateEvents()).toHaveLength(2);
    expect(privateReads()).toHaveLength(1);
  });

for (const mode of ['aborted', 'uncertain'] as const)
  test(`actual native readback ${mode} CAS preserves original evidence and exact idempotent retry`, async ({
    page
  }) => {
    await publishOriginal(page);
    const result = await page.evaluate(async (mode) => {
      const f = window.rbFixture,
        read = await f.read(),
        before = await f.stored(),
        uncertain =
          mode === 'uncertain' ? f.installUncertainCommit() : undefined,
        restore =
          mode === 'aborted'
            ? f.installNativeAbort()
            : () => uncertain!.restore();
      let first: Awaited<ReturnType<typeof f.appendRead>>;
      try {
        first = await f.appendRead(read);
      } finally {
        restore();
      }
      const afterFirst = await f.stored(),
        second = await f.appendRead(read),
        third = await f.appendRead(read);
      return {
        first,
        before,
        afterFirst,
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
    expect(result.row.receipts).toHaveLength(2);
    if (mode === 'uncertain')
      expect(result.native).toMatchObject({
        nativeComplete: 1,
        nativeAbort: 0,
        putSuccess: 1,
        abortRefused: 1
      });
  });
test('native full hundred-row namespace supports only existing original readback growth and serial duplicate evidence', async ({
  page
}) => {
  await publishOriginal(page);
  const result = await page.evaluate(async () => {
    const f = window.rbFixture;
    await f.fillCapacity(99);
    const read = await f.read(),
      concurrent = await Promise.all([f.appendRead(read), f.appendRead(read)]),
      final = await f.appendRead(read);
    return {
      read,
      concurrent,
      final,
      rows: (await f.stored()).length,
      row: f.snapshot()!.record
    };
  });
  expect(result.read.status).toBe('readback');
  expect(result.rows).toBe(100);
  expect(result.concurrent.map((x) => x.status).sort()).toEqual([
    'busy',
    'saved'
  ]);
  expect(result.final.status).toBe('existing');
  expect(result.row.receipts).toHaveLength(2);
});
test('native eight-MiB joint namespace refuses observed readback growth without adopting metadata or losing original ciphertext', async ({
  page
}) => {
  await publishOriginal(page);
  const result = await page.evaluate(async () => {
    const f = window.rbFixture,
      actual = await f.read(),
      original = await f.stored(),
      template = JSON.parse(
        original[0].wire
      ) as import('../../src/lib/persistence/private-records.ts').PrivateAccessProbe,
      id = crypto.randomUUID(),
      fact = {
        ...template.receipts[0],
        actionId: crypto.randomUUID(),
        observedAtMilliseconds: 1
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
    await f.inject([row(Math.floor((limit - empty + 1) / (step + 1)))]);
    const before = await f.stored(),
      appended = await f.appendRead(actual);
    return {
      appended,
      unchanged: JSON.stringify(await f.stored()) === JSON.stringify(before),
      headroom: limit - bytes(before),
      row: f.snapshot()!.record,
      evidence: !!window.rb.inboxProbeReadbackAttemptEvidence(actual)
    };
  });
  expect(result.headroom).toBeGreaterThanOrEqual(0);
  expect(result.appended.status).toBe('capacity');
  expect(result.unchanged).toBe(true);
  expect(result.evidence).toBe(true);
  expect(result.row.receipts).toHaveLength(1);
});
test('actual read page expires at existing fifteen-second bound before the shared action allowance', async ({
  page
}) => {
  await publishOriginal(page);
  readMode = 'unknown';
  await page.evaluate(() => {
    const original = performance.now.bind(performance);
    let offset = 0;
    performance.now = () =>
      /at remainingPage(?:\d+)? \(/.test(new Error().stack ?? '')
        ? original() + offset
        : original();
    window.rbRestore = () => {
      performance.now = original;
    };
    window.rbAdvancePage = () => {
      offset = 15001;
    };
    window.rbPending = window.rbFixture.read();
  });
  await expect.poll(() => privateReads().length).toBe(1);
  await page.evaluate(() => window.rbAdvancePage!());
  expect(await page.evaluate(async () => window.rbPending)).toMatchObject({
    status: 'timed_out',
    attempt: 2
  });
  await page.evaluate(() => window.rbRestore!());
  readMode = 'exact';
  expect(await page.evaluate(() => window.rbFixture.read())).toMatchObject({
    status: 'readback',
    attempt: 3
  });
});
for (const stop of ['abort', 'scope'] as const)
  test(`actual original SDK OPEN-time ${stop} retains installing cleanup with no uncaught page error`, async ({
    page
  }) => {
    await publishOriginal(page);
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    const result = await page.evaluate(async (stop) => {
      const f = window.rbFixture,
        abort = new AbortController();
      let pending = 0;
      const restore = f.installReadInstallation(() => {
        if (stop === 'abort') abort.abort();
        else
          try {
            f.stopRead();
          } catch {
            pending++;
          }
      });
      window.rbRestore = restore;
      const read = await f.read(undefined, abort.signal);
      restore();
      if (stop === 'scope') f.stopRead();
      return { read, pending, archive: await f.publish() };
    }, stop);
    expect(result.read).toMatchObject({ status: 'stopped', attempt: 2 });
    expect(result.archive).toMatchObject({ status: 'accepted', attempt: 3 });
    if (stop === 'scope') expect(result.pending).toBeGreaterThan(0);
    expect(errors).toEqual([]);
  });
test('known original C7 AUTH signer wait consumes no read key prompt, network meter or recipient REQ', async ({
  page
}) => {
  auth = true;
  await publishOriginal(page);
  await page.evaluate(() =>
    window.rb.authenticateInboxAccess(
      window.rbFixture.controller()!,
      'wss://inbox.example.org',
      'reviewed_inbox_connection_auth'
    )
  );
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.rb.inboxAccessControllerSnapshot(
            window.rbFixture.controller()!
          )?.auth[0]?.state
      )
    )
    .toBe('challenge');
  await page.evaluate(() => {
    const f = window.rbFixture;
    window.rbNetworkClock = f.installNetworkClock();
    window.rbRestore = () => window.rbNetworkClock!.restore();
    f.mode('hold_sign');
    window.rbPending = window.rb.respondInboxAccessAuthentication(
      f.controller()!,
      'wss://inbox.example.org',
      'reviewed_inbox_auth_response'
    );
  });
  await expect
    .poll(() => page.evaluate(() => window.rbFixture.waiting()))
    .toBe('sign');
  const denied = await page.evaluate(async () => {
    const before = window.rbFixture.counts();
    return {
      read: await window.rbFixture.read(),
      before,
      after: window.rbFixture.counts()
    };
  });
  expect(denied.read).toEqual({ status: 'stopped' });
  expect(denied.after).toEqual(denied.before);
  expect(privateReads()).toHaveLength(0);
  await page.evaluate(() => {
    window.rbNetworkClock!.advance(90000);
    window.rbFixture.mode('normal');
    window.rbFixture.settle();
  });
  expect(await page.evaluate(async () => window.rbPending)).toBe('accepted');
  expect(await page.evaluate(() => window.rbFixture.read())).toMatchObject({
    status: 'readback',
    attempt: 2
  });
});
test('three-origin original PUB and REQ action has nine total admissions and at most three original physical connections', async ({
  page
}) => {
  const origins = [
    'wss://inbox.example.org',
    'wss://peer.example.org',
    'wss://archive.example.org'
  ];
  const result = await page.evaluate(async (origins) => {
    const f = window.rbFixture,
      action = f.action(),
      results: Awaited<ReturnType<typeof f.read>>[] = [];
    for (const origin of origins) {
      const pub = await f.publish(f.permission(origin));
      await f.append(pub);
    }
    for (let round = 0; round < 2; round++)
      for (const origin of origins) {
        const read = await f.read(f.readPermission(origin));
        results.push(read);
        await f.appendRead(read);
        await f.commitOriginal();
      }
    return {
      results,
      extra: await f.read(f.readPermission(origins[2])),
      same: f.action() === action,
      row: f.snapshot()!.record
    };
  }, origins);
  expect(result.same).toBe(true);
  expect(result.extra).toEqual({ status: 'stopped' });
  expect(result.results.every((x) => x.status === 'readback')).toBe(true);
  expect(new Set(result.results.map((x) => x.actionId)).size).toBe(1);
  expect(result.row.receipts).toHaveLength(9);
  expect(privatePeak).toBe(3);
  expect(privateReads()).toHaveLength(6);
  expect(privateEvents()).toHaveLength(3);
  for (const origin of origins) expect(privateConnections.get(origin)).toBe(1);
});
for (const clock of [NaN, Infinity, -1, 0])
  test(`actual read meter invalid clock ${String(clock)} permanently refuses original action without recipient traffic`, async ({
    page
  }) => {
    await publishOriginal(page);
    const result = await page.evaluate(async (clock) => {
      const f = window.rbFixture,
        controlled = f.installNetworkClock();
      window.rbRestore = controlled.restore;
      controlled.set(clock);
      const read = await f.read(),
        retry = await f.read();
      return { read, retry, action: !!f.action(), row: f.snapshot()!.record };
    }, clock);
    expect(result.read).toEqual({ status: 'stopped' });
    expect(result.retry).toEqual({ status: 'stopped' });
    expect(result.action).toBe(false);
    expect(privateReads()).toHaveLength(0);
    expect(result.row.receipts).toHaveLength(1);
  });

for (const clock of ['network', 'page'] as const)
  test(`actual original socket startup exhausts ${clock} deadline before REQ or attempted read evidence`, async ({
    page
  }) => {
    const result = await page.evaluate(async (clock) => {
      const f = window.rbFixture;
      if (clock === 'network') {
        window.rbNetworkClock = f.installNetworkClock();
        window.rbRestore = () => window.rbNetworkClock!.restore();
      } else {
        const original = performance.now.bind(performance);
        let offset = 0;
        performance.now = () =>
          /at remainingPage(?:\d+)? \(/.test(new Error().stack ?? '')
            ? original() + offset
            : original();
        window.rbRestore = () => {
          performance.now = original;
        };
        window.rbAdvancePage = () => {
          offset = 15001;
        };
      }
      window.rbAdvanceOnSocket = clock;
      const read = await f.read();
      return {
        read,
        proof: !!window.rb.inboxProbeReadbackAttemptEvidence(read),
        row: f.snapshot()!.record
      };
    }, clock);
    expect(result.read).toMatchObject({ status: 'stopped' });
    expect(result.read.attempt).toBeUndefined();
    expect(result.proof).toBe(false);
    expect(result.row.receipts).toHaveLength(0);
    expect(privateReads()).toHaveLength(0);
    expect(privateEvents()).toHaveLength(0);
  });
test('actual original socket startup closing controller refuses before recipient REQ', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    window.rbInvalidateOnSocket = true;
    const f = window.rbFixture,
      read = await f.read();
    return { read, rows: await f.stored() };
  });
  expect(result.read).toMatchObject({ status: 'stopped' });
  expect(result.read.attempt).toBeUndefined();
  expect(privateReads()).toHaveLength(0);
  expect(
    JSON.parse(result.rows[0].wire) as Record<string, unknown>
  ).toHaveProperty('family', 'private_access_probe');
});
test('actual same SDK request with foreign observation source cannot mint readback provenance', async ({
  page
}) => {
  await publishOriginal(page);
  const result = await page.evaluate(async () => {
    const f = window.rbFixture,
      restore = f.installWrongReadSource();
    window.rbRestore = restore;
    const read = await f.read();
    restore();
    return {
      read,
      append: await f.appendRead(read),
      row: f.snapshot()!.record
    };
  });
  expect(result.read.status).toBe('error');
  expect(result.read.deliveries).toBe(1);
  expect(result.append.status).toBe('invalid');
  expect(result.row.receipts).toHaveLength(1);
});

for (const status of ['readback', 'nonce_verified'] as const)
  test(`handwritten strictly valid ${status} labels cannot replace original read permission or admit lineage`, async ({
    page
  }) => {
    await publishOriginal(page);
    const result = await page.evaluate(async (status) => {
      const f = window.rbFixture,
        token = f.readPermission()!,
        original = (await f.stored())[0],
        row = JSON.parse(
          original.wire
        ) as import('../../src/lib/persistence/private-records.ts').PrivateAccessProbe,
        fake = {
          ...row,
          revision: row.revision + 1,
          receipts: [
            ...row.receipts,
            {
              actionId: crypto.randomUUID(),
              origin: 'wss://inbox.example.org',
              role: 'access_readback',
              eventId: row.self!.eventId,
              attempt: 1,
              status,
              observedAtMilliseconds: Date.now(),
              readbackWire: row.self!.wire
            }
          ]
        },
        wire = JSON.stringify(fake);
      const valid = window.rb.decodePrivateRecord(wire, row.owner, row.id).ok;
      await f.inject([{ ...original, wire }]);
      const read = await f.read(token),
        commit = await f.commitOriginal();
      return {
        valid,
        read,
        commit: commit.status,
        unchanged: (await f.stored())[0].wire === wire,
        live: !!f.snapshot()
      };
    }, status);
    expect(result.valid).toBe(true);
    expect(result.read).toEqual({ status: 'stopped' });
    expect(result.commit).toBe('conflict');
    expect(result.unchanged).toBe(true);
    expect(result.live).toBe(false);
    expect(privateReads()).toHaveLength(0);
  });
for (const fault of ['held', 'failed'] as const)
  test(`actual original read-only ${fault} physical close blocks replacement until actual original retry and close`, async ({
    page
  }) => {
    const first = await page.evaluate(() => window.rbFixture.read());
    expect(first.status).toBe('empty');
    await expect.poll(() => privatePhysical.size).toBe(1);
    const stopped = await page.evaluate((fault) => {
      const f = window.rbFixture;
      window.rbDelayClose = fault === 'held';
      window.rbFailClose = fault === 'failed';
      let failed = false;
      try {
        f.stopRead();
      } catch {
        failed = true;
      }
      let replacement = false;
      try {
        replacement = !!window.rb.getPrivatePool(
          f.session,
          f.policy,
          ['wss://inbox.example.org'],
          Object.freeze({}),
          'access'
        );
      } catch {
        /* Actual original cleanup pending. */
      }
      return {
        failed,
        replacement,
        required: window.rb.privatePoolCleanupRequired(f.readPool())
      };
    }, fault);
    expect(stopped.required).toBe(true);
    expect(stopped.replacement).toBe(false);
    if (fault === 'failed') expect(stopped.failed).toBe(true);
    await page.evaluate(() => {
      window.rbDelayClose = false;
      window.rbFailClose = false;
      for (const release of window.rbHeldClose ?? []) release();
      window.rbFixture.stopRead();
    });
    await expect.poll(() => privatePhysical.size).toBe(0);
    await expect
      .poll(() =>
        page.evaluate(() =>
          window.rb.privatePoolCleanupRequired(window.rbFixture.readPool())
        )
      )
      .toBe(false);
    const next = await page.evaluate(async () => {
      const f = window.rbFixture;
      window.rbOtherPool = window.rb.getPrivatePool(
        f.session,
        f.policy,
        ['wss://inbox.example.org'],
        Object.freeze({}),
        'access'
      )!;
      const result = await window.rb.readInboxProbeAttempt(
        window.rbOtherPool,
        f.readPermission()!,
        new AbortController().signal
      );
      window.rb.closePrivatePool(window.rbOtherPool);
      return result;
    });
    expect(next).toMatchObject({
      status: 'empty',
      attempt: 2,
      actionId: first.actionId
    });
    expect(privateReads()).toHaveLength(2);
    expect(privateConnections.get('wss://inbox.example.org')).toBe(2);
  });
test('three-origin failed recipient cleanup preserves other original archive connection and scope', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.rbFixture;
    const first = await f.publish();
    await f.append(first);
    const other = await f.publish(f.permission('wss://peer.example.org'));
    await f.append(other);
    const failure = f.installReadTeardownFailure();
    window.rbRestore = failure.restore;
    const read = await f.read();
    failure.restore();
    f.stopRead();
    const next = await f.publish(f.permission('wss://peer.example.org'));
    return { read, next, other, before: first.actionId };
  });
  expect(result.read).toMatchObject({ status: 'unknown', attempt: 2 });
  expect(result.next).toMatchObject({
    status: 'accepted',
    attempt: 2,
    actionId: result.other.actionId
  });
  expect(result.next.actionId).toBe(result.before);
  expect(privateConnections.get('wss://peer.example.org')).toBe(1);
  expect(privateEvents()).toHaveLength(3);
});
