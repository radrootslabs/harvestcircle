import { test, expect } from '@playwright/test';
import { build } from 'vite';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-probe-nonce.ts';
declare global {
  interface Window {
    nc: typeof Fixture;
    ncFixture: Awaited<ReturnType<typeof Fixture.makeNonceFixture>>;
    ncPending?: Promise<unknown>;
    ncAbort?: AbortController;
    ncOtherPool?: Fixture.PrivatePool;
    ncRestore?: () => void;
    ncAdvancePage?: () => void;
    ncAdvanceOnSocket?: 'network' | 'page';
    ncClock?: number;
    ncNetworkClock?: ReturnType<Window['ncFixture']['installNetworkClock']>;
    ncDelayClose?: boolean;
    ncFailClose?: boolean;
    ncInvalidateOnSocket?: boolean;
    ncNestedCleanup?: number;
    ncNestedFailure?: number;
    ncHeldClose?: (() => void)[];
    ncNoncePending?: Promise<unknown>;
    ncNonceOther?: Promise<unknown>;
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
          new URL('./harness/inbox-probe-nonce.ts', import.meta.url)
        ),
        name: 'nc',
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
            window.ncAdvanceOnSocket
          ) {
            if (window.ncAdvanceOnSocket === 'network')
              window.ncNetworkClock!.advance(45001);
            else window.ncAdvancePage!();
          }
          if (
            window.ncInvalidateOnSocket &&
            origin === 'wss://inbox.example.org'
          )
            window.nc.closeInboxAccessController(
              window.ncFixture.controller()!
            );
        }
        override close(code?: number, reason?: string) {
          if (window.ncFailClose)
            throw Error('actual controlled disposal failure');
          if (window.ncDelayClose)
            (window.ncHeldClose ??= []).push(() => super.close(code, reason));
          else super.close(code, reason);
        }
      };
    },
    { endpoint }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(async (multi) => {
    window.ncFixture = await window.nc.makeNonceFixture();
    return window.ncFixture.preferenceWire(
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
    const f = window.ncFixture;
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
      window.ncRestore?.();
      window.ncFailClose = false;
      window.ncDelayClose = false;
      for (const release of window.ncHeldClose ?? []) release();
      window.ncFixture.close();
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
    const f = window.ncFixture,
      result = await f.publish();
    await f.append(result);
    return result;
  });
}
async function readOriginal(page: import('@playwright/test').Page) {
  await publishOriginal(page);
  expect(
    await page.evaluate(async () => {
      const f = window.ncFixture,
        result = await f.read();
      const saved = await f.appendRead(result);
      return { read: result.status, saved: saved.status };
    })
  ).toEqual({ read: 'readback', saved: 'saved' });
}
test('cold genuine B2 readback verifies original nonce by guarded SDK without an earlier ordinary send, plaintext exposure, row growth or extra traffic', async ({
  page
}) => {
  await readOriginal(page);
  const observed = await page.evaluate(async () => {
    const f = window.ncFixture,
      rows = await f.stored(),
      before = f.counts(),
      first = f.nonceCapture()!,
      second = f.nonceCapture();
    const inert = f.counts();
    const receiver = f.decryptCallback(0, (x) => x);
    const result = await f.nonce(first),
      after = f.counts(),
      again = await f.nonce(first),
      binding = f.proofBinding();
    const copies = {
      reader: await window.nc.readInboxProbeNonce({ ...first }),
      proof: window.nc.inboxProbeNonceProofBinding(
        { ...result.proof! },
        f.repository,
        f.session,
        f.policy
      ),
      ordinary: window.nc.recoveredSelfEnvelopeSnapshot(
        result.proof! as unknown as Parameters<
          typeof window.nc.recoveredSelfEnvelopeSnapshot
        >[0]
      )
    };
    return {
      same: first === second,
      before,
      inert,
      after,
      result,
      again,
      binding: binding && {
        owner: binding.owner,
        command: binding.command,
        rumorHash: binding.rumorHash,
        eventId: binding.eventId,
        keys: Object.keys(binding)
      },
      sameProof: result.proof === again.proof,
      receivers: receiver.receivers(),
      rowsSame: JSON.stringify(rows) === JSON.stringify(await f.stored()),
      copies,
      readiness: window.nc.inboxAccessControllerSnapshot(f.controller()!)
        ?.readiness
    };
  });
  expect(observed.same).toBe(true);
  expect(observed.inert).toEqual(observed.before);
  expect(observed.result.status).toBe('nonce_verified');
  expect(observed.again.status).toBe('nonce_verified');
  expect(observed.sameProof).toBe(true);
  expect(observed.after.decrypts - observed.before.decrypts).toBe(2);
  expect(observed.after.keys - observed.before.keys).toBe(3);
  expect(observed.receivers).toBe(2);
  expect(observed.rowsSame).toBe(true);
  expect(observed.binding?.keys).not.toContain('rumorWire');
  expect(observed.binding?.keys).not.toContain('sealWire');
  expect(observed.binding?.keys).not.toContain('nonce');
  expect(observed.copies).toEqual({
    reader: { status: 'invalid' },
    proof: undefined,
    ordinary: undefined
  });
  expect(observed.readiness).toBe('unavailable');
  expect(privateEvents()).toHaveLength(1);
  expect(privateReads()).toHaveLength(1);
});
for (const review of [
  undefined,
  true,
  'reviewed_self_decryption',
  'reviewed_inbox_probe_commit'
])
  test(`nonce capture refuses wrong explicit review ${String(review)} without effects`, async ({
    page
  }) => {
    await readOriginal(page);
    expect(
      await page.evaluate((review) => {
        const f = window.ncFixture,
          before = f.counts();
        const reader = window.nc.captureInboxProbeNonceReader(
          f.repository,
          f.identity,
          f.session,
          f.policy,
          f.lastRead()!,
          review
        );
        return {
          captured: !!reader,
          unchanged: JSON.stringify(before) === JSON.stringify(f.counts())
        };
      }, review)
    ).toEqual({ captured: false, unchanged: true });
  });
for (const fault of [
  'copy',
  'archive',
  'foreign_repository',
  'foreign_session',
  'copied_policy',
  'wrong_identity',
  'ordinary_family'
])
  test(`nonce admission rejects ${fault} original association`, async ({
    page
  }) => {
    await readOriginal(page);
    expect(
      await page.evaluate(async (fault) => {
        const f = window.ncFixture,
          before = f.counts();
        let identity = f.identity,
          session = f.session,
          repository = f.repository,
          policy = f.policy,
          observation = f.lastRead()!;
        if (fault === 'copy') observation = { ...observation };
        if (fault === 'archive')
          observation = f.last() as unknown as typeof observation;
        if (fault === 'foreign_repository') repository = f.foreignRepository();
        if (fault === 'foreign_session') session = await f.foreignSession();
        if (fault === 'copied_policy') policy = f.copiedPolicy();
        if (fault === 'wrong_identity')
          identity = window.nc.createIdentitySession();
        if (fault === 'ordinary_family')
          observation = {
            status: 'readback',
            eventId: f.snapshot()!.record.self!.eventId
          };
        const baseline = f.counts(),
          reader = window.nc.captureInboxProbeNonceReader(
            repository,
            identity,
            session,
            policy,
            observation,
            'reviewed_inbox_probe_nonce_decryption'
          );
        return {
          captured: !!reader,
          unchanged: JSON.stringify(baseline) === JSON.stringify(f.counts()),
          initial: !!before
        };
      }, fault)
    ).toEqual({ captured: false, unchanged: true, initial: true });
  });
for (const fault of [
  'removed',
  'alias',
  'corrupt',
  'handwritten_fact',
  'corrupt_tail'
])
  test(`nonce initial native custody rejects ${fault} before SDK`, async ({
    page
  }) => {
    await readOriginal(page);
    const result = await page.evaluate(async (fault) => {
      const f = window.ncFixture,
        reader = f.nonceCapture()!,
        before = f.counts(),
        row = (await f.stored())[0];
      if (fault === 'removed') await f.removeOriginal();
      else if (fault === 'alias')
        await f.inject([{ ...row, wire: row.wire + '\n' }]);
      else if (fault === 'corrupt') await f.inject([{ ...row, wire: '{}' }]);
      else if (fault === 'handwritten_fact') await f.changedNativeFact();
      else
        await f.inject([
          { owner: f.owner, id: crypto.randomUUID(), wire: '{}' }
        ]);
      const result = await f.nonce(reader);
      return {
        status: result.status,
        unchanged: JSON.stringify(before) === JSON.stringify(f.counts()),
        proof: !!f.proofBinding()
      };
    }, fault);
    expect(result.status).not.toBe('nonce_verified');
    expect(result.unchanged).toBe(true);
    expect(result.proof).toBe(false);
  });
for (const mode of [
  'wrong_key',
  'refuse_decrypt',
  'wrong_decrypt',
  'stop_decrypt_getter'
] as const)
  test(`actual provider ${mode} refuses nonce proof`, async ({ page }) => {
    await readOriginal(page);
    const result = await page.evaluate(async (mode) => {
      const f = window.ncFixture,
        reader = f.nonceCapture()!;
      f.mode(mode);
      const result = await f.nonce(reader);
      return { status: result.status, proof: !!f.proofBinding() };
    }, mode);
    expect(result.status).not.toBe('nonce_verified');
    expect(result.proof).toBe(false);
    expect(privateEvents()).toHaveLength(1);
    expect(privateReads()).toHaveLength(1);
  });
for (const fault of [
  'outer_huge',
  'seal_signature',
  'seal_tags',
  'seal_kind',
  'rumor_owner',
  'rumor_time',
  'rumor_hash',
  'rumor_body',
  'rumor_huge',
  'rumor_serialization'
])
  test(`actual decrypted ${fault} cannot mint a nonce proof`, async ({
    page
  }) => {
    await readOriginal(page);
    const result = await page.evaluate(async (fault) => {
      const f = window.ncFixture,
        outer = fault.startsWith('outer') || fault.startsWith('seal');
      f.decryptCallback(outer ? 1 : 2, async (wire) => {
        if (fault.endsWith('huge')) return 'x'.repeat(outer ? 16385 : 8193);
        if (fault === 'rumor_serialization') return wire + '\n';
        if (fault === 'seal_tags')
          return f.signSeal(wire, { tags: [['p', f.owner]] });
        if (fault === 'seal_kind') return f.signSeal(wire, { kind: 14 });
        const parsed = JSON.parse(wire) as Record<string, unknown>;
        if (fault === 'seal_signature') {
          parsed.sig = '0'.repeat(128);
          parsed.verified = true;
        }
        if (fault === 'rumor_owner') parsed.pubkey = 'f'.repeat(64);
        if (fault === 'rumor_time')
          parsed.created_at = Number(parsed.created_at) + 1;
        if (fault === 'rumor_hash') parsed.id = 'f'.repeat(64);
        if (fault === 'rumor_body')
          parsed.content =
            'Harvestcircle private inbox access check v1: ' + '0'.repeat(64);
        return JSON.stringify(parsed);
      });
      const result = await f.nonce();
      return { status: result.status, proof: !!f.proofBinding() };
    }, fault);
    expect(result.status).toBe('mismatch');
    expect(result.proof).toBe(false);
  });
for (const stage of [1, 2, 3])
  for (const fault of ['head', 'controller', 'session', 'native'])
    test(`actual key ${stage} ${fault} churn prevents nonce proof`, async ({
      page
    }) => {
      await readOriginal(page);
      const result = await page.evaluate(
        async ({ stage, fault }) => {
          const f = window.ncFixture,
            reader = f.nonceCapture()!;
          f.keyCallback(stage, async () => {
            if (fault === 'head') f.churnHead();
            else if (fault === 'controller')
              window.nc.closeInboxAccessController(f.controller()!);
            else if (fault === 'session') f.closeSession();
            else await f.changedNativeFact();
          });
          const result = await f.nonce(reader);
          return { status: result.status, proof: !!f.proofBinding() };
        },
        { stage, fault }
      );
      expect(result.status).not.toBe('nonce_verified');
      expect(result.proof).toBe(false);
    });
test('actual cipher getter currentness loss cannot call captured decrypt method', async ({
  page
}) => {
  await readOriginal(page);
  const result = await page.evaluate(async () => {
    const f = window.ncFixture,
      reader = f.nonceCapture()!,
      before = f.counts();
    f.cipherGetter(() => f.churnHead());
    const result = await f.nonce(reader);
    return {
      status: result.status,
      calls: f.counts().decrypts - before.decrypts,
      proof: !!f.proofBinding(),
      identityCurrent: !!window.nc
        .identityMessagingOwnership(f.identity)
        ?.current(),
      privateCurrent: !!window.nc.privateSessionOwnership(f.session)?.current()
    };
  });
  expect(result).toEqual({
    status: 'stopped',
    calls: 0,
    proof: false,
    identityCurrent: true,
    privateCurrent: true
  });
});
for (const stop of ['stop', 'expire', 'head', 'controller', 'session'])
  test(`pending real SDK decrypt ${stop} retains original slot and refuses late proof`, async ({
    page
  }) => {
    await readOriginal(page);
    await page.evaluate(() => {
      const f = window.ncFixture;
      f.mode('hold_decrypt');
      window.ncNoncePending = f.nonce();
    });
    await expect
      .poll(() => page.evaluate(() => window.ncFixture.waiting()))
      .toBe('decrypt');
    const pending = await page.evaluate(async (stop) => {
      const f = window.ncFixture,
        reader = f.nonceReader()!;
      if (stop === 'stop') window.nc.stopInboxProbeNonceReader(reader);
      else if (stop === 'expire')
        window.nc.expireInboxProbeNonceReaderWait(reader);
      else if (stop === 'head') f.churnHead();
      else if (stop === 'controller')
        window.nc.closeInboxAccessController(f.controller()!);
      else f.closeSession();
      const slot = window.nc.browserExtensionScheduler()!,
        before = window.nc.extensionSchedulerSnapshot(slot),
        another = await window.nc.runExtensionAction(
          slot,
          { owner: f.owner, session: Symbol(), operation: Symbol() },
          () => true,
          () => Promise.resolve('bad')
        );
      const proof = !!f.proofBinding();
      f.settle();
      return { before, another, proof };
    }, stop);
    expect(pending.before).toMatchObject({
      state: 'active',
      pending: 'decrypt'
    });
    expect(pending.another).toEqual({ status: 'busy' });
    expect(pending.proof).toBe(false);
    expect(
      await page.evaluate(async () => {
        await window.ncNoncePending;
        return {
          proof: !!window.ncFixture.proofBinding(),
          slot: window.nc.extensionSchedulerSnapshot(
            window.nc.browserExtensionScheduler()!
          )
        };
      })
    ).toEqual({ proof: false, slot: { state: 'idle' } });
  });
for (const stop of [
  'stop',
  'expire',
  'head',
  'controller',
  'session',
  'identity'
])
  test(`genuine memory proof revokes on ${stop} without persisted nonce label or cleanup`, async ({
    page
  }) => {
    await readOriginal(page);
    const result = await page.evaluate(async (stop) => {
      const f = window.ncFixture;
      const rows = await f.stored();
      await f.nonce();
      const before = !!f.proofBinding();
      if (stop === 'stop')
        window.nc.stopInboxProbeNonceReader(f.nonceReader()!);
      else if (stop === 'expire')
        window.nc.expireInboxProbeNonceReaderWait(f.nonceReader()!);
      else if (stop === 'head') f.churnHead();
      else if (stop === 'controller')
        window.nc.closeInboxAccessController(f.controller()!);
      else if (stop === 'identity') window.nc.disconnectIdentity(f.identity);
      else f.closeSession();
      return {
        before,
        after: !!f.proofBinding(),
        retained: JSON.stringify(rows) === JSON.stringify(await f.stored())
      };
    }, stop);
    expect(result).toEqual({ before: true, after: false, retained: true });
  });
test('parallel and callback reentry share original observation reader and never start another SDK action', async ({
  page
}) => {
  await readOriginal(page);
  await page.evaluate(() => {
    const f = window.ncFixture;
    const original = f.nonceCapture()!;
    f.keyCallback(1, () => {
      if (f.nonceCapture() !== original)
        throw Error('reentrant reader changed');
      window.ncNonceOther = window.nc.readInboxProbeNonce(original);
    });
    f.mode('hold_decrypt');
    window.ncNoncePending = f.nonce(original);
  });
  await expect
    .poll(() => page.evaluate(() => window.ncFixture.waiting()))
    .toBe('decrypt');
  expect(
    await page.evaluate(async () => {
      const f = window.ncFixture,
        other = await window.ncNonceOther,
        parallel = await window.nc.readInboxProbeNonce(f.nonceReader()!);
      f.mode('normal');
      f.settle();
      return { other, parallel };
    })
  ).toEqual({ other: { status: 'busy' }, parallel: { status: 'busy' } });
  expect(
    await page.evaluate(async () => {
      const result = (await window.ncNoncePending) as { status: string };
      return result.status;
    })
  ).toBe('nonce_verified');
});

test('genuine ordinary and nonce reader brands cannot cross and ordinary probe-row reading remains invalid_scope', async ({
  page
}) => {
  await readOriginal(page);
  const result = await page.evaluate(async () => {
    const f = window.ncFixture,
      nonce = f.nonceCapture()!,
      ordinary = window.nc.captureSelfRecoveryReader(
        f.repository,
        f.identity,
        f.snapshot()!.record.id,
        'reviewed_self_decryption'
      )!;
    const before = f.counts();
    const nonceFromOrdinary = await window.nc.readInboxProbeNonce(
        ordinary as unknown as Parameters<
          typeof window.nc.readInboxProbeNonce
        >[0]
      ),
      ordinaryFromNonce = await window.nc.readSelfRecoveryEnvelope(
        nonce as unknown as Parameters<
          typeof window.nc.readSelfRecoveryEnvelope
        >[0]
      ),
      originalOrdinary = await window.nc.readSelfRecoveryEnvelope(ordinary);
    return {
      genuine: !!ordinary && !!nonce,
      nonceFromOrdinary,
      ordinaryFromNonce,
      originalOrdinary,
      unchanged: JSON.stringify(before) === JSON.stringify(f.counts())
    };
  });
  expect(result).toEqual({
    genuine: true,
    nonceFromOrdinary: { status: 'invalid' },
    ordinaryFromNonce: { status: 'invalid' },
    originalOrdinary: { status: 'invalid_scope' },
    unchanged: true
  });
});
test('forged read result cannot retry cleanup of a genuine closed private session', async ({
  page
}) => {
  await readOriginal(page);
  expect(
    await page.evaluate(() => {
      const f = window.ncFixture;
      let retries = 0;
      const off = window.nc.subscribePrivateSessionClose(f.session, () => {
        retries++;
        throw Error('controlled retained original cleanup');
      });
      f.closeSession();
      const before = retries;
      const reader = window.nc.captureInboxProbeNonceReader(
        f.repository,
        f.identity,
        f.session,
        f.policy,
        { status: 'readback' },
        'reviewed_inbox_probe_nonce_decryption'
      );
      const after = retries,
        cleanup = window.nc.privateSessionCleanupRequired(f.session);
      off();
      return { captured: !!reader, unchanged: before === after, cleanup };
    })
  ).toEqual({ captured: false, unchanged: true, cleanup: true });
});
for (const fault of [
  'provider',
  'key_method',
  'cipher',
  'decrypt_method',
  'throw_cipher',
  'cipher_version'
])
  test(`missing or invalid actual ${fault} never authorizes nonce proof`, async ({
    page
  }) => {
    await readOriginal(page);
    const result = await page.evaluate(async (fault) => {
      const f = window.ncFixture,
        reader = f.nonceCapture()!,
        provider = Reflect.get(window, 'nostr') as {
          getPublicKey(): Promise<string>;
          nip44: { decrypt(target: string, content: string): Promise<string> };
        },
        descriptor = Object.getOwnPropertyDescriptor(window, 'nostr')!;
      const cipher = provider.nip44,
        cipherDescriptor = Object.getOwnPropertyDescriptor(provider, 'nip44')!,
        keyDescriptor = Object.getOwnPropertyDescriptor(
          provider,
          'getPublicKey'
        )!,
        decryptDescriptor = Object.getOwnPropertyDescriptor(cipher, 'decrypt')!;
      try {
        if (fault === 'provider')
          Object.defineProperty(window, 'nostr', {
            configurable: true,
            value: undefined
          });
        else if (fault === 'key_method')
          Object.defineProperty(provider, 'getPublicKey', {
            configurable: true,
            value: undefined
          });
        else if (fault === 'cipher')
          Object.defineProperty(provider, 'nip44', {
            configurable: true,
            value: undefined
          });
        else if (fault === 'decrypt_method')
          Object.defineProperty(cipher, 'decrypt', {
            configurable: true,
            value: undefined
          });
        else if (fault === 'throw_cipher')
          Object.defineProperty(provider, 'nip44', {
            configurable: true,
            get() {
              throw Error('controlled actual capability getter');
            }
          });
        else
          f.decryptCallback(1, (wire) =>
            f.signSeal(wire, { content: 'not NIP44v2' })
          );
        const result = await f.nonce(reader);
        return { status: result.status, proof: !!f.proofBinding() };
      } finally {
        Object.defineProperty(window, 'nostr', descriptor);
        Object.defineProperty(provider, 'nip44', cipherDescriptor);
        Object.defineProperty(provider, 'getPublicKey', keyDescriptor);
        Object.defineProperty(cipher, 'decrypt', decryptDescriptor);
      }
    }, fault);
    expect(result.status).not.toBe('nonce_verified');
    expect(result.proof).toBe(false);
  });
test('same-owner replacement genuine identity has no original private session nonce admission', async ({
  page
}) => {
  await readOriginal(page);
  expect(
    await page.evaluate(async () => {
      const f = window.ncFixture,
        replacement = window.nc.createIdentitySession();
      await window.nc.connectIdentity(replacement);
      await window.nc.probeIdentityMessaging(replacement, 'reviewed_self_copy');
      const sameOwner =
          window.nc.identityMessagingOwnership(replacement)?.owner === f.owner,
        before = f.counts();
      const reader = window.nc.captureInboxProbeNonceReader(
        f.repository,
        replacement,
        f.session,
        f.policy,
        f.lastRead()!,
        'reviewed_inbox_probe_nonce_decryption'
      );
      const unchanged = JSON.stringify(before) === JSON.stringify(f.counts());
      window.nc.disconnectIdentity(replacement);
      return { captured: !!reader, sameOwner, unchanged };
    })
  ).toEqual({ captured: false, sameOwner: true, unchanged: true });
});
for (const hit of [1, 2])
  test(`actual native readonly abort ${hit} refuses nonce proof`, async ({
    page
  }) => {
    await readOriginal(page);
    const result = await page.evaluate(async (hit) => {
      const f = window.ncFixture,
        reader = f.nonceCapture()!,
        original = Object.getOwnPropertyDescriptor(
          IDBDatabase.prototype,
          'transaction'
        )!.value as IDBDatabase['transaction'];
      let reads = 0,
        aborted = 0;
      IDBDatabase.prototype.transaction = function (
        ...args: Parameters<IDBDatabase['transaction']>
      ) {
        const tx = original.apply(this, args);
        if (
          args[1] === 'readonly' &&
          tx.objectStoreNames.contains('private_sends') &&
          ++reads === hit
        ) {
          aborted++;
          queueMicrotask(() => tx.abort());
        }
        return tx;
      };
      try {
        const result = await f.nonce(reader);
        return { status: result.status, proof: !!f.proofBinding(), aborted };
      } finally {
        IDBDatabase.prototype.transaction = original;
      }
    }, hit);
    expect(result.status).not.toBe('nonce_verified');
    expect(result.proof).toBe(false);
    expect(result.aborted).toBe(1);
  });
test('actual occupied owner lock prevents SDK nonce work', async ({ page }) => {
  await readOriginal(page);
  expect(
    await page.evaluate(async () => {
      const f = window.ncFixture,
        reader = f.nonceCapture()!;
      let entered: (() => void) | undefined, release: (() => void) | undefined;
      const ready = new Promise<void>((resolve) => {
        entered = resolve;
      });
      const held = navigator.locks.request(
        'harvestcircle:owner:' + f.owner,
        async () => {
          entered?.();
          await new Promise<void>((resolve) => {
            release = resolve;
          });
        }
      );
      await ready;
      const before = f.counts();
      try {
        const result = await f.nonce(reader);
        return {
          status: result.status,
          unchanged: JSON.stringify(before) === JSON.stringify(f.counts())
        };
      } finally {
        release?.();
        await held;
      }
    })
  ).toEqual({ status: 'busy', unchanged: true });
});
for (const fault of ['wrong', 'denied'])
  test(`actual ${fault} key refuses before any cipher getter acquisition`, async ({
    page
  }) => {
    await readOriginal(page);
    expect(
      await page.evaluate(async (fault) => {
        const f = window.ncFixture,
          reader = f.nonceCapture()!;
        let getters = 0;
        f.cipherGetter(() => {
          getters++;
        });
        if (fault === 'wrong') f.mode('wrong_key');
        else
          f.keyCallback(1, () => {
            throw Error('actual controlled key refusal');
          });
        const before = f.counts(),
          result = await f.nonce(reader);
        return {
          status: result.status,
          getters,
          decrypts: f.counts().decrypts - before.decrypts,
          identityCurrent: !!window.nc
            .identityMessagingOwnership(f.identity)
            ?.current()
        };
      }, fault)
    ).toEqual({
      status: 'stopped',
      getters: 0,
      decrypts: 0,
      identityCurrent: false
    });
  });
