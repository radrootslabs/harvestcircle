import { test, expect, type Page } from '@playwright/test';
import { build } from 'vite';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type WebSocket } from 'ws';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Fixture from './harness/inbox-probe-nonce-facts.ts';
declare global {
  interface Window {
    nf: typeof Fixture;
    nfFixture: Awaited<ReturnType<typeof Fixture.makeNonceFactsFixture>>;
    nfPending?: Promise<unknown>;
    nfRestore?: () => void;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>,
  sockets: WebSocketServer,
  endpoint: string,
  bundle: string,
  head = '';
const origins = [
    'wss://inbox.example.org',
    'wss://peer.example.org',
    'wss://archive.example.org'
  ],
  physical = new Set<WebSocket>(),
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
          const event = archived.get(JSON.stringify([origin, filter.ids?.[0]]));
          if (event) socket.send(JSON.stringify(['EVENT', frame[1], event]));
        }
        socket.send(JSON.stringify(['EOSE', frame[1]]));
      } else if (frame[0] === 'EVENT') {
        const event = frame[1] as { id: string };
        archived.set(JSON.stringify([origin, event.id]), event);
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
          new URL('./harness/inbox-probe-nonce-facts.ts', import.meta.url)
        ),
        name: 'nf',
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
      };
    },
    { endpoint, origins }
  );
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  head = await page.evaluate(async (origins) => {
    window.nfFixture = await window.nf.makeNonceFactsFixture();
    return window.nfFixture.host.preferenceWire(origins);
  }, origins);
  await page.evaluate(async () => {
    const f = window.nfFixture;
    await f.open();
    if ((await f.prepareOriginal()).status !== 'prepared')
      throw Error('actual original S3/R1');
  });
});
test.afterEach(async ({ page }) => {
  if (!page.isClosed())
    await page.evaluate(async () => {
      window.nfRestore?.();
      window.nfFixture.host.mode('normal');
      window.nfFixture.host.settle();
      window.nfFixture.close();
      await window.nfPending;
    });
});
async function setup(page: Page, recovered = true, apply = true) {
  expect(
    await page.evaluate(
      async ({ recovered, apply }) =>
        (await window.nfFixture.prepareRead(recovered, apply)).status,
      { recovered, apply }
    )
  ).toBe('readback');
}
async function verify(page: Page) {
  expect(
    await page.evaluate(async () => (await window.nfFixture.nonce()).status)
  ).toBe('nonce_verified');
}
async function wire(page: Page) {
  return page.evaluate(async () => (await window.nfFixture.storedRow()).wire);
}
const traffic = () =>
  frames.filter(
    (x) =>
      x.origin !== 'wss://discovery.example.org' &&
      ['EVENT', 'REQ', 'AUTH'].includes(String(x.frame[0]))
  );

for (const recovered of [false, true]) {
  test(`${recovered ? 'recovered' : 'current-original'} genuine nonce fact has exact native lineage, idempotence and separate opaque acknowledgement`, async ({
    page
  }) => {
    await setup(page, recovered);
    const predecessor = await wire(page),
      net = traffic().length;
    const result = await page.evaluate(async (recovered) => {
      const f = window.nfFixture,
        before = f.host.counts(),
        first = f.captureNonce()!,
        second = f.captureNonce(),
        read = await f.nonce(first),
        afterRead = f.host.counts(),
        again = await f.nonce(first),
        firstSave = await f.appendNonce(),
        afterSave = (await f.storedRow()).wire,
        secondSave = await f.appendNonce(),
        binding = f.nonceBinding(),
        proof = f.nonceProof()!,
        ack = f.nonceAcknowledgement()!,
        a = f.actors();
      return {
        sameReader: first === second,
        read: read.status,
        again: again.status,
        before,
        afterRead,
        afterSaveCounts: f.host.counts(),
        firstSave: firstSave.status,
        secondSave: secondSave.status,
        sameAck: firstSave.acknowledgement === secondSave.acknowledgement,
        provenance: firstSave.provenance,
        afterSave,
        final: (await f.storedRow()).wire,
        proofKeys: Object.keys(proof),
        ackKeys: Object.keys(ack),
        current: binding?.current(),
        copy: !!window.nf.inboxProbeNonceAcknowledgementBinding(
          f.repository,
          a.session,
          { ...ack },
          f.host.policy
        ),
        crossReader: recovered
          ? (
              await window.nf.readInboxProbeNonce(
                first as unknown as Parameters<
                  typeof window.nf.readInboxProbeNonce
                >[0]
              )
            ).status
          : (
              await window.nf.readRecoveredInboxProbeNonce(
                first as unknown as Parameters<
                  typeof window.nf.readRecoveredInboxProbeNonce
                >[0]
              )
            ).status,
        crossProof: recovered
          ? !!window.nf.inboxProbeNonceProofBinding(
              proof as unknown as Parameters<
                typeof window.nf.inboxProbeNonceProofBinding
              >[0],
              f.repository,
              a.session,
              f.host.policy
            )
          : !!window.nf.recoveredInboxProbeNonceProofBinding(
              proof as unknown as Parameters<
                typeof window.nf.recoveredInboxProbeNonceProofBinding
              >[0],
              f.repository,
              a.session,
              f.host.policy
            ),
        crossCapture: recovered
          ? !!window.nf.captureInboxProbeNonceReader(
              f.repository,
              a.identity,
              a.session,
              f.host.policy,
              f.observation()!,
              'reviewed_inbox_probe_nonce_decryption'
            )
          : !!window.nf.captureRecoveredInboxProbeNonceReader(
              f.repository,
              a.identity,
              a.session,
              f.host.policy,
              f.observation()!,
              'reviewed_inbox_probe_nonce_decryption'
            ),
        settlement: window.nf.privateRecordSettlement(binding!.record),
        ordinaryCleanup: !!(await window.nf.capturePrivateOutboxCleanup(
          f.repository,
          a.session,
          [f.command()],
          'reviewed_settled_outbox_selection'
        )),
        readiness: window.nf.inboxAccessControllerSnapshot(a.access)?.readiness
      };
    }, recovered);
    const before = JSON.parse(predecessor) as {
        revision: number;
        receipts: Record<string, unknown>[];
        self: unknown;
      },
      after = JSON.parse(result.final) as typeof before;
    expect(result.sameReader && result.sameAck && result.current).toBe(true);
    expect(result.read).toBe('nonce_verified');
    expect(result.again).toBe('nonce_verified');
    expect(result.firstSave).toBe('saved');
    expect(result.secondSave).toBe('existing');
    expect(result.provenance).toBe('current');
    expect(result.afterRead).toEqual({
      ...result.before,
      keys: result.before.keys + 3,
      decrypts: result.before.decrypts + 2
    });
    expect(result.afterSaveCounts).toEqual(result.afterRead);
    expect(result.afterSave).toBe(result.final);
    expect(after).toEqual({
      ...before,
      revision: before.revision + 1,
      receipts: [
        ...before.receipts,
        {
          ...before.receipts.at(-1),
          status: 'nonce_verified',
          observedAtMilliseconds: expect.any(Number)
        }
      ]
    });
    expect(result.proofKeys).toEqual([]);
    expect(result.ackKeys).toEqual([]);
    expect(
      result.copy ||
        result.crossProof ||
        result.crossCapture ||
        result.ordinaryCleanup
    ).toBe(false);
    expect(result.crossReader).toBe('invalid');
    expect(result.settlement).toBe('protected');
    expect(result.readiness).toBe('unavailable');
    expect(traffic()).toHaveLength(net);
  });

  test(`${recovered ? 'recovered' : 'original'} genuinely decrypted but un-applied B2 cannot append until that exact source observation commits`, async ({
    page
  }) => {
    await setup(page, recovered, false);
    await verify(page);
    const predecessor = await wire(page);
    const first = await page.evaluate(async () => {
      const f = window.nfFixture,
        before = f.host.counts(),
        result = await f.appendNonce();
      return {
        status: result.status,
        ack: !!result.acknowledgement,
        before,
        after: f.host.counts()
      };
    });
    expect(first.status).toBe('invalid');
    expect(first.ack).toBe(false);
    expect(first.after).toEqual(first.before);
    expect(await wire(page)).toBe(predecessor);
    expect(
      await page.evaluate(
        async () => (await window.nfFixture.applyRead())?.status
      )
    ).toBe('saved');
    expect(
      await page.evaluate(
        async () => (await window.nfFixture.appendNonce()).status
      )
    ).toBe('saved');
  });
}

for (const review of [undefined, null, false, 'reviewed_private_decryption'])
  test(`recovered nonce capture refuses wrong explicit consent ${String(review)}`, async ({
    page
  }) => {
    await setup(page);
    const result = await page.evaluate((review) => {
      const f = window.nfFixture,
        a = f.actors(),
        before = f.host.counts();
      return {
        accepted: !!window.nf.captureRecoveredInboxProbeNonceReader(
          f.repository,
          a.identity,
          a.session,
          f.host.policy,
          f.observation()!,
          review
        ),
        before,
        after: f.host.counts()
      };
    }, review);
    expect(result.accepted).toBe(false);
    expect(result.after).toEqual(result.before);
  });

for (const recovered of [false, true])
  for (const fault of ['repository', 'session', 'policy'] as const)
    test(`${recovered ? 'recovered' : 'original'} genuine nonce proof and acknowledgement reject foreign ${fault} at the actual issuer`, async ({
      page
    }) => {
      await setup(page, recovered);
      await verify(page);
      const predecessor = await wire(page),
        net = traffic().length;
      const result = await page.evaluate(async (fault) => {
        const f = window.nfFixture,
          a = f.actors(),
          identity = window.nf.createIdentitySession();
        await window.nf.connectIdentity(identity);
        await window.nf.probeIdentityMessaging(identity, 'reviewed_self_copy');
        const foreignSession = await window.nf.createPrivateSession(
          identity,
          'reviewed_private_session'
        );
        if (!foreignSession) throw Error('actual foreign same-owner session');
        const repository = fault === 'repository' ? a.repository : f.repository,
          session = fault === 'session' ? foreignSession : a.session,
          policy = fault === 'policy' ? f.host.copiedPolicy() : f.host.policy,
          before = f.host.counts(),
          writes = f.nativeWriteHook(() => {});
        window.nfRestore = writes.restore;
        const refused = await window.nf.commitInboxProbeNonceVerification(
            repository,
            session,
            policy,
            f.nonceProof()!
          ),
          refusedWire = (await f.storedRow()).wire,
          refusedWrites = writes.hits(),
          saved = await f.appendNonce(),
          acknowledgement = f.nonceAcknowledgement();
        if (!acknowledgement)
          throw Error('actual legitimate nonce acknowledgement');
        const wrong = window.nf.inboxProbeNonceAcknowledgementBinding(
            repository,
            session,
            acknowledgement,
            policy
          ),
          copied = window.nf.inboxProbeNonceAcknowledgementBinding(
            f.repository,
            a.session,
            { ...acknowledgement },
            f.host.policy
          ),
          current = f.nonceBinding()?.current(),
          after = f.host.counts(),
          totalWrites = writes.hits();
        writes.restore();
        window.nf.closePrivateSession(foreignSession);
        window.nf.disconnectIdentity(identity);
        return {
          refused: refused.status,
          refusedAck: !!refused.acknowledgement,
          refusedWire,
          refusedWrites,
          saved: saved.status,
          wrong: !!wrong,
          copied: !!copied,
          current,
          before,
          after,
          totalWrites
        };
      }, fault);
      expect(result.refused).toBe('invalid');
      expect(result.refusedAck || result.wrong || result.copied).toBe(false);
      expect(result.refusedWire).toBe(predecessor);
      expect(result.refusedWrites).toBe(0);
      expect(result.saved).toBe('saved');
      expect(result.totalWrites).toBe(1);
      expect(result.current).toBe(true);
      expect(result.after).toEqual(result.before);
      expect(traffic()).toHaveLength(net);
    });

test('copied proof, C2A recovery proof, recovered ACK and nonce status labels have no nonce-fact authority', async ({
  page
}) => {
  await setup(page);
  await verify(page);
  const predecessor = await wire(page),
    net = traffic().length;
  const result = await page.evaluate(async () => {
    const f = window.nfFixture,
      before = f.host.counts(),
      answers: boolean[] = [];
    for (const fake of [
      { ...f.nonceProof()! },
      f.proof(),
      f.acknowledgement(),
      { status: 'nonce_verified', proof: f.nonceProof(), current: () => true }
    ]) {
      const result = await window.nf.commitInboxProbeNonceVerification(
        f.repository,
        f.actors().session,
        f.host.policy,
        fake as Parameters<
          typeof window.nf.commitInboxProbeNonceVerification
        >[3]
      );
      answers.push(
        !!result.acknowledgement ||
          ['saved', 'reconciled', 'existing'].includes(result.status)
      );
    }
    return { answers, before, after: f.host.counts() };
  });
  expect(result.answers).toEqual([false, false, false, false]);
  expect(result.after).toEqual(result.before);
  expect(await wire(page)).toBe(predecessor);
  expect(traffic()).toHaveLength(net);
});

for (const fault of ['repository', 'policy', 'identity', 'session'] as const)
  test(`genuine recovered observation rejects same-owner foreign ${fault} association`, async ({
    page
  }) => {
    await setup(page);
    const result = await page.evaluate(async (fault) => {
      const f = window.nfFixture,
        a = f.actors(),
        identity = window.nf.createIdentitySession();
      await window.nf.connectIdentity(identity);
      await window.nf.probeIdentityMessaging(identity, 'reviewed_self_copy');
      const session = await window.nf.createPrivateSession(
        identity,
        'reviewed_private_session'
      );
      if (!session) throw Error('genuine foreign same-owner private session');
      const before = f.host.counts();
      const reader = window.nf.captureRecoveredInboxProbeNonceReader(
        fault === 'repository' ? a.repository : f.repository,
        fault === 'identity' ? identity : a.identity,
        fault === 'session' ? session : a.session,
        fault === 'policy' ? f.host.copiedPolicy() : f.host.policy,
        f.observation()!,
        'reviewed_inbox_probe_nonce_decryption'
      );
      const after = f.host.counts();
      window.nf.closePrivateSession(session);
      window.nf.disconnectIdentity(identity);
      return { accepted: !!reader, before, after };
    }, fault);
    expect(result.accepted).toBe(false);
    expect(result.after).toEqual(result.before);
  });

for (const mode of ['wrong_key', 'refuse_decrypt', 'wrong_decrypt'] as const)
  test(`recovered nonce provider ${mode} cannot issue proof or fact`, async ({
    page
  }) => {
    await setup(page);
    const predecessor = await wire(page);
    const result = await page.evaluate(async (mode) => {
      const f = window.nfFixture,
        reader = f.captureNonce()!;
      f.host.mode(mode);
      const result = await f.nonce(reader);
      return { status: result.status, proof: !!f.nonceProof() };
    }, mode);
    expect(result.status).not.toBe('nonce_verified');
    expect(result.proof).toBe(false);
    expect(await wire(page)).toBe(predecessor);
  });

for (const fault of [
  'seal_tags',
  'seal_kind',
  'seal_signature',
  'rumor_nonce',
  'rumor_hash',
  'rumor_time',
  'rumor_owner',
  'rumor_serialization'
] as const)
  test(`actual recovered decrypted ${fault} refuses nonce proof`, async ({
    page
  }) => {
    await setup(page);
    const predecessor = await wire(page);
    const result = await page.evaluate(async (fault) => {
      const f = window.nfFixture,
        restore = f.host.decryptCallback(
          fault.startsWith('seal_') ? 1 : 2,
          (wire) => {
            if (fault === 'seal_tags')
              return f.host.signSeal(wire, { tags: [['p', f.host.owner]] });
            if (fault === 'seal_kind')
              return f.host.signSeal(wire, { kind: 14 });
            if (fault === 'rumor_serialization') return wire + '\n';
            const data = JSON.parse(wire) as {
              sig?: string;
              content?: string;
              id?: string;
              created_at?: number;
              pubkey?: string;
            };
            if (fault === 'seal_signature') data.sig = '0'.repeat(128);
            if (fault === 'rumor_nonce')
              data.content = 'different diagnostic nonce';
            if (fault === 'rumor_hash') data.id = 'f'.repeat(64);
            if (fault === 'rumor_time')
              data.created_at = (data.created_at ?? 0) + 1;
            if (fault === 'rumor_owner') data.pubkey = 'f'.repeat(64);
            return JSON.stringify(data);
          }
        );
      window.nfRestore = () => restore.restore();
      return {
        status: (await f.nonce()).status,
        proof: !!f.nonceProof(),
        receivers: restore.receivers()
      };
    }, fault);
    expect(result.status).not.toBe('nonce_verified');
    expect(result.proof).toBe(false);
    expect(result.receivers).toBeGreaterThan(0);
    expect(await wire(page)).toBe(predecessor);
  });

for (const mutation of [
  'alias',
  'own_tail',
  'foreign_tail',
  'missing'
] as const)
  test(`nonce initial complete physical namespace handles ${mutation}`, async ({
    page
  }) => {
    await setup(page);
    const result = await page.evaluate(async (mutation) => {
      const f = window.nfFixture,
        reader = f.captureNonce()!,
        row = await f.storedRow();
      if (mutation === 'alias')
        await f.host.inject([{ ...row, wire: row.wire + '\n' }]);
      else if (mutation === 'own_tail' || mutation === 'foreign_tail')
        await f.host.inject([
          {
            owner: mutation === 'own_tail' ? row.owner : 'f'.repeat(64),
            id: crypto.randomUUID(),
            wire: 'unknown owner tail'
          }
        ]);
      else {
        const opened = await window.nf.openBrowserDatabase();
        if (opened.state !== 'ready')
          throw Error('actual fixture native deletion');
        const tx = window.nf.browserDatabaseTransaction(
          opened.owner,
          ['private_sends'],
          'readwrite'
        );
        tx.objectStore('private_sends').delete([row.owner, row.id]);
        await new Promise<void>((resolve, reject) => {
          tx.addEventListener('complete', () => resolve());
          tx.addEventListener('abort', () =>
            reject(Error('actual fixture delete abort'))
          );
        });
        window.nf.closeBrowserDatabase(opened.owner);
      }
      const before = f.host.counts(),
        raw = await f.host.stored(),
        result = await f.nonce(reader);
      return {
        status: result.status,
        before,
        after: f.host.counts(),
        raw,
        final: await f.host.stored()
      };
    }, mutation);
    expect(result.status === 'nonce_verified').toBe(
      mutation === 'foreign_tail'
    );
    if (mutation !== 'foreign_tail')
      expect(result.after).toEqual(result.before);
    expect(result.final).toEqual(result.raw);
  });

for (const hit of [1, 2, 3])
  for (const fault of ['head', 'session', 'native'] as const)
    test(`recovered nonce actual key${hit} ${fault} churn fences proof mint`, async ({
      page
    }) => {
      await setup(page);
      const result = await page.evaluate(
        async ({ hit, fault }) => {
          const f = window.nfFixture,
            row = await f.storedRow(),
            reader = f.captureNonce()!;
          window.nfRestore = f.host.keyCallback(hit, async () => {
            if (fault === 'native')
              await f.host.inject([{ ...row, wire: row.wire + '\n' }]);
            else f.lose(fault);
          });
          return {
            status: (await f.nonce(reader)).status,
            proof: !!f.nonceProof()
          };
        },
        { hit, fault }
      );
      expect(result.status).not.toBe('nonce_verified');
      expect(result.proof).toBe(false);
    });

for (const hit of [1, 2])
  for (const fault of ['before_abort', 'after_abort'] as const)
    test(`recovered nonce readonly${hit} ${fault} requires actual native completion`, async ({
      page
    }) => {
      await setup(page);
      const predecessor = await wire(page);
      const result = await page.evaluate(
        async ({ hit, fault }) => {
          const f = window.nfFixture,
            reader = f.captureNonce()!,
            hook = f.nativeReadonlyHook(hit, fault);
          window.nfRestore = hook.restore;
          return {
            status: (await f.nonce(reader)).status,
            proof: !!f.nonceProof(),
            faults: hook.faults()
          };
        },
        { hit, fault }
      );
      expect(result.faults).toBe(1);
      expect(result.status).not.toBe('nonce_verified');
      expect(result.proof).toBe(false);
      expect(await wire(page)).toBe(predecessor);
    });

for (const kind of [
  'reader',
  'expiry',
  'controller',
  'head',
  'session'
] as const) {
  test(`stopped ${kind} proof BEFORE append admission has zero native write`, async ({
    page
  }) => {
    await setup(page);
    await verify(page);
    const predecessor = await wire(page);
    const result = await page.evaluate(async (kind) => {
      const f = window.nfFixture,
        before = f.host.counts();
      f.lose(kind);
      const saved = await f.appendNonce();
      return {
        status: saved.status,
        ack: !!saved.acknowledgement,
        before,
        after: f.host.counts()
      };
    }, kind);
    expect(['saved', 'existing', 'reconciled']).not.toContain(result.status);
    expect(result.ack).toBe(false);
    expect(result.after).toEqual(result.before);
    expect(await wire(page)).toBe(predecessor);
  });
  test(`actual ${kind} loss AFTER native append admission can retain history but no current acknowledgement`, async ({
    page
  }) => {
    await setup(page);
    await verify(page);
    const result = await page.evaluate(async (kind) => {
      const f = window.nfFixture,
        hook = f.nativeWriteHook(() => f.lose(kind));
      window.nfRestore = hook.restore;
      const saved = await f.appendNonce(),
        row = JSON.parse((await f.storedRow()).wire) as {
          receipts: { status: string }[];
        };
      return {
        status: saved.status,
        ack: !!saved.acknowledgement,
        binding: !!f.nonceBinding(),
        provenance: saved.provenance,
        last: row.receipts.at(-1)?.status,
        hits: hook.hits()
      };
    }, kind);
    expect(result.hits).toBe(1);
    expect(result.last).toBe('nonce_verified');
    expect(result.ack || result.binding).toBe(false);
    expect(result.provenance).not.toBe('current');
  });
  test(`actual ${kind} loss DURING initial native read precedes append admission and has zero put`, async ({
    page
  }) => {
    await setup(page);
    await verify(page);
    const predecessor = await wire(page);
    const result = await page.evaluate(async (kind) => {
      const f = window.nfFixture,
        genuineBefore = !!window.nf.inboxProbeNonceVerificationEvidence(
          f.nonceProof()!,
          f.repository,
          f.actors().session,
          f.host.policy
        ),
        before = f.host.counts(),
        writes = f.nativeWriteHook(() => {}),
        read = f.nativeReadonlyHook(1, 'callback', () => f.lose(kind));
      window.nfRestore = () => {
        read.restore();
        writes.restore();
      };
      const result = await f.appendNonce();
      read.restore();
      const after = f.host.counts(),
        puts = writes.hits();
      writes.restore();
      return {
        genuineBefore,
        faults: read.faults(),
        puts,
        status: result.status,
        ack: !!result.acknowledgement,
        binding: !!f.nonceBinding(),
        before,
        after
      };
    }, kind);
    expect(result.genuineBefore).toBe(true);
    expect(result.faults).toBe(1);
    expect(result.puts).toBe(0);
    expect(result.ack || result.binding).toBe(false);
    expect(['saved', 'reconciled', 'existing']).not.toContain(result.status);
    expect(result.after).toEqual(result.before);
    expect(await wire(page)).toBe(predecessor);
  });
}

for (const timing of ['after_confirmed_abort', 'between_read_and_cas'] as const)
  test(`exact handwritten nonce successor ${timing} cannot acquire genuine operation provenance`, async ({
    page
  }) => {
    await setup(page);
    await verify(page);
    const predecessor = await wire(page),
      net = traffic().length;
    const result = await page.evaluate(async (timing) => {
      const f = window.nfFixture,
        candidate = await f.handwrittenNonceSuccessor(),
        before = f.host.counts();
      let aborted = 0,
        firstWire: string | undefined,
        readHook: ReturnType<typeof f.nativeReadonlyHook> | undefined;
      if (timing === 'after_confirmed_abort') {
        const hook = f.installCommitFault('aborted');
        window.nfRestore = hook.restore;
        const first = await f.appendNonce();
        hook.restore();
        if (first.acknowledgement)
          throw Error('aborted append cannot acknowledge');
        firstWire = (await f.storedRow()).wire;
        // The port can report failure before native abort dispatch. Observe
        // the following real readonly completion before sampling its event.
        aborted = hook.counts().aborted;
        await f.host.inject([candidate]);
      } else {
        readHook = f.nativeReadonlyHook(1, 'callback', () => {
          window.nfPending = f.host.inject([candidate]);
        });
        window.nfRestore = readHook.restore;
      }
      const genuineBefore = !!window.nf.inboxProbeNonceVerificationEvidence(
          f.nonceProof()!,
          f.repository,
          f.actors().session,
          f.host.policy
        ),
        saved = await f.appendNonce();
      await window.nfPending;
      window.nfRestore?.();
      return {
        genuineBefore,
        aborted,
        firstWire,
        faults: readHook?.faults(),
        status: saved.status,
        ack: !!saved.acknowledgement,
        binding: !!f.nonceBinding(),
        candidate: candidate.wire,
        actual: (await f.storedRow()).wire,
        before,
        after: f.host.counts()
      };
    }, timing);
    expect(result.genuineBefore).toBe(true);
    if (timing === 'after_confirmed_abort') {
      expect(result.aborted).toBe(1);
      expect(result.firstWire).toBe(predecessor);
    } else expect(result.faults).toBe(1);
    expect(result.status).toBe('conflict');
    expect(result.ack || result.binding).toBe(false);
    expect(result.actual).toBe(result.candidate);
    expect(result.actual).not.toBe(predecessor);
    expect(result.after).toEqual(result.before);
    expect(traffic()).toHaveLength(net);
  });

for (const fault of ['aborted', 'uncertain', 'changed_successor'] as const)
  test(`nonce append actual native ${fault} preserves exact outcome and genuine reconciliation`, async ({
    page
  }) => {
    await setup(page);
    await verify(page);
    const predecessor = await wire(page);
    const result = await page.evaluate(async (fault) => {
      const f = window.nfFixture,
        hook = f.installCommitFault(fault);
      window.nfRestore = hook.restore;
      const result = await f.appendNonce();
      hook.restore();
      const first = (await f.storedRow()).wire,
        again = await f.appendNonce();
      return {
        status: result.status,
        ack: !!result.acknowledgement,
        provenance: result.provenance,
        first,
        counts: hook.counts(),
        again: again.status,
        againAck: !!again.acknowledgement
      };
    }, fault);
    expect(result.counts.putSuccess).toBe(1);
    if (fault === 'aborted') {
      expect(result.ack).toBe(false);
      expect(result.first).toBe(predecessor);
      expect(result.counts.aborted).toBe(1);
      expect(result.again).toBe('saved');
      expect(result.againAck).toBe(true);
    } else if (fault === 'uncertain') {
      expect(result.status).toBe('reconciled');
      expect(result.ack).toBe(true);
      expect(result.counts.complete).toBe(1);
      expect(result.again).toBe('existing');
      expect(result.againAck).toBe(true);
    } else {
      expect(result.status).toBe('conflict');
      expect(result.ack || result.againAck).toBe(false);
    }
  });

for (const hit of [1, 2])
  for (const fault of ['before_abort', 'after_abort'] as const)
    test(`nonce fact readonly${hit} ${fault} cannot fabricate completion and later exact operation may reconcile`, async ({
      page
    }) => {
      await setup(page);
      await verify(page);
      const predecessor = await wire(page);
      const result = await page.evaluate(
        async ({ hit, fault }) => {
          const f = window.nfFixture,
            hook = f.nativeReadonlyHook(hit, fault);
          window.nfRestore = hook.restore;
          const result = await f.appendNonce();
          hook.restore();
          const first = (await f.storedRow()).wire,
            next = await f.appendNonce();
          return {
            status: result.status,
            ack: !!result.acknowledgement,
            first,
            faults: hook.faults(),
            next: next.status,
            nextAck: !!next.acknowledgement
          };
        },
        { hit, fault }
      );
      expect(result.faults).toBe(1);
      expect(result.ack).toBe(false);
      if (hit === 1) {
        expect(result.first).toBe(predecessor);
        expect(result.next).toBe('saved');
      } else {
        expect(result.first).not.toBe(predecessor);
        expect(result.next).toBe('reconciled');
      }
      expect(result.nextAck).toBe(true);
    });

for (const recovered of [false, true])
  for (const contention of ['owner_lock', 'shared_sdk_job'] as const)
    test(`${recovered ? 'recovered' : 'original'} genuine nonce append respects actual occupied ${contention} and preserves explicit same-proof retry`, async ({
      page
    }) => {
      await setup(page, recovered);
      await verify(page);
      const predecessor = await wire(page),
        net = traffic().length;
      const result = await page.evaluate(async (contention) => {
        const f = window.nfFixture,
          a = f.actors(),
          proof = f.nonceProof()!;
        let entered: (() => void) | undefined,
          release: (() => void) | undefined;
        const ready = new Promise<void>((resolve) => {
          entered = resolve;
        });
        async function hold() {
          entered?.();
          await new Promise<void>((resolve) => {
            release = resolve;
          });
        }
        const held =
          contention === 'owner_lock'
            ? navigator.locks.request(
                'harvestcircle:owner:' + f.host.owner,
                { mode: 'exclusive' },
                hold
              )
            : window.nf.withIdlePrivateLocalStorage(a.session, hold);
        window.nfPending = held;
        await ready;
        const genuineBefore = !!window.nf.inboxProbeNonceVerificationEvidence(
            proof,
            f.repository,
            a.session,
            f.host.policy
          ),
          before = f.host.counts(),
          reads = f.nativeReadonlyHook(1, 'callback'),
          writes = f.nativeWriteHook(() => {});
        window.nfRestore = () => {
          reads.restore();
          writes.restore();
          release?.();
        };
        let refused;
        try {
          const saved = await f.appendNonce();
          refused = {
            status: saved.status,
            ack: !!saved.acknowledgement,
            binding: !!f.nonceBinding(),
            reads: reads.faults(),
            puts: writes.hits(),
            after: f.host.counts()
          };
        } finally {
          reads.restore();
          writes.restore();
          release?.();
          await held;
        }
        const refusedWire = (await f.storedRow()).wire,
          sameProof =
            f.nonceProof() === proof &&
            !!window.nf.inboxProbeNonceVerificationEvidence(
              proof,
              f.repository,
              a.session,
              f.host.policy
            ),
          writesAfterRelease = f.nativeWriteHook(() => {});
        window.nfRestore = writesAfterRelease.restore;
        const first = await f.appendNonce(),
          again = await f.appendNonce(),
          putsAfterRelease = writesAfterRelease.hits();
        writesAfterRelease.restore();
        return {
          genuineBefore,
          before,
          refused,
          refusedWire,
          sameProof,
          first: first.status,
          again: again.status,
          sameAck: first.acknowledgement === again.acknowledgement,
          current: f.nonceBinding()?.current(),
          putsAfterRelease,
          after: f.host.counts()
        };
      }, contention);
      expect(result.genuineBefore && result.sameProof).toBe(true);
      expect(result.refused).toEqual({
        status: 'busy',
        ack: false,
        binding: false,
        reads: 0,
        puts: 0,
        after: result.before
      });
      expect(result.refusedWire).toBe(predecessor);
      expect(result.first).toBe('saved');
      expect(result.again).toBe('existing');
      expect(result.sameAck && result.current).toBe(true);
      expect(result.putsAfterRelease).toBe(1);
      expect(result.after).toEqual(result.before);
      expect(traffic()).toHaveLength(net);
    });

test('genuine proof and native admission serialize parallel nonce append without extra SDK work', async ({
  page
}) => {
  await setup(page);
  await verify(page);
  const result = await page.evaluate(async () => {
    const f = window.nfFixture,
      before = f.host.counts(),
      pair = await Promise.all([f.appendNonce(), f.appendNonce()]),
      row = JSON.parse((await f.storedRow()).wire) as {
        receipts: { status: string }[];
      };
    return {
      states: pair.map((x) => x.status).sort(),
      facts: row.receipts.filter((x) => x.status === 'nonce_verified').length,
      before,
      after: f.host.counts()
    };
  });
  expect(result.states).toEqual(['busy', 'saved']);
  expect(result.facts).toBe(1);
  expect(result.after).toEqual(result.before);
});

for (const loss of ['reader', 'head', 'session'] as const)
  test(`actual pending decrypt ${loss} loss keeps original slot until settlement and never revives nonce proof`, async ({
    page
  }) => {
    await setup(page);
    await page.evaluate(() => {
      const f = window.nfFixture;
      f.host.mode('hold_decrypt');
      window.nfPending = f.nonce();
    });
    await expect
      .poll(() => page.evaluate(() => window.nfFixture.host.waiting()))
      .toBe('decrypt');
    const result = await page.evaluate(async (loss) => {
      const f = window.nfFixture;
      f.lose(loss);
      const before = f.host.counts(),
        busy = await window.nf.withIdlePrivateLocalStorage(
          f.actors().session,
          () => Promise.resolve('unreachable')
        );
      const after = f.host.counts();
      f.host.mode('normal');
      f.host.settle();
      await window.nfPending;
      return {
        busy: busy.ok,
        before,
        after,
        proof: !!f.nonceProof(),
        binding: !!f.nonceBinding()
      };
    }, loss);
    expect(result.busy || result.proof || result.binding).toBe(false);
    expect(result.after).toEqual(result.before);
  });

for (const kind of ['head', 'native', 'reentry'] as const)
  test(`actual final nonce clock ${kind} callback is fenced before physical read and proof mint`, async ({
    page
  }) => {
    await setup(page);
    const result = await page.evaluate(async (kind) => {
      const f = window.nfFixture,
        row = await f.storedRow(),
        reader = f.captureNonce()!;
      let same = false;
      const hook = f.clockHook(() => {
        if (kind === 'head') f.lose('head');
        else if (kind === 'native')
          window.nfPending = f.host.inject([{ ...row, wire: row.wire + '\n' }]);
        else same = f.captureNonce() === reader;
      });
      window.nfRestore = hook.restore;
      const result = await f.nonce(reader);
      await window.nfPending;
      return {
        status: result.status,
        proof: !!f.nonceProof(),
        hits: hook.hits(),
        same
      };
    }, kind);
    expect(result.hits).toBeGreaterThan(0);
    expect(result.status === 'nonce_verified').toBe(kind === 'reentry');
    expect(result.proof).toBe(kind === 'reentry');
    if (kind === 'reentry') expect(result.same).toBe(true);
  });

test('nonce append at full100 native rows preserves all protected rows and no automatic cleanup', async ({
  page
}) => {
  await setup(page);
  await verify(page);
  const result = await page.evaluate(async () => {
    const f = window.nfFixture,
      row = await f.storedRow(),
      record = JSON.parse(row.wire) as { id: string; receipts: unknown[] },
      others = Array.from({ length: 99 }, () => {
        const id = crypto.randomUUID();
        return {
          ...row,
          id,
          wire: JSON.stringify({ ...record, id, receipts: [] })
        };
      });
    await f.host.inject(others);
    const saved = await f.appendNonce(),
      final = await f.host.stored();
    return {
      status: saved.status,
      ack: !!saved.acknowledgement,
      count: final.length,
      preserved: others.every((x) =>
        final.some((y) => y.id === x.id && y.wire === x.wire)
      )
    };
  });
  expect(result).toEqual({
    status: 'saved',
    ack: true,
    count: 100,
    preserved: true
  });
});

test('actual8MiB capacity refuses nonce growth while preserving current proof and every original wire', async ({
  page
}) => {
  await setup(page);
  await verify(page);
  const result = await page.evaluate(async () => {
    const f = window.nfFixture,
      row = await f.storedRow(),
      record = JSON.parse(row.wire) as {
        id: string;
        receipts: Record<string, unknown>[];
      },
      cap = 8 * 1024 * 1024,
      bytes = (x: unknown) =>
        new TextEncoder().encode(JSON.stringify(x)).length,
      id = crypto.randomUUID(),
      fact = { ...record.receipts[0], status: 'unknown' },
      make = (n: number) => ({
        ...row,
        id,
        wire: JSON.stringify({
          ...record,
          id,
          receipts: Array.from({ length: n }, () => fact)
        })
      }),
      zero = bytes(make(0)),
      one = bytes(make(1)),
      increment = bytes(make(2)) - one,
      nonceDelta =
        bytes({
          ...row,
          wire: JSON.stringify({
            ...record,
            receipts: [
              ...record.receipts,
              { ...record.receipts.at(-1), status: 'nonce_verified' }
            ]
          })
        }) - bytes(row),
      n = Math.floor((cap - bytes(row) - nonceDelta / 2 - one) / increment) + 1,
      filler = make(n),
      remaining = cap - bytes(row) - bytes(filler);
    if (!(remaining > 0 && remaining < nonceDelta && zero < one))
      throw Error('actual bounded native quota fixture');
    await f.host.inject([filler]);
    const saved = await f.appendNonce(),
      final = await f.host.stored();
    return {
      status: saved.status,
      ack: !!saved.acknowledgement,
      remaining,
      nonceDelta,
      preserved:
        final.some((x) => x.id === row.id && x.wire === row.wire) &&
        final.some((x) => x.id === filler.id && x.wire === filler.wire)
    };
  });
  expect(result.status).toBe('capacity');
  expect(result.ack).toBe(false);
  expect(result.preserved).toBe(true);
  expect(result.remaining).toBeLessThan(result.nonceDelta);
});

test('actual maximum revision cannot append a nonce fact after genuine recovered PUB and B2', async ({
  page
}) => {
  await page.evaluate(async () => {
    const f = window.nfFixture,
      row = await f.storedRow(),
      record = JSON.parse(row.wire) as { revision: number };
    record.revision = Number.MAX_SAFE_INTEGER - 2;
    await f.host.inject([{ ...row, wire: JSON.stringify(record) }]);
  });
  await setup(page);
  await verify(page);
  const predecessor = await wire(page);
  const result = await page.evaluate(async () => {
    const saved = await window.nfFixture.appendNonce();
    return { status: saved.status, ack: !!saved.acknowledgement };
  });
  expect(result).toEqual({ status: 'invalid_record', ack: false });
  expect(await wire(page)).toBe(predecessor);
});
