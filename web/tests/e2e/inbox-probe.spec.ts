import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { WebSocketServer } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-probe.ts';
declare global {
  interface Window {
    p8: typeof Fixture;
    p8Fixture: Awaited<ReturnType<typeof Fixture.makeProbeFixture>>;
    p8Pending?: Promise<string>;
    p8Restore?: () => void;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string;
let head = '';
const frames: unknown[][] = [];
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
    throw Error('owned fixture relay');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket) => {
    socket.on('message', (bytes) => {
      const raw = Buffer.isBuffer(bytes)
        ? bytes
        : Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.from(bytes);
      const frame = JSON.parse(raw.toString()) as unknown[];
      frames.push(frame);
      if (frame[0] === 'REQ') {
        const filter = frame[2] as { kinds?: number[] };
        if (filter.kinds?.includes(10050) && head)
          socket.send(JSON.stringify(['EVENT', frame[1], JSON.parse(head)]));
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
          new URL('./harness/inbox-probe.ts', import.meta.url)
        ),
        name: 'p8',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('actual bundle');
  const chunks = output.output.filter((chunk) => chunk.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
});
test.afterAll(async () => {
  for (const socket of sockets.clients) socket.terminate();
  await new Promise<void>((resolve) => sockets.close(() => resolve()));
  await server.close();
});
test.beforeEach(async ({ page }) => {
  await expect.poll(() => sockets.clients.size).toBe(0);
  head = '';
  frames.length = 0;
  await page.addInitScript(
    ({ endpoint }) => {
      const Native = window.WebSocket;
      window.WebSocket = class extends Native {
        constructor(url: string | URL, protocols?: string | string[]) {
          if (new URL(String(url)).origin !== 'wss://discovery.example.org')
            throw Error('construction attempted private transport');
          super(endpoint, protocols);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(async () => {
    window.p8Fixture = await window.p8.makeProbeFixture();
    return window.p8Fixture.preferenceWire(['wss://inbox.example.org']);
  });
  await page.evaluate(() => window.p8Fixture.capture());
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => {
      window.p8Restore?.();
      window.p8Fixture.close();
    });
});

test('cold genuine self diagnostic reserves native metadata before prompts and decrypts the exact original SDK layers', async ({
  page
}) => {
  const before = await page.evaluate(() => window.p8Fixture.counts());
  // Evaluate the native promise explicitly; there was no earlier ordinary send.
  expect(await page.evaluate(() => window.p8Fixture.stored())).toHaveLength(0);
  const reservation = await page.evaluate(() => window.p8Fixture.reserve());
  expect(reservation.status).toBe('reserved');
  const saved = await page.evaluate(() => window.p8Fixture.stored());
  expect(saved).toHaveLength(1);
  expect(JSON.stringify(saved)).not.toContain(
    'Harvestcircle private inbox access check'
  );
  expect(JSON.parse(saved[0].wire)).toMatchObject({
    family: 'private_access_probe',
    revision: 0,
    self: null,
    receipts: []
  });
  expect(await page.evaluate(() => window.p8Fixture.counts())).toEqual(before);
  expect(await page.evaluate(() => window.p8Fixture.prepare())).toBe('wrapped');
  expect(await page.evaluate(() => window.p8Fixture.decryptOriginal())).toEqual(
    {
      outerVerified: true,
      sealVerified: true,
      exactRumor: true,
      hash: true,
      owner: true,
      recipient: true,
      createdAt: true,
      kind: 14,
      diagnostic: true
    }
  );
  expect(await page.evaluate(() => window.p8Fixture.stored())).toEqual(saved);
  const duplicate = await page.evaluate(() => window.p8Fixture.reserve());
  expect(duplicate.status).toBe('existing');
  expect(duplicate.snapshot).toEqual(reservation.snapshot);
  expect(
    await page.evaluate(
      () =>
        window.p8.inboxAccessControllerSnapshot(window.p8Fixture.controller()!)
          ?.readiness
    )
  ).toBe('unavailable');
  expect(
    frames.some((frame) => ['EVENT', 'AUTH'].includes(String(frame[0])))
  ).toBe(false);
  expect(
    frames
      .filter((frame) => frame[0] === 'REQ')
      .every((frame) => (frame[2] as { kinds: number[] }).kinds.includes(10050))
  ).toBe(true);
});

test('separate genuine maps reject ordinary adoption, copied snapshots and replacement crypto artifacts', async ({
  page
}) => {
  await page.evaluate(() => window.p8Fixture.reserve());
  expect(await page.evaluate(() => window.p8Fixture.prepare())).toBe('wrapped');
  const result = await page.evaluate(async () => {
    const f = window.p8Fixture,
      api = window.p8,
      identity = f.probeIdentity()!,
      seal = f.seal()!,
      wrap = f.wrap()!,
      operation = f.operation()!;
    const same = api.buildInboxProbeGiftwrap(seal, 'reviewed_inbox_probe_wrap');
    const reserved = identity as unknown as Parameters<
      typeof api.capturePrivateSealOperation
    >[1];
    const ordinarySeal = seal as unknown as Parameters<
      typeof api.privateSealSnapshot
    >[0];
    const ordinaryWrap = wrap as unknown as Parameters<
      typeof api.privateGiftwrapSnapshot
    >[0];
    const copy = api.inboxProbeSnapshot(identity)! as unknown as Parameters<
      typeof api.captureInboxProbeSealOperation
    >[1];
    return {
      sameWrap: same === wrap,
      sameOperation:
        api.captureInboxProbeSealOperation(
          f.session,
          identity,
          'reviewed_inbox_probe_seal'
        ) === operation,
      duplicateRun: (await api.buildInboxProbeSeal(operation)).status,
      ordinaryIdentity: api.reservedSendSnapshot(reserved) === undefined,
      ordinaryCapture:
        api.capturePrivateSealOperation(
          f.identity,
          reserved,
          'self',
          'reviewed_private_seal'
        ) === undefined,
      ordinaryRun: (
        await api.buildPrivateSeal(
          operation as unknown as Parameters<typeof api.buildPrivateSeal>[0]
        )
      ).status,
      ordinarySeal:
        api.privateSealSnapshot(ordinarySeal) === undefined &&
        api.privateSealReservation(ordinarySeal) === undefined,
      ordinaryWrap:
        api.buildPrivateGiftwrap(ordinarySeal, 'reviewed_private_wrap') ===
          undefined && api.privateGiftwrapSnapshot(ordinaryWrap) === undefined,
      outbound:
        api.verifyOutboundEnvelope(
          reserved,
          ordinaryWrap,
          'reviewed_outbound_layers'
        ) === undefined,
      copy:
        api.captureInboxProbeSealOperation(
          f.session,
          copy,
          'reviewed_inbox_probe_seal'
        ) === undefined
    };
  });
  expect(result).toEqual({
    sameWrap: true,
    sameOperation: true,
    duplicateRun: 'stopped',
    ordinaryIdentity: true,
    ordinaryCapture: true,
    ordinaryRun: 'invalid',
    ordinarySeal: true,
    ordinaryWrap: true,
    outbound: true,
    copy: true
  });
});

test('simultaneous genuine reservation returns busy while pending and creates exactly one native original', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.p8Fixture,
      first = f.reserve(),
      second = f.reserve(),
      [a, b] = await Promise.all([first, second]);
    return {
      a,
      b,
      rows: await f.stored(),
      counts: f.counts(),
      duplicate: await f.reserve()
    };
  });
  expect(result.a.status).toBe('reserved');
  expect(result.b).toMatchObject({
    status: 'busy',
    disposition: 'not_created'
  });
  expect(result.rows).toHaveLength(1);
  expect(result.rows[0].id).toBe(result.a.snapshot?.id);
  expect(result.duplicate.status).toBe('existing');
  expect(result.duplicate.snapshot).toEqual(result.a.snapshot);
  expect(result.counts).toEqual({
    keys: 0,
    encrypts: 0,
    decrypts: 0,
    signs: 0
  });
});

test('failed original probe wrapping stays protected and cannot mint a replacement after SDK recovery', async ({
  page
}) => {
  await page.evaluate(() => window.p8Fixture.reserve());
  expect(await page.evaluate(() => window.p8Fixture.prepareSeal())).toBe(
    'sealed'
  );
  const result = await page.evaluate(() => {
    const api = window.p8,
      f = window.p8Fixture,
      fault = f.installWrapFault();
    window.p8Restore = () => fault.restore();
    const first = f.wrapOriginal();
    fault.allow();
    const second = f.wrapOriginal();
    return {
      first,
      second,
      calls: fault.calls(),
      sealCurrent: api.inboxProbeSealSnapshot(f.seal()!) !== undefined,
      noWrap: f.wrap() === undefined,
      noReplacement:
        api.buildInboxProbeGiftwrap(f.seal()!, 'reviewed_inbox_probe_wrap') ===
        undefined
    };
  });
  expect(result).toEqual({
    first: 'wrap_failed',
    second: 'wrap_failed',
    calls: 1,
    sealCurrent: true,
    noWrap: true,
    noReplacement: true
  });
  const rows = await page.evaluate(() => window.p8Fixture.stored());
  expect(rows).toHaveLength(1);
  expect(JSON.parse(rows[0].wire)).toMatchObject({
    family: 'private_access_probe',
    revision: 0,
    self: null,
    receipts: []
  });
  expect(
    frames.some((frame) => ['EVENT', 'AUTH'].includes(String(frame[0])))
  ).toBe(false);
});

test('controller close rejects reentry, clears the original nonce first and retries only failed cleanup', async ({
  page
}) => {
  await page.evaluate(() => window.p8Fixture.reserve());
  const original = await page.evaluate(() => window.p8Fixture.stored());
  const result = await page.evaluate(() => {
    const api = window.p8,
      f = window.p8Fixture,
      controller = f.controller()!;
    let calls = 0,
      nested: boolean | undefined,
      failing = true,
      cleared = false;
    const off = api.subscribeInboxAccessControllerClose(controller, () => {
      calls++;
      cleared =
        api.inboxProbeRumorWire(f.probeIdentity()!, f.session) === undefined;
      nested = api.closeInboxAccessController(controller);
      if (failing) throw Error('controlled original cleanup fault');
    });
    const first = api.closeInboxAccessController(controller),
      firstCleanup =
        api.inboxAccessControllerSnapshot(controller)?.cleanupRequired;
    failing = false;
    const second = api.closeInboxAccessController(controller),
      secondCleanup =
        api.inboxAccessControllerSnapshot(controller)?.cleanupRequired;
    off();
    return {
      first,
      firstCleanup,
      second,
      secondCleanup,
      calls,
      nested,
      cleared
    };
  });
  expect(result).toEqual({
    first: false,
    firstCleanup: true,
    second: true,
    secondCleanup: false,
    calls: 2,
    nested: false,
    cleared: true
  });
  expect(await page.evaluate(() => window.p8Fixture.stored())).toEqual(
    original
  );
  expect(await page.evaluate(() => window.p8Fixture.reserve())).toMatchObject({
    status: 'stopped',
    disposition: 'may_exist'
  });
});

test('unsupported and unconfigured declarations refuse before rows, provider work or private transport', async ({
  page
}) => {
  const before = await page.evaluate(() => window.p8Fixture.counts());
  for (const selected of ['unsupported', 'unconfigured'] as const) {
    const result = await page.evaluate(
      (selected) =>
        window.p8Fixture.reserve(
          window.p8Fixture.operator(
            selected === 'unsupported',
            selected === 'unconfigured'
          )
        ),
      selected
    );
    expect(result).toMatchObject({
      status: 'unavailable',
      disposition: 'not_created'
    });
  }
  expect(await page.evaluate(() => window.p8Fixture.stored())).toHaveLength(0);
  expect(await page.evaluate(() => window.p8Fixture.counts())).toEqual(before);
});

test('foreign namespace and forged owner/config inputs cannot reserve or observe a provider', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.p8Fixture,
      api = window.p8,
      before = f.counts();
    const fake = Object.freeze({ owner: f.owner, current: () => true });
    const reservation = await api.reserveInboxProbeIdentity(
      fake as unknown as Parameters<typeof api.reserveInboxProbeIdentity>[0],
      f.session,
      f.policy,
      f.controller()!,
      f.qualification,
      'reviewed_inbox_probe_reservation'
    );
    const forged = await api.reserveInboxProbeIdentity(
      f.repository,
      fake as unknown as Parameters<typeof api.reserveInboxProbeIdentity>[1],
      f.policy,
      f.controller()!,
      f.qualification,
      'reviewed_inbox_probe_reservation'
    );
    const config = await api.reserveInboxProbeIdentity(
      f.repository,
      f.session,
      f.policy,
      f.controller()!,
      fake as unknown as Parameters<typeof api.reserveInboxProbeIdentity>[4],
      'reviewed_inbox_probe_reservation'
    );
    return {
      reservation,
      forged,
      config,
      before,
      after: f.counts(),
      rows: await f.stored()
    };
  });
  expect(result.reservation).toMatchObject({
    status: 'invalid_scope',
    disposition: 'not_created'
  });
  expect(result.forged).toMatchObject({
    status: 'invalid_scope',
    disposition: 'not_created'
  });
  expect(result.config).toMatchObject({
    status: 'unavailable',
    disposition: 'not_created'
  });
  expect(result.before).toEqual(result.after);
  expect(result.rows).toHaveLength(0);
});

test('genuine foreign namespace, same-owner session and distinct policy cannot borrow the original controller', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.p8Fixture,
      api = window.p8,
      session = await f.foreignSession(),
      before = f.counts();
    const invoke = (
      repository: Parameters<typeof api.reserveInboxProbeIdentity>[0],
      session: Parameters<typeof api.reserveInboxProbeIdentity>[1],
      policy: Parameters<typeof api.reserveInboxProbeIdentity>[2]
    ) =>
      api.reserveInboxProbeIdentity(
        repository,
        session,
        policy,
        f.controller()!,
        f.qualification,
        'reviewed_inbox_probe_reservation'
      );
    return {
      namespace: await invoke(f.foreignRepository(), f.session, f.policy),
      session: await invoke(f.repository, session, f.policy),
      policy: await invoke(f.repository, f.session, f.copiedPolicy()),
      before,
      after: f.counts(),
      rows: await f.stored()
    };
  });
  for (const disposition of [result.namespace, result.session, result.policy])
    expect(disposition).toEqual({
      status: 'invalid_scope',
      disposition: 'not_created'
    });
  expect(result.before).toEqual(result.after);
  expect(result.rows).toHaveLength(0);
});

test('native put success followed by abort gives no identity, row or crypto permission', async ({
  page
}) => {
  await page.evaluate(() => {
    window.p8Restore = window.p8Fixture.installNativeAbort();
  });
  expect(await page.evaluate(() => window.p8Fixture.reserve())).toMatchObject({
    status: 'aborted',
    disposition: 'not_created'
  });
  expect(await page.evaluate(() => window.p8Fixture.stored())).toHaveLength(0);
  expect(
    await page.evaluate(() => window.p8Fixture.probeIdentity())
  ).toBeUndefined();
  expect(await page.evaluate(() => window.p8Fixture.counts())).toEqual({
    keys: 0,
    encrypts: 0,
    decrypts: 0,
    signs: 0
  });
});

test('actual competing native write before readonly reconciliation cannot issue an original identity', async ({
  page
}) => {
  await page.evaluate(() => {
    window.p8Restore = window.p8Fixture.installChangedReadback();
  });
  expect(await page.evaluate(() => window.p8Fixture.reserve())).toMatchObject({
    status: 'conflict',
    disposition: 'may_exist'
  });
  expect(await page.evaluate(() => window.p8Fixture.stored())).toHaveLength(1);
  expect(
    await page.evaluate(() => window.p8Fixture.probeIdentity())
  ).toBeUndefined();
  expect(await page.evaluate(() => window.p8Fixture.counts())).toEqual({
    keys: 0,
    encrypts: 0,
    decrypts: 0,
    signs: 0
  });
});

test('the genuine reservation respects the existing full joint native quota before prompts', async ({
  page
}) => {
  await page.evaluate(() => window.p8Fixture.fillCapacity());
  expect(await page.evaluate(() => window.p8Fixture.reserve())).toMatchObject({
    status: 'capacity',
    disposition: 'not_created'
  });
  expect(await page.evaluate(() => window.p8Fixture.stored())).toHaveLength(
    100
  );
  expect(await page.evaluate(() => window.p8Fixture.counts())).toEqual({
    keys: 0,
    encrypts: 0,
    decrypts: 0,
    signs: 0
  });
});

for (const mode of [
  'wrong_key',
  'refuse_encrypt',
  'refuse_decrypt',
  'refuse_sign',
  'wrong_encrypt',
  'wrong_decrypt',
  'wrong_sign_author',
  'wrong_sign_time',
  'wrong_sign_tags',
  'cached_signature',
  'stop_encrypt_getter',
  'stop_decrypt_getter'
] as const) {
  test(`genuine construction rejects ${mode} without any probe EVENT`, async ({
    page
  }) => {
    await page.evaluate(() => window.p8Fixture.reserve());
    const rows = await page.evaluate(() => window.p8Fixture.stored());
    await page.evaluate((mode) => window.p8Fixture.mode(mode), mode);
    expect(await page.evaluate(() => window.p8Fixture.prepare())).not.toBe(
      'wrapped'
    );
    expect(await page.evaluate(() => window.p8Fixture.wrap())).toBeUndefined();
    expect(await page.evaluate(() => window.p8Fixture.stored())).toEqual(rows);
    expect(
      frames.some((frame) => ['EVENT', 'AUTH'].includes(String(frame[0])))
    ).toBe(false);
    if (mode.startsWith('stop_'))
      expect(await page.evaluate(() => window.p8Fixture.counts())).toEqual({
        keys: 0,
        encrypts: 0,
        decrypts: 0,
        signs: 0
      });
  });
}

for (const stage of ['key', 'encrypt', 'decrypt', 'sign'] as const) {
  for (const invalidation of ['stop', 'head', 'session'] as const) {
    test(`pending ${stage} remains owned and late results are fenced after ${invalidation}`, async ({
      page
    }) => {
      await page.evaluate(() => window.p8Fixture.reserve());
      const rows = await page.evaluate(() => window.p8Fixture.stored());
      await page.evaluate((stage) => {
        window.p8Fixture.mode(('hold_' + stage) as Fixture.Mode);
        window.p8Pending = window.p8Fixture.prepare();
      }, stage);
      await expect
        .poll(() => page.evaluate(() => window.p8Fixture.waiting()))
        .toBe(stage);
      const observed = await page.evaluate((invalidation) => {
        const api = window.p8,
          f = window.p8Fixture;
        if (invalidation === 'stop') api.stopInboxProbeSeal(f.operation()!);
        else if (invalidation === 'head') f.churnHead();
        else f.logout();
        return {
          identity: api.inboxProbeSnapshot(f.probeIdentity()!) === undefined,
          wire:
            api.inboxProbeRumorWire(f.probeIdentity()!, f.session) === undefined
        };
      }, invalidation);
      expect(observed).toEqual({ identity: true, wire: true });
      await page.evaluate(() => window.p8Fixture.settle());
      expect(await page.evaluate(() => window.p8Pending)).toBe('stopped');
      expect(
        await page.evaluate(() => window.p8Fixture.wrap())
      ).toBeUndefined();
      expect(await page.evaluate(() => window.p8Fixture.stored())).toEqual(
        rows
      );
      expect(
        await page.evaluate(() => window.p8Fixture.reserve())
      ).toMatchObject({ status: 'stopped', disposition: 'may_exist' });
      expect(
        frames.some((frame) => ['EVENT', 'AUTH'].includes(String(frame[0])))
      ).toBe(false);
    });
  }
}
