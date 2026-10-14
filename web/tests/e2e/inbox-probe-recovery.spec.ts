import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-probe-recovery.ts';
declare global {
  interface Window {
    pr: typeof Fixture;
    prFixture: Awaited<ReturnType<typeof Fixture.makeRecoveryFixture>>;
    prFresh: Awaited<ReturnType<typeof Fixture.openRecoveryContext>>;
    prPending?: Promise<unknown>;
    prRelease?: () => void;
    prRestore?: () => void;
    prHosted?: ReturnType<typeof Fixture.createHostedProvider>;
    prHostedContext?: Awaited<ReturnType<typeof Fixture.openRecoveryContext>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string,
  head = '';
const physical = new Set<WebSocket>(),
  archived = new Map<string, Record<string, unknown>>(),
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
    throw Error('actual loopback fixture');
  endpoint = 'ws://127.0.0.1:' + address.port;
  sockets.on('connection', (socket, request) => {
    physical.add(socket);
    socket.on('close', () => physical.delete(socket));
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
      if (frame[0] === 'REQ') {
        const filter = frame[2] as { kinds?: number[]; ids?: string[] };
        if (filter.kinds?.includes(10050) && head)
          socket.send(JSON.stringify(['EVENT', frame[1], JSON.parse(head)]));
        if (filter.kinds?.includes(1059)) {
          const event = archived.get(filter.ids?.[0] ?? '');
          if (event) socket.send(JSON.stringify(['EVENT', frame[1], event]));
        }
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      } else if (frame[0] === 'EVENT') {
        const event = frame[1] as { id: string };
        archived.set(event.id, event);
        socket.send(JSON.stringify(['OK', event.id, true, '']));
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
          new URL('./harness/inbox-probe-recovery.ts', import.meta.url)
        ),
        name: 'pr',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('compiled actual source fixture');
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
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(async () => {
    window.prFixture = await window.pr.makeRecoveryFixture();
    return window.prFixture.preferenceWire(['wss://inbox.example.org']);
  });
  await page.evaluate(async () => {
    const f = window.prFixture;
    await f.capture();
    if (
      (await f.reserve()).status !== 'reserved' ||
      (await f.prepare()) !== 'wrapped' ||
      (await f.commitOriginal()).status !== 'prepared'
    )
      throw Error('actual cold stored original');
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(() => {
      window.prRestore?.();
      window.prRelease?.();
      window.prHostedContext?.close();
      window.prHosted?.close();
      window.prFixture.close();
    });
});
async function fresh(page: Page) {
  await page.evaluate(async () => {
    window.prFresh = await window.prFixture.fresh();
  });
}
function privateFrames() {
  return frames.filter((x) => x.origin !== 'wss://discovery.example.org');
}
async function originalReadback(page: Page) {
  await page.evaluate(async () => {
    const f = window.prFixture,
      publication = await f.publish();
    if (
      publication.status !== 'accepted' ||
      (await f.append(publication)).status !== 'saved'
    )
      throw Error('actual original B1');
    const readback = await f.read();
    if (
      readback.status !== 'readback' ||
      (await f.appendRead(readback)).status !== 'saved'
    )
      throw Error('actual original B2');
  });
}
test('genuine ordinary and C1 readers/proofs cannot cross into recovered-probe families', async ({
  page
}) => {
  await originalReadback(page);
  const result = await page.evaluate(async () => {
    const f = window.prFixture,
      nonce = await f.nonce();
    if (!('proof' in nonce)) throw Error('genuine current C1 proof');
    const id = f.snapshot()!.record.id;
    const ordinary = window.pr.captureSelfRecoveryReader(
      f.repository,
      f.identity,
      id,
      'reviewed_self_decryption'
    )!;
    const recovery = window.pr.captureInboxProbeRecoveryReader(
      f.repository,
      f.identity,
      f.session,
      f.policy,
      f.controller()!,
      f.qualification,
      id,
      'reviewed_inbox_probe_recovery'
    )!;
    const before = f.counts();
    const c1Reader = await window.pr.recoverInboxProbe(
      f.nonceReader()! as unknown as Parameters<
        typeof window.pr.recoverInboxProbe
      >[0]
    );
    const ordinaryReader = await window.pr.recoverInboxProbe(
      ordinary as unknown as Parameters<typeof window.pr.recoverInboxProbe>[0]
    );
    const c1Proof = window.pr.recoveredInboxProbeBinding(
      nonce.proof as unknown as Parameters<
        typeof window.pr.recoveredInboxProbeBinding
      >[0],
      f.repository,
      f.session,
      f.controller()!,
      f.policy,
      f.qualification
    );
    const noEffects = JSON.stringify(before) === JSON.stringify(f.counts());
    const recovered = await window.pr.recoverInboxProbe(recovery);
    const wrongC1 = await window.pr.readInboxProbeNonce(
      recovery as unknown as Parameters<typeof window.pr.readInboxProbeNonce>[0]
    );
    return {
      ordinaryGenuine: !!ordinary,
      c1Reader,
      ordinaryReader,
      c1Proof: !!c1Proof,
      noEffects,
      recovery: recovered.status,
      wrongC1
    };
  });
  expect(result).toEqual({
    ordinaryGenuine: true,
    c1Reader: { status: 'invalid' },
    ordinaryReader: { status: 'invalid' },
    c1Proof: false,
    noEffects: true,
    recovery: 'recovered',
    wrongC1: { status: 'invalid' }
  });
});
test('genuine current C1 memory proof cannot replace actual original stored ciphertext decryption', async ({
  page
}) => {
  await originalReadback(page);
  const result = await page.evaluate(async () => {
    const f = window.prFixture,
      nonce = await f.nonce();
    if (!('proof' in nonce)) throw Error('genuine current C1 proof');
    const reader = window.pr.captureInboxProbeRecoveryReader(
      f.repository,
      f.identity,
      f.session,
      f.policy,
      f.controller()!,
      f.qualification,
      f.snapshot()!.record.id,
      'reviewed_inbox_probe_recovery'
    )!;
    f.mode('wrong_decrypt');
    const before = await f.stored(),
      result = await window.pr.recoverInboxProbe(reader);
    return {
      status: result.status,
      same: JSON.stringify(before) === JSON.stringify(await f.stored())
    };
  });
  expect(result).toEqual({ status: 'mismatch', same: true });
});
for (const alias of ['whitespace', 'key-order'])
  for (const phase of ['before', 'final-key'])
    test(`physical ${alias} alias at ${phase} cannot pass canonical-only native comparison`, async ({
      page
    }) => {
      await fresh(page);
      const result = await page.evaluate(
        async ({ alias, phase }) => {
          const f = window.prFixture,
            r = window.prFresh,
            row = (await f.stored()).find((x) => x.id === r.localId)!;
          const decoded = window.pr.decodePrivateRecord(
            row.wire,
            row.owner,
            row.id
          );
          if (!decoded.ok) throw Error('original strict native row');
          const record = JSON.parse(row.wire) as Record<string, unknown>;
          const wire =
            alias === 'whitespace'
              ? JSON.stringify(record, null, 1)
              : JSON.stringify(
                  Object.fromEntries(Object.entries(record).reverse())
                );
          const alternate = window.pr.decodePrivateRecord(
            wire,
            row.owner,
            row.id
          );
          if (
            !alternate.ok ||
            window.pr.privateRecordWire(alternate.value, row.owner, row.id) !==
              window.pr.privateRecordWire(decoded.value, row.owner, row.id)
          )
            throw Error('actual canonical-equivalent physical alias');
          if (phase === 'before') await f.inject([{ ...row, wire }]);
          else
            window.prRestore = f.keyCallback(3, () =>
              f.inject([{ ...row, wire }])
            );
          const result = await r.recover();
          return {
            status: result.status,
            proof: !!r.binding(),
            aliasRetained:
              (await f.stored()).find((x) => x.id === r.localId)?.wire === wire
          };
        },
        { alias, phase }
      );
      expect(result.status).not.toBe('recovered');
      expect(result.proof).toBe(false);
      expect(result.aliasRetained).toBe(true);
    });
test('actual B1/B2 then memory teardown recovers the immutable original with no ordinary send, new artifact, native growth or traffic', async ({
  page
}) => {
  await page.evaluate(async () => {
    const f = window.prFixture;
    const pub = await f.publish();
    if (pub.status !== 'accepted' || (await f.append(pub)).status !== 'saved')
      throw Error('actual B1');
    const read = await f.read();
    if (
      read.status !== 'readback' ||
      (await f.appendRead(read)).status !== 'saved'
    )
      throw Error('actual B2');
  });
  await fresh(page);
  const traffic = privateFrames().length;
  const result = await page.evaluate(async () => {
    const f = window.prFixture,
      r = window.prFresh,
      before = f.counts(),
      rows = await f.stored(),
      reader = r.capture()!,
      repeat = r.capture();
    const inert = f.counts(),
      receiver = f.decryptCallback(0, (wire) => wire);
    const originalRandom = crypto.getRandomValues.bind(crypto),
      originalUuid = crypto.randomUUID.bind(crypto);
    let randomness = 0;
    crypto.getRandomValues = () => {
      randomness++;
      throw Error('new recovery randomness');
    };
    crypto.randomUUID = () => {
      randomness++;
      throw Error('new recovery command');
    };
    let recovered: Awaited<ReturnType<typeof r.recover>>;
    try {
      recovered = await r.recover(reader);
    } finally {
      crypto.getRandomValues = originalRandom;
      crypto.randomUUID = originalUuid;
    }
    const again = await r.recover(reader),
      binding = r.binding(),
      after = f.counts();
    return {
      status: recovered.status,
      again: again.status,
      sameProof:
        'proof' in recovered &&
        'proof' in again &&
        recovered.proof === again.proof,
      sameReader: reader === repeat,
      before,
      inert,
      after,
      rowsSame: JSON.stringify(rows) === JSON.stringify(await f.stored()),
      receivers: receiver.receivers(),
      randomness,
      binding: binding && {
        command: binding.command,
        rumorHash: binding.rumorHash,
        eventId: binding.eventId,
        keys: Object.keys(binding)
      },
      oldNonceGone: !window.pr.inboxProbeRumorWire(
        f.probeIdentity()!,
        f.session
      ),
      readiness: window.pr.inboxAccessControllerSnapshot(r.access)?.readiness
    };
  });
  expect(result.status).toBe('recovered');
  expect(result.again).toBe('recovered');
  expect(result.sameProof).toBe(true);
  expect(result.sameReader).toBe(true);
  expect(result.inert).toEqual(result.before);
  expect(result.after.decrypts - result.before.decrypts).toBe(2);
  expect(result.after.keys - result.before.keys).toBe(3);
  expect(result.after.signs).toBe(result.before.signs);
  expect(result.after.encrypts).toBe(result.before.encrypts);
  expect(result.randomness).toBe(0);
  expect(result.receivers).toBe(2);
  expect(result.rowsSame).toBe(true);
  expect(result.oldNonceGone).toBe(true);
  expect(result.readiness).toBe('unavailable');
  expect(result.binding?.keys).not.toContain('rumorWire');
  expect(privateFrames()).toHaveLength(traffic);
});
for (const bad of [
  'review',
  'id',
  'repository',
  'identity',
  'session',
  'policy',
  'controller',
  'operator',
  'unconfigured',
  'unsupported',
  'copy'
])
  test(`inert recovery capture rejects ${bad} before provider or native work`, async ({
    page
  }) => {
    await fresh(page);
    const result = await page.evaluate((bad) => {
      const f = window.prFixture,
        r = window.prFresh,
        before = f.counts();
      let native = 0;
      const original = Object.getOwnPropertyDescriptor(
        IDBDatabase.prototype,
        'transaction'
      )!.value as IDBDatabase['transaction'];
      IDBDatabase.prototype.transaction = function (
        ...args: Parameters<IDBDatabase['transaction']>
      ) {
        native++;
        return original.apply(this, args);
      };
      let reader;
      try {
        reader = window.pr.captureInboxProbeRecoveryReader(
          bad === 'repository' ? f.foreignRepository() : r.repository,
          bad === 'identity' ? f.identity : r.identity,
          bad === 'session' ? f.session : r.session,
          bad === 'policy' ? f.copiedPolicy() : r.policy,
          bad === 'controller'
            ? f.controller()!
            : bad === 'copy'
              ? { ...r.access }
              : r.access,
          bad === 'operator'
            ? ({} as typeof r.operator)
            : bad === 'unconfigured'
              ? f.operator(false, true)
              : bad === 'unsupported'
                ? f.operator(true)
                : r.operator,
          bad === 'id' ? 'not-original' : r.localId,
          bad === 'review' ? true : 'reviewed_inbox_probe_recovery'
        );
      } finally {
        IDBDatabase.prototype.transaction = original;
      }
      return { admitted: !!reader, before, after: f.counts(), native };
    }, bad);
    expect(result.admitted).toBe(false);
    expect(result.after).toEqual(result.before);
    expect(result.native).toBe(0);
  });
test('invalid controller/session/policy associations never retry actual retained failed private cleanup', async ({
  page
}) => {
  const result = await page.evaluate(() => {
    const f = window.prFixture;
    let retries = 0;
    const off = window.pr.subscribePrivateSessionClose(f.session, () => {
      retries++;
      throw Error('retained original cleanup');
    });
    window.pr.closePrivateSession(f.session);
    const before = retries;
    const fake = {} as Parameters<
      typeof window.pr.inboxAccessConstructionBinding
    >[0];
    const absent = window.pr.inboxAccessConstructionBinding(
        fake,
        f.session,
        f.policy
      ),
      wrongPolicy = window.pr.inboxAccessConstructionBinding(
        f.controller()!,
        f.session,
        f.copiedPolicy()
      ),
      wrongSession = window.pr.inboxAccessConstructionBinding(
        f.controller()!,
        fake as unknown as Parameters<
          typeof window.pr.inboxAccessConstructionBinding
        >[1],
        f.policy
      );
    const matching = window.pr.inboxAccessConstructionBinding(
      f.controller()!,
      f.session,
      f.policy
    );
    const wrongIdentity = {} as typeof f.identity;
    const refused = window.pr.captureInboxProbeRecoveryReader(
      f.repository,
      wrongIdentity,
      f.session,
      f.policy,
      f.controller()!,
      f.qualification,
      '11111111-1111-4111-8111-111111111111',
      'reviewed_inbox_probe_recovery'
    );
    const correctIdentity = window.pr.captureInboxProbeRecoveryReader(
      f.repository,
      f.identity,
      f.session,
      f.policy,
      f.controller()!,
      f.qualification,
      '11111111-1111-4111-8111-111111111111',
      'reviewed_inbox_probe_recovery'
    );
    const after = retries,
      retained = window.pr.privateSessionCleanupRequired(f.session);
    off();
    window.pr.closePrivateSession(f.session);
    return {
      before,
      after,
      retained,
      admitted: !!(
        absent ||
        wrongPolicy ||
        wrongSession ||
        matching ||
        refused ||
        correctIdentity
      )
    };
  });
  expect(result.before).toBeGreaterThan(0);
  expect(result.after).toBe(result.before);
  expect(result.retained).toBe(true);
  expect(result.admitted).toBe(false);
});
test('one selected original reader prevents second-command/reset and copied/cross-family capability admission', async ({
  page
}) => {
  await fresh(page);
  const result = await page.evaluate(async () => {
    const r = window.prFresh,
      reader = r.capture()!,
      other = r.capture('reviewed_inbox_probe_recovery', crypto.randomUUID());
    const recovered = await r.recover(reader);
    if (!('proof' in recovered)) throw Error('genuine recovered family');
    const copies = await window.pr.recoverInboxProbe({ ...reader }),
      ordinary = window.pr.recoveredSelfEnvelopeSnapshot(
        recovered.proof as unknown as Parameters<
          typeof window.pr.recoveredSelfEnvelopeSnapshot
        >[0]
      ),
      nonce = window.pr.inboxProbeNonceProofBinding(
        recovered.proof as unknown as Parameters<
          typeof window.pr.inboxProbeNonceProofBinding
        >[0],
        r.repository,
        r.session,
        r.policy
      ),
      copied = window.pr.recoveredInboxProbeBinding(
        { ...recovered.proof },
        r.repository,
        r.session,
        r.access,
        r.policy,
        r.operator
      );
    return {
      other: !!other,
      copies,
      ordinary: !!ordinary,
      nonce: !!nonce,
      copied: !!copied
    };
  });
  expect(result).toEqual({
    other: false,
    copies: { status: 'invalid' },
    ordinary: false,
    nonce: false,
    copied: false
  });
});
for (const mutation of [
  'r0',
  'missing-self',
  'ordinary',
  'corrupt',
  'alias',
  'tail',
  'binding',
  'hash',
  'time',
  'disappeared'
])
  test(`actual native ${mutation} cannot substitute original recovery custody`, async ({
    page
  }) => {
    await fresh(page);
    const result = await page.evaluate(async (mutation) => {
      const f = window.prFixture,
        r = window.prFresh,
        row = (await f.stored()).find((x) => x.id === r.localId)!,
        record = JSON.parse(row.wire) as Record<string, unknown>,
        before = f.counts();
      if (mutation === 'r0') {
        record.revision = 0;
        record.self = null;
        record.receipts = [];
      }
      if (mutation === 'missing-self') record.self = null;
      if (mutation === 'ordinary') {
        record.family = 'private_send_reservation';
        delete record.binding;
        delete record.self;
        delete record.receipts;
        record.revision = 0;
        record.peer = f.owner;
      }
      if (mutation === 'binding')
        (record.binding as { preferenceId: string }).preferenceId = 'f'.repeat(
          64
        );
      if (mutation === 'hash') record.rumorHash = 'f'.repeat(64);
      if (mutation === 'time') record.createdAt = Number(record.createdAt) + 1;
      if (mutation === 'alias')
        await f.inject([{ ...row, id: crypto.randomUUID() }]);
      else if (mutation === 'tail')
        await f.inject([
          { owner: row.owner, id: crypto.randomUUID(), wire: '{' }
        ]);
      else if (mutation === 'disappeared') await f.removeOriginal();
      else
        await f.inject([
          {
            ...row,
            wire: mutation === 'corrupt' ? '{' : JSON.stringify(record)
          }
        ]);
      const rows = await f.stored(),
        recovered = await r.recover();
      return {
        status: recovered.status,
        before,
        after: f.counts(),
        rowsSame: JSON.stringify(rows) === JSON.stringify(await f.stored()),
        proof: !!r.binding()
      };
    }, mutation);
    expect(result.status).not.toBe('recovered');
    expect(result.rowsSame).toBe(true);
    expect(result.proof).toBe(false);
    expect(result.after.signs).toBe(result.before.signs);
    expect(result.after.encrypts).toBe(result.before.encrypts);
    if (!['hash', 'time'].includes(mutation))
      expect(result.after.decrypts).toBe(result.before.decrypts);
  });
test('valid handwritten prior unknown/network labels permit only local decryption and confer no remote readiness', async ({
  page
}) => {
  await page.evaluate(async () => {
    const f = window.prFixture;
    const result = await f.publish();
    await f.append(result);
  });
  await fresh(page);
  const result = await page.evaluate(async () => {
    const f = window.prFixture,
      r = window.prFresh,
      row = (await f.stored()).find((x) => x.id === r.localId)!,
      record = JSON.parse(row.wire) as { receipts: { status: string }[] };
    record.receipts[0].status = 'unknown';
    await f.inject([{ ...row, wire: JSON.stringify(record) }]);
    const rows = await f.stored(),
      result = await r.recover();
    return {
      status: result.status,
      same: JSON.stringify(rows) === JSON.stringify(await f.stored()),
      readiness: window.pr.inboxAccessControllerSnapshot(r.access)?.readiness
    };
  });
  expect(result).toEqual({
    status: 'recovered',
    same: true,
    readiness: 'unavailable'
  });
});
for (const mode of ['wrong_key', 'refuse_decrypt', 'wrong_decrypt'] as const)
  test(`actual provider ${mode} consumes the original action without a repeated prompt`, async ({
    page
  }) => {
    await fresh(page);
    const result = await page.evaluate(async (mode) => {
      const f = window.prFixture,
        r = window.prFresh;
      f.mode(mode);
      const before = f.counts(),
        first = await r.recover(),
        after = f.counts();
      f.mode('normal');
      const again = await r.recover();
      return {
        first: first.status,
        again: again.status,
        before,
        after,
        final: f.counts()
      };
    }, mode);
    expect(result.first).not.toBe('recovered');
    expect(result.again).not.toBe('recovered');
    expect(result.final).toEqual(result.after);
    if (mode === 'wrong_key')
      expect(result.after.decrypts).toBe(result.before.decrypts);
  });
for (const stage of ['key', 'decrypt'] as const)
  for (const loss of [
    'stop',
    'expire',
    'head',
    'controller',
    'private',
    'identity'
  ] as const)
    test(`pending real ${stage} ${loss} hides recovery and retains the actual shared occupied slot until settlement`, async ({
      page
    }) => {
      await fresh(page);
      await page.evaluate((stage) => {
        window.prFixture.mode(stage === 'key' ? 'hold_key' : 'hold_decrypt');
        window.prPending = window.prFresh.recover();
      }, stage);
      await expect
        .poll(() => page.evaluate(() => window.prFixture.waiting()))
        .toBe(stage);
      const result = await page.evaluate(async (loss) => {
        const f = window.prFixture,
          r = window.prFresh,
          reader = r.reader()!;
        if (loss === 'stop') window.pr.stopInboxProbeRecoveryReader(reader);
        if (loss === 'expire')
          window.pr.expireInboxProbeRecoveryReaderWait(reader);
        if (loss === 'head') r.churnHead();
        if (loss === 'controller')
          window.pr.closeInboxAccessController(r.access);
        if (loss === 'private') window.pr.closePrivateSession(r.session);
        if (loss === 'identity') window.pr.disconnectIdentity(r.identity);
        const occupied = window.pr.extensionSchedulerSnapshot(
          window.pr.browserExtensionScheduler()!
        )?.state;
        const busy = await window.pr.runExtensionAction(
          window.pr.browserExtensionScheduler()!,
          { owner: r.owner, session: Symbol(), operation: Symbol() },
          () => true,
          () => Promise.resolve('foreign')
        );
        f.mode('normal');
        f.settle();
        const result = await window.prPending;
        const after = f.counts();
        const repeated = await r.recover(reader);
        return {
          occupied,
          busy: busy.status,
          result,
          repeated: repeated.status,
          unchanged: JSON.stringify(after) === JSON.stringify(f.counts()),
          proof: !!r.binding(),
          final: window.pr.extensionSchedulerSnapshot(
            window.pr.browserExtensionScheduler()!
          )?.state
        };
      }, loss);
      expect(result.occupied).toBe('active');
      expect(result.busy).toBe('busy');
      expect(result.result).not.toMatchObject({ status: 'recovered' });
      expect(result.repeated).not.toBe('recovered');
      expect(result.proof).toBe(false);
      expect(result.unchanged).toBe(true);
      expect(result.final).toBe('idle');
    });
test('actual owner lock busy admits zero SDK calls and explicit same-reader execution recovers after release', async ({
  page
}) => {
  await fresh(page);
  await page.evaluate(() => {
    const r = window.prFresh;
    window.prPending = navigator.locks.request(
      'harvestcircle:owner:' + r.owner,
      () =>
        new Promise<void>((resolve) => {
          window.prRelease = resolve;
        })
    );
  });
  await expect.poll(() => page.evaluate(() => !!window.prRelease)).toBe(true);
  const first = await page.evaluate(async () => {
    const f = window.prFixture,
      r = window.prFresh,
      reader = r.capture()!,
      before = f.counts(),
      result = await r.recover(reader);
    return {
      result,
      equal: JSON.stringify(before) === JSON.stringify(f.counts()),
      same: reader === r.capture()
    };
  });
  expect(first).toEqual({
    result: { status: 'busy' },
    equal: true,
    same: true
  });
  await page.evaluate(async () => {
    window.prRelease!();
    await window.prPending;
  });
  expect(
    await page.evaluate(async () => (await window.prFresh.recover()).status)
  ).toBe('recovered');
});
test('actual shared scheduler busy permits one explicit original-operation run after release', async ({
  page
}) => {
  await fresh(page);
  await page.evaluate(() => {
    window.prPending = window.pr.runExtensionAction(
      window.pr.browserExtensionScheduler()!,
      { owner: window.prFresh.owner, session: Symbol(), operation: Symbol() },
      () => true,
      () =>
        new Promise<void>((resolve) => {
          window.prRelease = resolve;
        })
    );
  });
  await expect.poll(() => page.evaluate(() => !!window.prRelease)).toBe(true);
  const result = await page.evaluate(async () => {
    const f = window.prFixture,
      r = window.prFresh,
      before = f.counts(),
      first = await r.recover();
    return {
      first: first.status,
      same: r.reader() === r.capture(),
      equal: JSON.stringify(before) === JSON.stringify(f.counts())
    };
  });
  expect(result).toEqual({ first: 'busy', same: true, equal: true });
  await page.evaluate(async () => {
    window.prRelease!();
    await window.prPending;
  });
  expect(
    await page.evaluate(async () => (await window.prFresh.recover()).status)
  ).toBe('recovered');
});
for (const hit of [1, 2, 3])
  for (const loss of ['native', 'head', 'controller', 'private', 'identity'])
    test(`actual fresh-key${hit} ${loss} mutation cannot precede a stale recovered proof`, async ({
      page
    }) => {
      await fresh(page);
      const result = await page.evaluate(
        async ({ hit, loss }) => {
          const f = window.prFixture,
            r = window.prFresh;
          window.prRestore = f.keyCallback(hit, async () => {
            if (loss === 'native') {
              const row = (await f.stored()).find((x) => x.id === r.localId)!,
                record = JSON.parse(row.wire) as {
                  binding: { preferenceId: string };
                };
              record.binding.preferenceId = 'f'.repeat(64);
              await f.inject([{ ...row, wire: JSON.stringify(record) }]);
            }
            if (loss === 'head') r.churnHead();
            if (loss === 'controller')
              window.pr.closeInboxAccessController(r.access);
            if (loss === 'private') window.pr.closePrivateSession(r.session);
            if (loss === 'identity') window.pr.disconnectIdentity(r.identity);
          });
          const result = await r.recover();
          return { status: result.status, proof: !!r.binding() };
        },
        { hit, loss }
      );
      expect(result.status).not.toBe('recovered');
      expect(result.proof).toBe(false);
    });
test('cipher getter scope loss preserves current identity and independent private session', async ({
  page
}) => {
  await fresh(page);
  const result = await page.evaluate(async () => {
    const f = window.prFixture,
      r = window.prFresh;
    window.prRestore = f.cipherGetter(() =>
      window.pr.closeInboxAccessController(r.access)
    );
    const before = f.counts(),
      result = await r.recover();
    return {
      status: result.status,
      noDecrypt: before.decrypts === f.counts().decrypts,
      identity:
        window.pr.identityMessagingOwnership(r.identity)?.current() ?? false,
      session: window.pr.privateSessionSnapshot(r.session)?.current ?? false
    };
  });
  expect(result.status).not.toBe('recovered');
  expect(result.noDecrypt).toBe(true);
  expect(result.identity).toBe(true);
  expect(result.session).toBe(true);
});
test('invalid fresh key never acquires the cipher getter', async ({ page }) => {
  await fresh(page);
  const result = await page.evaluate(async () => {
    const f = window.prFixture,
      r = window.prFresh;
    let getters = 0;
    window.prRestore = f.cipherGetter(() => {
      getters++;
    });
    f.mode('wrong_key');
    const before = f.counts(),
      result = await r.recover();
    return {
      status: result.status,
      getters,
      decrypts: f.counts().decrypts - before.decrypts
    };
  });
  expect(result.status).not.toBe('recovered');
  expect(result.getters).toBe(0);
  expect(result.decrypts).toBe(0);
});
test('pending duplicate/reentrant capture keeps the same reader and admits no second SDK action', async ({
  page
}) => {
  await fresh(page);
  await page.evaluate(() => {
    window.prFixture.mode('hold_decrypt');
    window.prPending = window.prFresh.recover();
  });
  await expect
    .poll(() => page.evaluate(() => window.prFixture.waiting()))
    .toBe('decrypt');
  const result = await page.evaluate(async () => {
    const f = window.prFixture,
      r = window.prFresh,
      before = f.counts(),
      reader = r.reader(),
      duplicate = await r.recover();
    const same = reader === r.capture(),
      inert = JSON.stringify(before) === JSON.stringify(f.counts());
    f.mode('normal');
    f.settle();
    const original = await window.prPending;
    return {
      duplicate: duplicate.status,
      same,
      inert,
      original,
      decrypts: f.counts().decrypts - before.decrypts
    };
  });
  expect(result.duplicate).toBe('busy');
  expect(result.same).toBe(true);
  expect(result.inert).toBe(true);
  expect(result.original).toMatchObject({ status: 'recovered' });
  expect(result.decrypts).toBe(1);
});
for (const loss of [
  'stop',
  'expire',
  'head',
  'controller',
  'private',
  'identity'
])
  test(`genuine recovered proof revokes all original rumor visibility on ${loss}`, async ({
    page
  }) => {
    await fresh(page);
    const result = await page.evaluate(async (loss) => {
      const r = window.prFresh,
        result = await r.recover();
      if (!('proof' in result)) throw Error('actual recovered proof');
      const visible = !!window.pr.recoveredInboxProbeRumorWire(
        result.proof,
        r.repository,
        r.session,
        r.access,
        r.policy,
        r.operator
      );
      if (loss === 'stop') window.pr.stopInboxProbeRecoveryReader(r.reader()!);
      if (loss === 'expire')
        window.pr.expireInboxProbeRecoveryReaderWait(r.reader()!);
      if (loss === 'head') r.churnHead();
      if (loss === 'controller') window.pr.closeInboxAccessController(r.access);
      if (loss === 'private') window.pr.closePrivateSession(r.session);
      if (loss === 'identity') window.pr.disconnectIdentity(r.identity);
      return {
        visible,
        binding: !!r.binding(),
        rumor: !!window.pr.recoveredInboxProbeRumorWire(
          result.proof,
          r.repository,
          r.session,
          r.access,
          r.policy,
          r.operator
        )
      };
    }, loss);
    expect(result).toEqual({ visible: true, binding: false, rumor: false });
  });
for (const malformed of [
  'id',
  'author',
  'time',
  'kind',
  'recipient',
  'prefix',
  'nonce',
  'extra',
  'duplicate',
  'numeric',
  'oversized'
])
  test(`actual SDK recovered diagnostic ${malformed} cannot mint original proof`, async ({
    page
  }) => {
    await fresh(page);
    const result = await page.evaluate(async (malformed) => {
      const f = window.prFixture,
        r = window.prFresh;
      const control = f.decryptCallback(2, (wire) => {
        const rumor = JSON.parse(wire) as Record<string, unknown>;
        if (malformed === 'id') rumor.id = 'f'.repeat(64);
        if (malformed === 'author') rumor.pubkey = 'f'.repeat(64);
        if (malformed === 'time')
          rumor.created_at = Number(rumor.created_at) + 1;
        if (malformed === 'kind') rumor.kind = 1;
        if (malformed === 'recipient') rumor.tags = [['p', 'f'.repeat(64)]];
        if (malformed === 'prefix')
          rumor.content = String(rumor.content).replace(
            'Harvestcircle',
            'Other'
          );
        if (malformed === 'nonce')
          rumor.content = String(rumor.content).slice(0, -1) + 'Z';
        if (malformed === 'extra') rumor.extra = true;
        if (malformed === 'oversized') rumor.content = 'x'.repeat(8193);
        const changed = JSON.stringify(rumor);
        return malformed === 'duplicate'
          ? changed.replace('"kind":14', '"kind":14,"kind":14')
          : malformed === 'numeric'
            ? changed.replace('"kind":14', '"kind":1.4e1')
            : changed;
      });
      window.prRestore = control.restore;
      const before = await f.stored(),
        result = await r.recover();
      return {
        status: result.status,
        proof: !!r.binding(),
        same: JSON.stringify(before) === JSON.stringify(await f.stored()),
        receivers: control.receivers()
      };
    }, malformed);
    expect(result.status).toBe('mismatch');
    expect(result.proof).toBe(false);
    expect(result.same).toBe(true);
    expect(result.receivers).toBe(2);
  });
for (const malformed of ['tags', 'kind', 'ciphertext'])
  test(`actual independently signed inner seal ${malformed} is rejected`, async ({
    page
  }) => {
    await fresh(page);
    const result = await page.evaluate(async (malformed) => {
      const f = window.prFixture,
        r = window.prFresh;
      const control = f.decryptCallback(1, (wire) =>
        f.signSeal(
          wire,
          malformed === 'tags'
            ? { tags: [['p', f.owner]] }
            : malformed === 'kind'
              ? { kind: 1 }
              : { content: 'unsupported ciphertext' }
        )
      );
      window.prRestore = control.restore;
      const result = await r.recover();
      return {
        status: result.status,
        proof: !!r.binding(),
        receivers: control.receivers()
      };
    }, malformed);
    expect(result.status).toBe('mismatch');
    expect(result.proof).toBe(false);
    expect(result.receivers).toBe(1);
  });
for (const hit of [1, 2])
  for (const fault of ['before', 'after'])
    test(`actual native readonly${hit} ${fault} abort refuses recovered authority`, async ({
      page
    }) => {
      await fresh(page);
      const result = await page.evaluate(
        async ({ hit, fault }) => {
          const original = Object.getOwnPropertyDescriptor(
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
          window.prRestore = () => {
            IDBIndex.prototype.openCursor = original;
          };
          const result = await window.prFresh.recover();
          return {
            status: result.status,
            aborts,
            proof: !!window.prFresh.binding()
          };
        },
        { hit, fault }
      );
      expect(result.status).not.toBe('recovered');
      expect(result.aborts).toBe(1);
      expect(result.proof).toBe(false);
    });
test('actual same-origin child reload recovers stored ciphertext through a fresh source realm and extension-host closure', async ({
  page
}) => {
  await page.evaluate(() => window.prFixture.close());
  head = await page.evaluate(() => {
    window.prHosted = window.pr.createHostedProvider();
    return window.prHosted.head();
  });
  const setup = await page.evaluate(() => {
    const policyWire = JSON.stringify({
      schemaVersion: 1,
      public: [
        {
          origin: 'wss://discovery.example.org',
          read: true,
          write: true,
          nip50: false
        }
      ],
      inbox: [{ origin: 'wss://inbox.example.org', read: true, write: true }],
      postingEnabled: true,
      messagingEnabled: true,
      operatorDenylist: []
    });
    const operatorWire = JSON.stringify({
      schemaVersion: 1,
      revision: 1,
      authority: 'source_fixture',
      policyWire,
      evidenceSha256: 'a'.repeat(64),
      relays: [
        {
          origin: 'wss://inbox.example.org',
          auth: 'not_required',
          recipientRead: 'supported',
          namedArchiveAck: 'supported',
          exactReadback: 'supported',
          retention: 'operator_defined'
        }
      ]
    });
    return { owner: window.prHosted!.owner, policyWire, operatorWire };
  });
  await page.evaluate((url) => {
    const iframe = document.createElement('iframe');
    iframe.id = 'recovery-realm';
    iframe.src = url;
    document.body.append(iframe);
  }, server.url + '/search');
  const frame = await (
    await page.locator('#recovery-realm').elementHandle()
  ).contentFrame();
  if (!frame) throw Error('actual same-origin child');
  await frame.waitForLoadState();
  await frame.addScriptTag({ content: bundle });
  const original = await frame.evaluate(async (setup) => {
    window.pr.installHostedProvider();
    const policy = window.pr.validateRelayPolicy(setup.policyWire)!,
      operator = window.pr.validateInboxOperatorQualification(
        setup.operatorWire
      )!;
    window.prHostedContext = await window.pr.openRecoveryContext(
      setup.owner,
      policy,
      operator,
      crypto.randomUUID()
    );
    const record = await window.pr.prepareHostedOriginal(
      window.prHostedContext
    );
    window.prHostedContext.close();
    return {
      owner: record.owner,
      id: record.id,
      hash: record.rumorHash,
      eventId: record.self!.eventId,
      policyWire: setup.policyWire,
      operatorWire: setup.operatorWire
    };
  }, setup);
  for (let cycle = 0; cycle < 2; cycle++) {
    await frame.goto(server.url + '/search');
    await frame.addScriptTag({ content: bundle });
    const before = await page.evaluate(() => window.prHosted!.counts());
    const result = await frame.evaluate(async (setup) => {
      window.pr.installHostedProvider();
      const policy = window.pr.validateRelayPolicy(setup.policyWire)!,
        operator = window.pr.validateInboxOperatorQualification(
          setup.operatorWire
        )!;
      window.prFresh = await window.pr.openRecoveryContext(
        setup.owner,
        policy,
        operator,
        setup.id
      );
      const result = await window.prFresh.recover(),
        binding = window.prFresh.binding();
      const output = {
        status: result.status,
        command: binding?.command,
        hash: binding?.rumorHash,
        eventId: binding?.eventId
      };
      window.prFresh.close();
      return output;
    }, original);
    expect(result).toEqual({
      status: 'recovered',
      command: original.id,
      hash: original.hash,
      eventId: original.eventId
    });
    const after = await page.evaluate(() => window.prHosted!.counts());
    expect(after.signs).toBe(before.signs);
    expect(after.encrypts - before.encrypts).toBe(1); // Actual fresh capability probe; recovery creates no artifact.
    expect(after.decrypts - before.decrypts).toBe(3); // Fresh capability probe plus original two-layer recovery.
  }
});
