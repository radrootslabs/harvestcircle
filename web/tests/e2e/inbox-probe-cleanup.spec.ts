import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-probe-cleanup.ts';
import type { PrivateAccessProbe } from '../../src/lib/persistence/private-records.ts';
declare global {
  interface Window {
    pc: typeof Fixture;
    pcFixture: Awaited<ReturnType<typeof Fixture.makeCleanupFixture>>;
    pcPending?: Promise<unknown>;
    pcRestore?: () => void;
    pcFinishAttempt?: () => Promise<string>;
    pcRelease?: () => void;
    pcCloseMode?: 'hold' | 'fail';
    pcHeldClose?: (() => void)[];
    pcCloseCalls?: number;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string,
  head = '';
let ackMode: 'accept' | 'refuse' | 'hold' = 'accept';
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
      socket.send(JSON.stringify(['AUTH', 'c3b-source-challenge']));
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
          new URL('./harness/inbox-probe-cleanup.ts', import.meta.url)
        ),
        name: 'pc',
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
test.beforeEach(async ({ page }) => {
  await expect.poll(() => physical.size).toBe(0);
  ackMode = 'accept';
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
          window.pcCloseCalls = (window.pcCloseCalls ?? 0) + 1;
          if (window.pcCloseMode === 'fail')
            throw Error('actual controlled physical close failure');
          if (window.pcCloseMode === 'hold')
            (window.pcHeldClose ??= []).push(() => super.close(code, reason));
          else super.close(code, reason);
        }
      };
    },
    { endpoint, origins }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(async (origins) => {
    window.pcFixture = await window.pc.makeCleanupFixture();
    return window.pcFixture.host.preferenceWire(origins);
  }, origins);
  await page.evaluate(async () => {
    const f = window.pcFixture;
    await f.open();
    if ((await f.prepareOriginal()).status !== 'prepared')
      throw Error('actual original S3/R1');
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(async () => {
      window.pcRestore?.();
      window.pcCloseMode = undefined;
      for (const release of window.pcHeldClose ?? []) release();
      window.pcRelease?.();
      window.pcFixture.host.mode('normal');
      window.pcFixture.host.settle();
      window.pcFixture.close();
      await window.pcPending;
    });
});

async function qualify(page: Page, recovered = false, skip: string[] = []) {
  expect(
    await page.evaluate(
      async ({ recovered, skip }) =>
        (await window.pcFixture.qualify(recovered, { skip })).status,
      { recovered, skip }
    )
  ).toBe('saved');
}
async function select(page: Page) {
  expect(
    await page.evaluate(async () => !!(await window.pcFixture.select()))
  ).toBe(true);
}
const traffic = () =>
  frames.filter(
    (x) =>
      x.origin !== 'wss://discovery.example.org' &&
      ['EVENT', 'REQ', 'AUTH'].includes(String(x.frame[0]))
  );

for (const recovered of [false, true]) {
  test(`${recovered ? 'recovered' : 'current-original'} reviewed genuine cleanup has native completion, fresh absence, exact disposal and independent removal provenance`, async ({
    page
  }) => {
    await qualify(page, recovered);
    const before = traffic().length;
    const result = await page.evaluate(async (recovered) => {
      const f = window.pcFixture,
        a = f.actors(),
        record = (await f.host.stored())[0],
        nonce = f.nonce()!,
        proof = f.nonceProof()!,
        original = f.originalIdentity(),
        oldAck = recovered ? f.acknowledgement()! : f.originalAck()!,
        controller = window.pc.inboxAccessConstructionBinding(
          a.access,
          a.session,
          f.host.policy
        ),
        counts = f.host.counts();
      const selected = await f.select();
      const capturedCounts = f.host.counts(),
        capturedRows = await f.host.stored();
      const hook = f.nativeFault('wire_race');
      hook.restore();
      const removed = await f.remove(),
        after = await f.host.stored(),
        binding = f.removalBinding(),
        nonceBinding = window.pc.inboxProbeNonceAcknowledgementBinding(
          f.repository,
          a.session,
          nonce,
          f.host.policy
        ),
        proofBinding = window.pc.inboxProbeNonceVerificationEvidence(
          proof,
          f.repository,
          a.session,
          f.host.policy
        ),
        oldBinding = recovered
          ? window.pc.recoveredInboxProbePublicationBinding(
              f.repository,
              a.session,
              oldAck,
              f.host.policy
            )
          : window.pc.inboxProbePublicationBinding(
              f.repository,
              a.session,
              oldAck,
              f.host.policy
            );
      return {
        selected: !!selected,
        counts,
        capturedCounts,
        capturedSame:
          capturedRows.length === 1 && capturedRows[0].wire === record.wire,
        status: removed.status,
        removed: removed.removed,
        after: after.length,
        nonceCurrent: !!nonceBinding,
        proofCurrent: !!proofBinding,
        oldAckCurrent: !!oldBinding,
        originalNonce: original
          ? !!window.pc.inboxProbeRumorWire(original, a.session)
          : false,
        controller: controller?.current(),
        removal: binding?.current(),
        command: binding?.command,
        expected: record.id,
        keys: removed.acknowledgement && Object.keys(removed.acknowledgement),
        copy: !!window.pc.inboxProbeRemovalAcknowledgementBinding(
          { ...removed.acknowledgement! },
          f.repository,
          a.session,
          f.host.policy
        ),
        again: (await f.remove()).status,
        afterCounts: f.host.counts()
      };
    }, recovered);
    expect(result).toMatchObject({
      selected: true,
      capturedSame: true,
      status: 'removed',
      removed: 1,
      after: 0,
      nonceCurrent: false,
      proofCurrent: false,
      oldAckCurrent: false,
      originalNonce: false,
      controller: true,
      removal: true,
      keys: [],
      copy: false,
      again: 'invalid'
    });
    expect(result.command).toBe(result.expected);
    expect(result.capturedCounts).toEqual(result.counts);
    expect(result.afterCounts).toEqual({
      ...result.counts,
      keys: result.counts.keys + 1
    });
    expect(traffic()).toHaveLength(before);
    const reads = traffic().filter((x) => x.frame[0] === 'REQ');
    expect(new Set(reads.map((x) => x.origin))).toEqual(new Set([origins[0]]));
    expect(
      traffic()
        .filter((x) => x.frame[0] === 'EVENT')
        .map((x) => x.origin)
        .sort()
    ).toEqual(origins.slice().sort());
  });

  for (const missing of origins.slice(1))
    test(`${recovered ? 'recovered' : 'original'} every archive ACK is required even with genuine nonce on one approved read path: ${missing}`, async ({
      page
    }) => {
      await qualify(page, recovered, [missing]);
      const result = await page.evaluate(async () => {
        const f = window.pcFixture,
          before = await f.host.stored(),
          counts = f.host.counts(),
          token = await f.select();
        return {
          selected: !!token,
          same:
            JSON.stringify(before) === JSON.stringify(await f.host.stored()),
          counts,
          after: f.host.counts()
        };
      });
      expect(result.selected).toBe(false);
      expect(result.same).toBe(true);
      expect(result.after).toEqual(result.counts);
    });

  for (const held of ['lock', 'SDK'] as const)
    test(`${recovered ? 'recovered' : 'original'} genuine cleanup under actual occupied ${held} preserves row and permits fresh explicit selection`, async ({
      page
    }) => {
      await qualify(page, recovered);
      await select(page);
      const before = traffic().length;
      const result = await page.evaluate(async (held) => {
        const f = window.pcFixture,
          row = JSON.stringify(await f.host.stored()),
          counts = f.host.counts();
        const denied = await f.withHeld(held, () => f.remove());
        const after = f.host.counts(),
          same = JSON.stringify(await f.host.stored()) === row;
        await f.select();
        const later = await f.remove();
        return {
          denied: denied.status,
          later: later.status,
          counts,
          after,
          same
        };
      }, held);
      expect(result).toMatchObject({
        denied: 'busy',
        later: 'removed',
        same: true
      });
      expect(result.after).toEqual(result.counts);
      expect(traffic()).toHaveLength(before);
    });

  for (const mode of ['initial_read_abort', 'abort', 'wire_race'] as const)
    test(`${recovered ? 'recovered' : 'original'} definite native ${mode} never removes or retires and remains explicitly protected`, async ({
      page
    }) => {
      await qualify(page, recovered);
      await select(page);
      const result = await page.evaluate(async (mode) => {
        const f = window.pcFixture,
          before = (await f.host.stored())[0].wire,
          hook = f.nativeFault(mode);
        try {
          const denied = await f.remove();
          hook.restore();
          const rows = await f.host.stored();
          const selected = await f.select();
          return {
            status: denied.status,
            ack: !!denied.acknowledgement,
            count: rows.length,
            same: rows[0]?.wire === before,
            counts: hook.counts(),
            selected: !!selected,
            next: selected ? (await f.remove()).status : undefined
          };
        } finally {
          hook.restore();
        }
      }, mode);
      expect(result.ack).toBe(false);
      expect(result.count).toBe(1);
      if (mode === 'wire_race')
        expect(result).toMatchObject({
          status: 'conflict',
          selected: false,
          counts: { deletes: 0, races: 1 }
        });
      else {
        expect(result.same).toBe(true);
        expect(result.selected).toBe(true);
        expect(result.next).toBe('removed');
      }
      if (mode === 'abort')
        expect(result.counts).toMatchObject({
          deletes: 1,
          completes: 0,
          aborts: 1
        });
    });

  for (const mode of ['uncertain', 'read_before', 'read_after'] as const)
    test(`${recovered ? 'recovered' : 'original'} native ${mode} keeps conservative lease and never invents removal provenance`, async ({
      page
    }) => {
      await qualify(page, recovered);
      await select(page);
      const result = await page.evaluate(
        async ({ mode, recovered }) => {
          const f = window.pcFixture,
            a = f.actors(),
            ack = recovered ? f.acknowledgement()! : f.originalAck()!,
            action = recovered
              ? f.action()!
              : window.pc.captureInboxProbePublicationAction(
                  f.repository,
                  a.session,
                  f.host.policy,
                  ack,
                  'reviewed_inbox_probe_network'
                )!,
            hook = f.nativeFault(mode);
          try {
            const removed = await f.remove();
            hook.restore();
            const rows = (await f.host.stored()).length;
            return {
              status: removed.status,
              ack: !!removed.acknowledgement,
              counts: hook.counts(),
              rows,
              binding: !!f.removalBinding(),
              permission: !!window.pc.captureInboxProbePublication(
                f.repository,
                a.session,
                f.host.policy,
                ack,
                action,
                originsForBrowser()[0],
                'reviewed_inbox_probe_publication'
              )
            };
          } finally {
            hook.restore();
          }
          function originsForBrowser() {
            return ['wss://inbox.example.org'];
          }
        },
        { mode, recovered }
      );
      expect(result).toMatchObject({
        status: 'unknown_completion',
        ack: false,
        rows: 0,
        binding: false,
        permission: false,
        counts: { deletes: 1, completes: 1 }
      });
      if (mode === 'uncertain')
        expect(result.counts.refused).toBeGreaterThan(0);
    });

  for (const mode of [
    'read_loss',
    'native_pre_delete_loss',
    'delete_loss',
    'complete_loss'
  ] as const)
    test(`${recovered ? 'recovered' : 'original'} Stop at actual ${mode} cannot mint current removal or resurrect nonce authority`, async ({
      page
    }) => {
      await qualify(page, recovered);
      await select(page);
      const result = await page.evaluate(async (mode) => {
        const f = window.pcFixture,
          hook = f.nativeFault(mode, () => f.stopCleanup());
        try {
          const removed = await f.remove();
          hook.restore();
          const rows = (await f.host.stored()).length;
          return {
            status: removed.status,
            ack: !!removed.acknowledgement,
            counts: hook.counts(),
            rows,
            removedBinding: !!f.removalBinding()
          };
        } finally {
          hook.restore();
        }
      }, mode);
      expect(result.ack).toBe(false);
      expect(result.removedBinding).toBe(false);
      if (mode === 'read_loss' || mode === 'native_pre_delete_loss') {
        expect(result.counts.deletes).toBe(0);
        expect(result.rows).toBe(1);
      } else if (mode === 'complete_loss') {
        expect(result).toMatchObject({
          status: 'unknown_completion',
          rows: 0,
          counts: { deletes: 1, completes: 1 }
        });
      } else
        expect(result).toMatchObject({
          rows: 1,
          counts: { deletes: 1, aborts: 1 }
        });
    });

  for (const loss of ['head', 'controller', 'session'] as const)
    test(`${recovered ? 'recovered' : 'original'} actual ${loss} loss after capture prevents deletion`, async ({
      page
    }) => {
      await qualify(page, recovered);
      await select(page);
      const result = await page.evaluate(async (loss) => {
        const f = window.pcFixture;
        if (loss === 'head') f.actors().churnHead();
        else if (loss === 'controller') f.stop();
        else window.pc.closePrivateSession(f.actors().session);
        const removed = await f.remove();
        return {
          status: removed.status,
          rows: (await f.host.stored()).length,
          ack: !!removed.acknowledgement
        };
      }, loss);
      expect(result).toEqual({ status: 'stopped', rows: 1, ack: false });
    });

  test(`${recovered ? 'recovered' : 'original'} exact command lease fences a previously captured permission and new capture before native deletion`, async ({
    page
  }) => {
    await qualify(page, recovered);
    await select(page);
    const before = traffic().length;
    const result = await page.evaluate(async (recovered) => {
      const f = window.pcFixture,
        a = f.actors(),
        ack = recovered ? f.acknowledgement()! : f.originalAck()!,
        action = recovered
          ? f.action()!
          : window.pc.captureInboxProbePublicationAction(
              f.repository,
              a.session,
              f.host.policy,
              ack,
              'reviewed_inbox_probe_network'
            )!,
        permission = window.pc.captureInboxProbePublication(
          f.repository,
          a.session,
          f.host.policy,
          ack,
          action,
          'wss://peer.example.org',
          'reviewed_inbox_probe_publication'
        )!;
      const before = !!window.pc.inboxProbePublicationSnapshot(permission),
        counts = f.host.counts();
      let during = true,
        newPermission = true,
        pending: Promise<unknown> | undefined;
      const hook = f.nativeFault('read_loss', () => {
        during = !!window.pc.inboxProbePublicationSnapshot(permission);
        newPermission = !!window.pc.captureInboxProbePublication(
          f.repository,
          a.session,
          f.host.policy,
          ack,
          action,
          'wss://peer.example.org',
          'reviewed_inbox_probe_publication'
        );
        pending = window.pc.publishInboxProbeArchiveAttempt(
          f.pool()!,
          permission,
          new AbortController().signal
        );
      });
      try {
        const removed = await f.remove();
        return {
          before,
          during,
          newPermission,
          status: removed.status,
          attempt: await pending,
          counts,
          after: f.host.counts(),
          afterPermission: !!window.pc.inboxProbePublicationSnapshot(permission)
        };
      } finally {
        hook.restore();
      }
    }, recovered);
    expect(result).toMatchObject({
      before: true,
      during: false,
      newPermission: false,
      status: 'removed',
      attempt: { status: 'stopped' },
      afterPermission: false
    });
    expect(result.after).toEqual({
      ...result.counts,
      keys: result.counts.keys + 1
    });
    expect(traffic()).toHaveLength(before);
  });
}

test('same actual C7 original and recovered actions share retirement and clear both genuine source identities without closing C7', async ({
  page
}) => {
  await qualify(page, false);
  const result = await page.evaluate(async () => {
    const f = window.pcFixture,
      a = f.actors(),
      original = f.originalIdentity()!,
      ack = f.originalAck()!,
      proof = f.nonceProof()!;
    // A fresh recovered reader on the SAME controller/session, not teardown.
    const current = a.access;
    // Inbox already used archive+read; the shared three-attempt budget leaves
    // the second exact read on peer, which has only two archive attempts.
    await f.qualify(true, {
      fresh: false,
      readOrigin: 'wss://peer.example.org'
    });
    const same =
      f.actors().access === current && f.actors().session === a.session;
    await f.select();
    const removed = await f.remove();
    return {
      same,
      status: removed.status,
      originalNonce: !!window.pc.inboxProbeRumorWire(original, a.session),
      originalAck: !!window.pc.inboxProbePublicationBinding(
        f.repository,
        a.session,
        ack,
        f.host.policy
      ),
      originalProof: !!window.pc.inboxProbeNonceVerificationEvidence(
        proof,
        f.repository,
        a.session,
        f.host.policy
      ),
      recovered: !!f.recoveredBinding(),
      controller: !!window.pc.inboxAccessConstructionBinding(
        current,
        a.session,
        f.host.policy
      ),
      removal: f.removalBinding()?.current()
    };
  });
  expect(result).toEqual({
    same: true,
    status: 'removed',
    originalNonce: false,
    originalAck: false,
    originalProof: false,
    recovered: false,
    controller: true,
    removal: true
  });
});

test('100 genuine rows and command slots regain 101st admission only through actual cleanup in the SAME private session', async ({
  page
}) => {
  const result = await page.evaluate(async () => {
    const f = window.pcFixture,
      full = await f.capacity();
    if (full.denied.status !== 'capacity')
      throw Error('actual native 101st admission must refuse at100');
    await f.qualify(true, { fresh: false });
    await f.select();
    const removed = await f.remove(),
      selected = f.command()!,
      after = await f.host.stored();
    const preserved =
      after.length === 99 &&
      after.every((row) => row.wire === full.originals.get(row.id)) &&
      !after.some((row) => row.id === selected);
    await f.replaceController();
    const next = await f.prepareOriginal();
    const action =
      'acknowledgement' in next &&
      window.pc.captureInboxProbePublicationAction(
        f.repository,
        f.actors().session,
        f.host.policy,
        next.acknowledgement,
        'reviewed_inbox_probe_network'
      );
    return {
      denied: full.denied.status,
      removed: removed.status,
      preserved,
      next: next.status,
      action: !!action,
      sameSession: f.actors().session === full.session,
      rows: (await f.host.stored()).length
    };
  });
  expect(result).toEqual({
    denied: 'capacity',
    removed: 'removed',
    preserved: true,
    next: 'prepared',
    action: true,
    sameSession: true,
    rows: 100
  });
});

for (const recovered of [false, true]) {
  test(`${recovered ? 'recovered' : 'original'} copied and wrong actual issuer scopes cannot capture cleanup or native removal authority`, async ({
    page
  }) => {
    await qualify(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.pcFixture,
        a = f.actors(),
        opened = await window.pc.openBrowserDatabase();
      if (opened.state !== 'ready')
        throw Error('actual second repository scope');
      const second = window.pc.createPrivateStorageRepository(
          opened.owner,
          f.host.owner
        )!,
        foreign = f.host.foreignRepository(),
        policy = f.host.copiedPolicy(),
        observations = f.observations(),
        nonce = f.nonce()!,
        before = await f.host.stored(),
        counts = f.host.counts();
      const transaction = Object.getOwnPropertyDescriptor(
        IDBDatabase.prototype,
        'transaction'
      )!.value as IDBDatabase['transaction'];
      let native = 0;
      IDBDatabase.prototype.transaction = function (
        ...args: Parameters<IDBDatabase['transaction']>
      ) {
        native++;
        return transaction.apply(this, args);
      };
      try {
        const candidates = [
          [f.repository, a.session, f.host.policy, { ...nonce }, observations],
          [
            f.repository,
            a.session,
            f.host.policy,
            nonce,
            observations.map((x) => ({ ...x }))
          ],
          [f.repository, a.session, f.host.policy, nonce, []],
          [second, a.session, f.host.policy, nonce, observations],
          [foreign, a.session, f.host.policy, nonce, observations],
          [f.repository, a.session, policy, nonce, observations],
          [
            f.repository,
            {} as typeof a.session,
            f.host.policy,
            nonce,
            observations
          ]
        ] as const;
        const admitted = [];
        for (const [repo, session, selectedPolicy, ack, effects] of candidates)
          admitted.push(
            !!(await window.pc.captureInboxProbeCleanup(
              repo,
              session,
              selectedPolicy,
              ack,
              effects,
              'reviewed_inbox_probe_cleanup_selection'
            ))
          );
        const nativeResult = await window.pc.removePrivateInboxProbe(
          f.repository,
          a.session,
          nonce as unknown as Parameters<
            typeof window.pc.removePrivateInboxProbe
          >[2]
        );
        return {
          admitted,
          native,
          nativeResult,
          counts,
          after: f.host.counts(),
          rows: before.length
        };
      } finally {
        IDBDatabase.prototype.transaction = transaction;
        window.pc.closeBrowserDatabase(opened.owner);
      }
    });
    expect(result.admitted).toEqual(Array(7).fill(false));
    expect(result.native).toBe(0);
    expect(result.nativeResult).toEqual({
      ok: false,
      reason: 'invalid_scope',
      disposition: 'not_removed'
    });
    expect(result.after).toEqual(result.counts);
    expect(result.rows).toBe(1);
  });

  test(`${recovered ? 'recovered' : 'original'} fully qualified probe remains protected from ordinary settled-send cleanup`, async ({
    page
  }) => {
    await qualify(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.pcFixture,
        a = f.actors(),
        row = (await f.host.stored())[0],
        record = JSON.parse(row.wire) as PrivateAccessProbe;
      const ordinary = await window.pc.capturePrivateOutboxCleanup(
        f.repository,
        a.session,
        [row.id],
        'reviewed_settled_outbox_selection'
      );
      return {
        settlement: window.pc.privateRecordSettlement(record),
        ordinary: !!ordinary,
        same: (await f.host.stored())[0].wire === row.wire
      };
    });
    expect(result).toEqual({
      settlement: 'protected',
      ordinary: false,
      same: true
    });
  });

  for (const mode of [
    'alias',
    'corrupt_tail',
    'missing',
    'foreign_tail'
  ] as const)
    test(`${recovered ? 'recovered' : 'original'} complete namespace ${mode} preserves exact native and authority boundaries`, async ({
      page
    }) => {
      await qualify(page, recovered);
      await select(page);
      const result = await page.evaluate(async (mode) => {
        const f = window.pcFixture,
          original = (await f.host.stored())[0];
        let foreign: typeof original | undefined;
        if (mode === 'missing') await f.removeFixtureRow();
        else {
          const owner =
            mode === 'foreign_tail' ? 'f'.repeat(64) : original.owner;
          const row = {
            owner,
            id: crypto.randomUUID(),
            wire: mode === 'alias' ? original.wire : '{}'
          };
          if (mode === 'foreign_tail') foreign = row;
          await f.host.inject([row]);
        }
        const removed = await f.remove(),
          rows = await f.host.stored();
        return {
          status: removed.status,
          ack: !!removed.acknowledgement,
          rows: rows.length,
          original: rows.some(
            (x) => x.id === original.id && x.wire === original.wire
          ),
          foreign: foreign
            ? rows.some((x) => JSON.stringify(x) === JSON.stringify(foreign))
            : false
        };
      }, mode);
      if (mode === 'foreign_tail')
        expect(result).toMatchObject({
          status: 'removed',
          ack: true,
          rows: 1,
          original: false,
          foreign: true
        });
      else if (mode === 'missing')
        expect(result).toMatchObject({ ack: false, rows: 0, original: false });
      else
        expect(result).toMatchObject({
          status: 'corrupt_record',
          ack: false,
          rows: 2,
          original: true
        });
    });

  test(`${recovered ? 'recovered' : 'original'} current removal provenance follows actual head loss after deliberate nonce disposal`, async ({
    page
  }) => {
    await qualify(page, recovered);
    await select(page);
    const result = await page.evaluate(async () => {
      const f = window.pcFixture,
        removed = await f.remove(),
        before = !!f.removalBinding();
      f.actors().churnHead();
      return { status: removed.status, before, after: !!f.removalBinding() };
    });
    expect(result).toEqual({ status: 'removed', before: true, after: false });
  });
}

for (const failure of ['refused', 'unknown', 'stopped', 'timed_out'] as const)
  test(`actual ${failure} archive history needs later genuine same-origin event acceptance before reviewed cleanup`, async ({
    page
  }) => {
    ackMode =
      failure === 'refused'
        ? 'refuse'
        : failure === 'unknown'
          ? 'accept'
          : 'hold';
    await page.evaluate((failure) => {
      const f = window.pcFixture,
        controller = new AbortController(),
        fault =
          failure === 'unknown' ? f.installAttemptTeardownFailure() : undefined,
        clock = failure === 'timed_out' ? f.installNetworkClock() : undefined;
      window.pcRestore = () => {
        fault?.restore();
        clock?.restore();
        controller.abort();
      };
      const pending = f.originalPublish(
        'wss://peer.example.org',
        controller.signal
      );
      window.pcFinishAttempt = async () => {
        if (failure === 'stopped') controller.abort();
        if (failure === 'timed_out') clock!.advance(45000);
        const result = await pending;
        clock?.restore();
        fault?.restore();
        if (!result) throw Error('actual original negative archive result');
        const saved = await f.append(result);
        if (saved.status !== 'saved')
          throw Error('actual negative native history ' + saved.status);
        return result.status;
      };
    }, failure);
    await expect
      .poll(() => traffic().filter((x) => x.frame[0] === 'EVENT').length)
      .toBe(1);
    expect(await page.evaluate(() => window.pcFinishAttempt!())).toBe(failure);
    ackMode = 'accept';
    const result = await page.evaluate(async (failure) => {
      const f = window.pcFixture,
        session = f.actors().session;
      await f.replaceController();
      await f.qualify(true, { fresh: false });
      const record = JSON.parse((await f.host.stored())[0].wire) as {
          receipts: {
            status: string;
            origin: string;
            role: string;
            eventId: string;
          }[];
        },
        negative = record.receipts.findIndex((x) => x.status === failure),
        first = record.receipts[negative],
        positive = record.receipts.findIndex(
          (x, i) =>
            i > negative &&
            x.status === 'accepted' &&
            x.origin === first.origin &&
            x.role === first.role &&
            x.eventId === first.eventId
        );
      const selected = await f.select(),
        removed = await f.remove();
      return {
        sameSession: f.actors().session === session,
        negative,
        positive,
        selected: !!selected,
        status: removed.status
      };
    }, failure);
    expect(result.negative).toBeGreaterThanOrEqual(0);
    expect(result.positive).toBeGreaterThan(result.negative);
    expect(result).toMatchObject({
      sameSession: true,
      selected: true,
      status: 'removed'
    });
  });

test('earlier genuine positive and later readback cannot resolve a subsequent refused archive for the same origin', async ({
  page
}) => {
  await qualify(page, false);
  ackMode = 'refuse';
  expect(
    await page.evaluate(async () => {
      const f = window.pcFixture,
        result = await f.originalPublish('wss://peer.example.org');
      if (!result || result.status !== 'refused')
        throw Error('actual refused later publication');
      return (await f.append(result)).status;
    })
  ).toBe('saved');
  ackMode = 'accept';
  const result = await page.evaluate(async () => {
    const f = window.pcFixture,
      observations = f.observations(),
      read = await f.originalRead('wss://peer.example.org');
    if (
      !read ||
      read.status !== 'readback' ||
      (await f.appendRead(read)).status !== 'saved'
    )
      throw Error('actual different-role later read');
    const earlier = await f.select(),
      otherRole = await f.select('reviewed_inbox_probe_cleanup_selection', [
        ...observations,
        read
      ]);
    return {
      earlier: !!earlier,
      otherRole: !!otherRole,
      rows: (await f.host.stored()).length
    };
  });
  expect(result).toEqual({ earlier: false, otherRole: false, rows: 1 });
});

test('unapplied later actual archive result blocks cleanup of an otherwise genuine current qualified probe', async ({
  page
}) => {
  await qualify(page, false);
  await select(page);
  ackMode = 'refuse';
  expect(
    await page.evaluate(async () => {
      const f = window.pcFixture,
        result = await f.originalPublish('wss://peer.example.org');
      if (!result || result.status !== 'refused')
        throw Error('actual later negative result');
      // Deliberately do not commit this actual settled attempt to the journal.
      const current = !!window.pc.inboxProbeNonceAcknowledgementBinding(
          f.repository,
          f.actors().session,
          f.nonce()!,
          f.host.policy
        ),
        idle = f.idle(),
        before = f.host.counts(),
        removed = await f.remove();
      if (JSON.stringify(before) !== JSON.stringify(f.host.counts()))
        throw Error('pending preflight must precede every provider request');
      return {
        current,
        idle,
        status: removed.status,
        ack: !!removed.acknowledgement,
        rows: (await f.host.stored()).length
      };
    })
  ).toEqual({ current: true, idle: true, status: 'busy', ack: false, rows: 1 });
});

test('unapplied archive survives genuine ended-controller replacement and failed or forged native bookkeeping', async ({
  page
}) => {
  await qualify(page);
  await select(page);
  ackMode = 'refuse';
  const result = await page.evaluate(async () => {
    const f = window.pcFixture,
      a = f.actors(),
      actual = await f.originalPublish('wss://peer.example.org');
    if (!actual || actual.status !== 'refused') throw Error('actual negative');
    const forged = await f.append({ ...actual }),
      hook = f.nativeFault('initial_read_abort');
    const failed = await f.append(actual);
    hook.restore();
    const before = f.host.counts(),
      denied = await f.remove(),
      after = f.host.counts();
    await f.replaceController();
    if (
      (await f.recover())?.status !== 'recovered' ||
      (await f.adopt()).status !== 'adopted'
    )
      throw Error('genuine same-session recovery');
    const genuine = !!window.pc.recoveredInboxProbePublicationBinding(
      f.repository,
      a.session,
      f.acknowledgement()!,
      f.host.policy
    );
    let uuids = 0;
    const original = Object.getOwnPropertyDescriptor(
      Crypto.prototype,
      'randomUUID'
    )!.value as Crypto['randomUUID'];
    crypto.randomUUID = () => {
      uuids++;
      return original.call(crypto);
    };
    let replaced: boolean;
    try {
      replaced = !!(await f.captureAction());
    } finally {
      crypto.randomUUID = original;
    }
    // Historical genuine native bookkeeping may settle after its controller.
    const applied = await f.append(actual),
      existing = await f.append(actual);
    return {
      forged: forged.status,
      failed: failed.status,
      denied: denied.status,
      before,
      after,
      genuine,
      replaced,
      uuids,
      applied: applied.status,
      existing: existing.status,
      sameSession: a.session === f.actors().session,
      rows: (await f.host.stored()).length
    };
  });
  expect(result).toMatchObject({
    forged: 'invalid',
    denied: 'busy',
    genuine: true,
    replaced: false,
    uuids: 0,
    applied: 'saved',
    existing: 'existing',
    sameSession: true,
    rows: 1
  });
  expect(['saved', 'existing', 'reconciled']).not.toContain(result.failed);
  expect(result.after).toEqual(result.before);
});

for (const recovered of [false, true]) {
  for (const lane of ['EVENT', 'REQ', 'page', 'live', 'AUTH'] as const)
    test(`${recovered ? 'recovered' : 'original'} genuinely qualified cleanup refuses actual active ${lane} before provider or native removal`, async ({
      page
    }) => {
      await qualify(page, recovered);
      await select(page);
      if (lane === 'EVENT') ackMode = 'hold';
      if (lane === 'REQ' || lane === 'page' || lane === 'live') holdRead = true;
      if (lane === 'AUTH') {
        challenge = true;
        const started = await page.evaluate(async () => {
          const f = window.pcFixture;
          const result = await window.pc.authenticateInboxAccess(
            f.actors().access,
            'wss://inbox.example.org',
            'reviewed_inbox_connection_auth'
          );
          return {
            result,
            snapshot: window.pc.inboxAccessControllerSnapshot(f.actors().access)
          };
        });
        console.info(
          JSON.stringify({
            fixture: 'C3B_ACTUAL_AUTH_START',
            recovered,
            started
          })
        );
        expect(['waiting', 'challenge']).toContain(started.result);
        // Qualification may already own an open SDK connection. Deliver the
        // genuine relay challenge on that exact existing physical connection.
        for (const socket of physical)
          if (physicalOrigins.get(socket) === 'wss://inbox.example.org')
            socket.send(JSON.stringify(['AUTH', 'c3b-source-challenge']));
        await expect
          .poll(() =>
            page.evaluate(
              () =>
                window.pc.inboxAccessControllerSnapshot(
                  window.pcFixture.actors().access
                )?.auth[0]?.state
            )
          )
          .toBe('challenge');
      }
      const trafficBefore = traffic().length;
      await page.evaluate(
        ({ lane, recovered }) => {
          const f = window.pcFixture,
            a = f.actors();
          if (lane === 'EVENT')
            window.pcPending = recovered
              ? f.publish('wss://peer.example.org')
              : f.originalPublish('wss://peer.example.org');
          else if (lane === 'REQ')
            window.pcPending = recovered
              ? f.read('wss://peer.example.org')
              : f.originalRead('wss://peer.example.org');
          else if (lane === 'AUTH')
            window.pcPending = window.pc.respondInboxAccessAuthentication(
              a.access,
              'wss://inbox.example.org',
              'reviewed_inbox_auth_response'
            );
          else {
            const pool = window.pc.getPrivatePool(
              a.session,
              f.host.policy,
              ['wss://inbox.example.org'],
              Object.freeze({}),
              'receive'
            );
            if (!pool) throw Error('actual foreground consumer');
            const stop =
              lane === 'page'
                ? window.pc.subscribePrivatePage(pool, 1, () => {})
                : window.pc.subscribePrivateLive(pool, () => {});
            window.pcRelease = () => {
              stop();
              window.pc.closePrivatePool(pool);
            };
          }
        },
        { lane, recovered }
      );
      await expect.poll(() => traffic().length).toBeGreaterThan(trafficBefore);
      const framesBefore = frames.slice();
      const result = await page.evaluate(async () => {
        const f = window.pcFixture,
          a = f.actors(),
          current = !!window.pc.inboxProbeNonceAcknowledgementBinding(
            f.repository,
            a.session,
            f.nonce()!,
            f.host.policy
          ),
          before = f.host.counts(),
          hook = f.nativeFault('abort'),
          idle = f.idle();
        try {
          const removed = await f.remove();
          return {
            current,
            idle,
            controllerBusy: window.pc.inboxAccessControllerSnapshot(a.access)
              ?.busy,
            before,
            after: f.host.counts(),
            status: removed.status,
            ack: !!removed.acknowledgement,
            deletes: hook.counts().deletes,
            rows: (await f.host.stored()).length
          };
        } finally {
          hook.restore();
        }
      });
      expect(result).toMatchObject({
        current: true,
        // AUTH occupies the actual C7/shared SDK job; the pool's independent
        // network-idle observation intentionally remains true for this lane.
        idle: lane === 'AUTH',
        controllerBusy: lane === 'AUTH',
        status: 'busy',
        ack: false,
        deletes: 0,
        rows: 1
      });
      expect(result.after).toEqual(result.before);
      expect(frames).toEqual(framesBefore);
      // Release only this fixture's owned operation after the refusal check.
      if (lane === 'EVENT' || lane === 'AUTH') {
        const frame = traffic().findLast((x) => x.frame[0] === lane)
          ?.frame[1] as { id: string };
        for (const socket of physical)
          socket.send(JSON.stringify(['OK', frame.id, true, '']));
      } else if (lane === 'REQ') {
        for (const item of traffic()
          .slice(trafficBefore)
          .filter((x) => x.frame[0] === 'REQ'))
          for (const socket of physical)
            socket.send(JSON.stringify(['EOSE', item.frame[1]]));
      } else
        await page.evaluate(() => {
          window.pcRelease!();
          window.pcRelease = undefined;
        });
      await page.evaluate(() => window.pcPending);
    });

  for (const mode of ['hold', 'fail'] as const)
    test(`${recovered ? 'recovered' : 'original'} genuine cleanup preserves actual ${mode} physical close and never retries it implicitly`, async ({
      page
    }) => {
      await qualify(page, recovered);
      await select(page);
      await page.evaluate(async () => {
        const f = window.pcFixture,
          pool = window.pc.getPrivatePool(
            f.actors().session,
            f.host.policy,
            ['wss://inbox.example.org'],
            Object.freeze({}),
            'receive'
          );
        if (!pool) throw Error('actual receive owner');
        const auth = await window.pc.beginInboxAuthentication(
          pool,
          'wss://inbox.example.org',
          'reviewed_connection_auth'
        );
        if (!auth) throw Error('actual independently owned AUTH connection');
        window.pcRelease = () => {
          window.pc.closeInboxAuthentication(auth);
          window.pc.closePrivatePool(pool);
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
        const f = window.pcFixture;
        window.pcCloseMode = mode;
        let failed = false;
        try {
          window.pcRelease!();
        } catch {
          failed = true;
        }
        const current = !!window.pc.inboxProbeNonceAcknowledgementBinding(
            f.repository,
            f.actors().session,
            f.nonce()!,
            f.host.policy
          ),
          idle = f.idle(),
          before = f.host.counts(),
          closes = window.pcCloseCalls ?? 0,
          held = window.pcHeldClose?.length ?? 0;
        const removed = await f.remove();
        return {
          current,
          idle,
          failed,
          before,
          after: f.host.counts(),
          closes,
          afterCloses: window.pcCloseCalls ?? 0,
          held,
          afterHeld: window.pcHeldClose?.length ?? 0,
          status: removed.status,
          rows: (await f.host.stored()).length,
          ack: !!removed.acknowledgement
        };
      }, mode);
      expect(result).toMatchObject({
        current: true,
        idle: false,
        status: 'busy',
        rows: 1,
        ack: false
      });
      expect(result.after).toEqual(result.before);
      expect(result.afterCloses).toBe(result.closes);
      expect(result.afterHeld).toBe(result.held);
      if (mode === 'fail') expect(result.failed).toBe(true);
      else expect(result.held).toBeGreaterThan(0);
      await page.evaluate(() => {
        window.pcCloseMode = undefined;
        for (const release of window.pcHeldClose ?? []) release();
        try {
          window.pcRelease!();
        } catch (e) {
          if (
            !(e instanceof Error) ||
            ![
              'private_pool_close_pending',
              'private_pool_close_failed'
            ].includes(e.message)
          )
            throw e;
        }
      });
      await expect
        .poll(() => page.evaluate(() => window.pcFixture.idle()))
        .toBe(true);
      expect(
        await page.evaluate(async () => {
          await window.pcFixture.select();
          return (await window.pcFixture.remove()).status;
        })
      ).toBe('removed');
    });

  for (const loss of ['head', 'controller', 'session'] as const)
    for (const at of ['delete_loss', 'complete_loss'] as const)
      test(`${recovered ? 'recovered' : 'original'} actual ${loss} loss at native ${at} refuses current removal provenance`, async ({
        page
      }) => {
        await qualify(page, recovered);
        await select(page);
        const result = await page.evaluate(
          async ({ loss, at }) => {
            const f = window.pcFixture,
              hook = f.nativeFault(at, () => {
                if (loss === 'head') f.actors().churnHead();
                else if (loss === 'controller') f.stop();
                else window.pc.closePrivateSession(f.actors().session);
              });
            try {
              const removed = await f.remove();
              hook.restore();
              const rows = (await f.host.stored()).length;
              return {
                status: removed.status,
                ack: !!removed.acknowledgement,
                binding: !!f.removalBinding(),
                rows,
                counts: hook.counts()
              };
            } finally {
              hook.restore();
            }
          },
          { loss, at }
        );
        expect(result.ack).toBe(false);
        expect(result.binding).toBe(false);
        expect(result.counts.deletes).toBe(1);
        // A head change has no synchronous cancellation notification. The
        // already-admitted native delete may complete historically, but final
        // source revalidation must withhold current provenance and retirement.
        if (at === 'complete_loss' || loss === 'head')
          expect(result).toMatchObject({
            status: 'unknown_completion',
            rows: 0,
            counts: { completes: 1 }
          });
        else expect(result).toMatchObject({ rows: 1, counts: { aborts: 1 } });
      });

  test(`${recovered ? 'recovered' : 'original'} exact cleanup preserves same-owner settled send, received envelope and unrelated protected probe bytes`, async ({
    page
  }) => {
    await qualify(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.pcFixture,
        preserved = await f.seedPreservedRecords();
      await f.select();
      const removed = await f.remove(),
        sends = await f.host.stored(),
        received = await f.receivedRows();
      return { status: removed.status, preserved, sends, received };
    });
    expect(result.status).toBe('removed');
    expect(result.sends).toEqual(
      expect.arrayContaining(result.preserved.slice(0, 2))
    );
    expect(result.sends).toHaveLength(2);
    expect(result.received).toEqual([result.preserved[2]]);
  });
}

test('same-C7 recovered-first network action removes genuine row while old unregistered original cannot recreate it or emit EVENT', async ({
  page
}) => {
  // Initial S3/R1 exists, but no original network action has ever been captured.
  const result = await page.evaluate(async () => {
    const f = window.pcFixture,
      access = f.actors().access,
      original = f.originalIdentity()!;
    await f.qualify(true, { fresh: false });
    await f.select();
    const removed = await f.remove(),
      before = f.host.counts(),
      stillPrepared = !!window.pc.inboxProbeRumorWire(
        original,
        f.actors().session
      ),
      retry = await f.retryOriginalPreparation(),
      publication = await f.originalPublish('wss://peer.example.org');
    return {
      sameC7: access === f.actors().access,
      liveC7: !!window.pc
        .inboxAccessConstructionBinding(
          access,
          f.actors().session,
          f.host.policy
        )
        ?.current(),
      removed: removed.status,
      stillPrepared,
      retry: retry.status,
      retryAck: 'acknowledgement' in retry,
      publication,
      before,
      after: f.host.counts(),
      rows: (await f.host.stored()).length
    };
  });
  expect(result).toMatchObject({
    sameC7: true,
    liveC7: true,
    removed: 'removed',
    stillPrepared: true,
    retryAck: false,
    rows: 0
  });
  // This original identity was never associated with the retired REC meter.
  // Native absence rejects its real publication before EVENT admission.
  expect(result.publication).toEqual({ status: 'stopped' });
  expect(traffic().filter((x) => x.frame[0] === 'EVENT')).toHaveLength(3);
  // Unregistered preparation is honestly retained until actual C7 teardown.
  expect(
    await page.evaluate(() => {
      const f = window.pcFixture,
        original = f.originalIdentity()!;
      f.stop();
      return !!window.pc.inboxProbeRumorWire(original, f.actors().session);
    })
  ).toBe(false);
});

for (const recovered of [false, true]) {
  test(`${recovered ? 'recovered' : 'original'} bounded genuine observation array never reads or consumes a custom iterator`, async ({
    page
  }) => {
    await qualify(page, recovered);
    const result = await page.evaluate(async () => {
      const f = window.pcFixture,
        values = f.observations();
      let gets = 0,
        bodies = 0,
        steps = 0;
      Object.defineProperty(values, Symbol.iterator, {
        configurable: true,
        get() {
          gets++;
          return function* () {
            bodies++;
            // Finite real regression, never an infinite/hanging iterator.
            for (let index = 0; index < values.length + 2; index++) {
              steps++;
              yield values[index % values.length];
            }
          };
        }
      });
      const before = f.host.counts(),
        selected = await f.select(
          'reviewed_inbox_probe_cleanup_selection',
          values
        );
      return {
        selected: !!selected,
        gets,
        bodies,
        steps,
        declaredLength: values.length,
        before,
        after: f.host.counts(),
        rows: (await f.host.stored()).length
      };
    });
    expect(result).toMatchObject({
      selected: true,
      gets: 0,
      bodies: 0,
      steps: 0,
      declaredLength: 4,
      rows: 1
    });
    expect(result.after).toEqual(result.before);
  });

  for (const kind of [
    'accessor',
    'sparse',
    'inherited',
    'oversized',
    'non_integer',
    'unsafe_length'
  ] as const)
    test(`${recovered ? 'recovered' : 'original'} genuine cleanup refuses ${kind} observation data without native or provider work`, async ({
      page
    }) => {
      await qualify(page, recovered);
      const result = await page.evaluate(async (kind) => {
        const f = window.pcFixture,
          values = f.observations(),
          first = values[0],
          a = f.actors(),
          record = JSON.parse((await f.host.stored())[0].wire) as {
            receipts: unknown[];
          };
        let gets = 0,
          lengthDescriptors = 0;
        let input: typeof values = values;
        if (kind === 'accessor')
          Object.defineProperty(values, '0', {
            configurable: true,
            get() {
              gets++;
              return first;
            }
          });
        else if (kind === 'sparse') Reflect.deleteProperty(values, '0');
        else if (kind === 'inherited') {
          Reflect.deleteProperty(values, '0');
          const prototype: object = Object.create(Array.prototype) as object;
          Object.defineProperty(prototype, '0', {
            value: first,
            configurable: true
          });
          Object.setPrototypeOf(values, prototype);
        } else if (kind === 'oversized')
          values.length = record.receipts.length + 1;
        else
          input = new Proxy(values, {
            getOwnPropertyDescriptor(target, key) {
              const field = Reflect.getOwnPropertyDescriptor(target, key);
              if (key === 'length' && field) {
                lengthDescriptors++;
                return {
                  ...field,
                  value:
                    kind === 'non_integer' ? 1.5 : Number.MAX_SAFE_INTEGER + 1
                };
              }
              return field;
            }
          });
        const genuine = !!window.pc.inboxProbeNonceAcknowledgementBinding(
            f.repository,
            a.session,
            f.nonce()!,
            f.host.policy
          ),
          before = f.host.counts(),
          hook = f.nativeFault('abort');
        try {
          const selected = await f.select(
            'reviewed_inbox_probe_cleanup_selection',
            input
          );
          hook.restore();
          return {
            genuine,
            selected: !!selected,
            gets,
            lengthDescriptors,
            counts: hook.counts(),
            before,
            after: f.host.counts(),
            rows: (await f.host.stored()).length
          };
        } finally {
          hook.restore();
        }
      }, kind);
      expect(result).toMatchObject({
        genuine: true,
        selected: false,
        gets: 0,
        rows: 1,
        counts: { readonly: 0, deletes: 0 }
      });
      expect(result.after).toEqual(result.before);
      if (kind === 'non_integer' || kind === 'unsafe_length')
        expect(result.lengthDescriptors).toBe(1);
    });

  for (const loss of ['head', 'nonce'] as const)
    test(`${recovered ? 'recovered' : 'original'} observation descriptor callback actual ${loss} loss is fenced after bounded inspection`, async ({
      page
    }) => {
      await qualify(page, recovered);
      const result = await page.evaluate(async (loss) => {
        const f = window.pcFixture,
          a = f.actors(),
          original = (await f.host.stored())[0].wire,
          values = f.observations();
        let callbacks = 0,
          descriptors = 0;
        const input = new Proxy(values, {
          getOwnPropertyDescriptor(target, key) {
            descriptors++;
            if (key === '0' && callbacks === 0) {
              callbacks++;
              if (loss === 'head') a.churnHead();
              else
                window.pc.disposeInboxProbeNonceProof(
                  f.nonceProof()!,
                  f.repository,
                  a.session,
                  f.host.policy
                );
            }
            return Reflect.getOwnPropertyDescriptor(target, key);
          }
        });
        const genuine = !!window.pc.inboxProbeNonceAcknowledgementBinding(
            f.repository,
            a.session,
            f.nonce()!,
            f.host.policy
          ),
          before = f.host.counts(),
          hook = f.nativeFault('abort');
        try {
          const selected = await f.select(
            'reviewed_inbox_probe_cleanup_selection',
            input
          );
          hook.restore();
          return {
            genuine,
            selected: !!selected,
            callbacks,
            descriptors,
            limit: values.length + 1,
            before,
            after: f.host.counts(),
            counts: hook.counts(),
            same: (await f.host.stored())[0].wire === original
          };
        } finally {
          hook.restore();
        }
      }, loss);
      expect(result).toMatchObject({
        genuine: true,
        selected: false,
        callbacks: 1,
        same: true,
        counts: { readonly: 0, deletes: 0 }
      });
      expect(result.descriptors).toBeLessThanOrEqual(result.limit);
      expect(result.after).toEqual(result.before);
    });

  test(`${recovered ? 'recovered' : 'original'} actual Lineage append started by a descriptor callback cannot authorize stale cleanup`, async ({
    page
  }) => {
    await qualify(page, recovered);
    const result = await page.evaluate(async (recovered) => {
      const f = window.pcFixture,
        a = f.actors(),
        extra = await (recovered
          ? f.publish('wss://peer.example.org')
          : f.originalPublish('wss://peer.example.org'));
      if (!extra || extra.status !== 'accepted')
        throw Error('actual extra source result');
      const values = f.observations(),
        before = f.host.counts();
      let pending: ReturnType<typeof f.append> | undefined,
        callbacks = 0;
      const input = new Proxy(values, {
        getOwnPropertyDescriptor(target, key) {
          if (key === '0' && !pending) {
            callbacks++;
            pending = f.append(extra);
          }
          return Reflect.getOwnPropertyDescriptor(target, key);
        }
      });
      const genuine = !!window.pc.inboxProbeNonceAcknowledgementBinding(
          f.repository,
          a.session,
          f.nonce()!,
          f.host.policy
        ),
        hook = f.nativeFault('abort');
      try {
        const selected = await f.select(
          'reviewed_inbox_probe_cleanup_selection',
          input
        );
        if (!pending)
          throw Error('actual bounded descriptor callback did not run');
        const applied = await pending,
          removed = await f.remove();
        hook.restore();
        const rows = await f.host.stored();
        return {
          genuine,
          callbacks,
          captured: !!selected,
          applied: applied.status,
          status: removed.status,
          ack: !!removed.acknowledgement,
          counts: hook.counts(),
          before,
          after: f.host.counts(),
          rows: rows.length,
          receipts: (JSON.parse(rows[0].wire) as { receipts: unknown[] })
            .receipts.length
        };
      } finally {
        hook.restore();
      }
    }, recovered);
    expect(result).toMatchObject({
      genuine: true,
      callbacks: 1,
      applied: 'saved',
      ack: false,
      rows: 1,
      receipts: 6,
      counts: { deletes: 0 }
    });
    expect(['invalid', 'stopped']).toContain(result.status);
    expect(result.after).toEqual(result.before);
  });
}
