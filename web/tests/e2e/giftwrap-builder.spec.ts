import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Wraps from './harness/giftwrap-builder.ts';
declare global {
  interface Window {
    hcp076: typeof Wraps;
    hcp076Fixture: Awaited<ReturnType<typeof Wraps.makeFixture>>;
  }
}
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(
          new URL('./harness/giftwrap-builder.ts', import.meta.url)
        ),
        name: 'hcp076',
        formats: ['iife']
      }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw Error('missing bundle');
  const chunks = output.output.filter((x) => x.type === 'chunk');
  expect(chunks).toHaveLength(1);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      fixture: 'HCP076_ACTUAL_SDK_WRAPPER_CRYPTO',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      moduleIds: Object.keys(chunks[0].modules),
      qualification:
        'Controlled SDK provider and real IDB/WebLocks; not installed extensions/relay/clientQ'
    })
  );
});
test.afterAll(async () => server.close());
async function load(page: Page) {
  await page.goto(server.url + '/search');
  await page.addScriptTag({ content: bundle });
  await page.evaluate(async () => {
    window.hcp076Fixture = await window.hcp076.makeFixture();
  });
}
for (const role of ['peer', 'self'] as const)
  test(`actual ${role}1059 decrypts to exact sender seal and original same rumor`, async ({
    page
  }) => {
    await load(page);
    const result = await page.evaluate(async (role) => {
      const f = window.hcp076Fixture,
        s = window.hcp076;
      try {
        const op = f.operation(role);
        if (!op) throw Error('missing operation');
        const seal = await s.buildPrivateSeal(op);
        if (seal.status !== 'sealed') throw Error('missing seal');
        const inner = s.privateSealSnapshot(seal.seal)!;
        const before = f.counts(),
          token = s.buildPrivateGiftwrap(seal.seal, 'reviewed_private_wrap');
        if (!token) throw Error('missing wrap');
        const saved = s.privateGiftwrapSnapshot(token)!,
          outer = JSON.parse(saved.wire) as {
            kind: number;
            tags: string[][];
            pubkey: string;
            created_at: number;
          };
        const plain = f.decryptWrap(saved.wire, role),
          decrypted = f.decrypt(plain, role),
          stored = await f.stored();
        return {
          kind: outer.kind,
          tags: outer.tags,
          target: inner.destination,
          exactSeal: plain === inner.wire,
          sameRumor: decrypted.equal,
          disposable: outer.pubkey !== f.owner && outer.pubkey !== f.peer,
          seven: Object.keys(outer).length,
          changedCalls: JSON.stringify(before) !== JSON.stringify(f.counts()),
          noPlain:
            !saved.wire.includes('sentinel') && !stored.includes('sentinel'),
          bounded: new TextEncoder().encode(saved.wire).length <= 32768,
          time: outer.created_at,
          now: Math.floor(Date.now() / 1000),
          role: saved.role,
          owner: saved.owner,
          peer: saved.peer
        };
      } finally {
        f.close();
      }
    }, role);
    expect(result.kind).toBe(1059);
    expect(result.tags).toEqual([['p', result.target]]);
    expect(result.exactSeal).toBe(true);
    expect(result.sameRumor).toBe(true);
    expect(result.disposable).toBe(true);
    expect(result.seven).toBe(7);
    expect(result.changedCalls).toBe(false);
    expect(result.noPlain).toBe(true);
    expect(result.bounded).toBe(true);
    expect(result.time).toBeLessThanOrEqual(result.now);
    expect(result.time).toBeGreaterThanOrEqual(result.now - 3600);
    expect(result.role).toBe(role);
  });
test('distinct disposable wrappers preserve the exact original seal without retargeting or identity effects', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp076Fixture,
      s = window.hcp076;
    try {
      const seal = await s.buildPrivateSeal(f.operation()!);
      if (seal.status !== 'sealed') throw Error('missing seal');
      const before = f.counts(),
        a = s.privateGiftwrapSnapshot(
          s.buildPrivateGiftwrap(seal.seal, 'reviewed_private_wrap')!
        )!,
        b = s.privateGiftwrapSnapshot(
          s.buildPrivateGiftwrap(seal.seal, 'reviewed_private_wrap')!
        )!;
      const ae = JSON.parse(a.wire) as { id: string; pubkey: string },
        be = JSON.parse(b.wire) as { id: string; pubkey: string };
      return {
        fresh: ae.pubkey !== be.pubkey && ae.id !== be.id,
        equal: f.decryptWrap(a.wire, 'peer') === f.decryptWrap(b.wire, 'peer'),
        sameBinding:
          a.command === b.command &&
          a.rumorHash === b.rumorHash &&
          a.destination === b.destination,
        calls: JSON.stringify(before) === JSON.stringify(f.counts())
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    fresh: true,
    equal: true,
    sameBinding: true,
    calls: true
  });
});
test('wrong review, cast/copy wire and stopped genuine operation do not mint wrappers', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp076Fixture,
      s = window.hcp076;
    try {
      const op = f.operation()!,
        seal = await s.buildPrivateSeal(op);
      if (seal.status !== 'sealed') throw Error('missing seal');
      const snapshot = s.privateSealSnapshot(seal.seal)!;
      const wrong = s.buildPrivateGiftwrap(seal.seal, 'connect'),
        cast = s.buildPrivateGiftwrap(
          snapshot as unknown as typeof seal.seal,
          'reviewed_private_wrap'
        );
      s.stopPrivateSeal(op);
      const stopped = s.buildPrivateGiftwrap(
        seal.seal,
        'reviewed_private_wrap'
      );
      return {
        wrong: wrong === undefined,
        cast: cast === undefined,
        stopped: stopped === undefined
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({ wrong: true, cast: true, stopped: true });
});
test('disconnect removes wrapper lifetime access and cannot turn ciphertext metadata into a token', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp076Fixture,
      s = window.hcp076;
    try {
      const seal = await s.buildPrivateSeal(f.operation()!);
      if (seal.status !== 'sealed') throw Error('missing seal');
      const wrap = s.buildPrivateGiftwrap(seal.seal, 'reviewed_private_wrap')!,
        saved = s.privateGiftwrapSnapshot(wrap)!;
      const cast = s.privateGiftwrapSnapshot(saved as unknown as typeof wrap);
      f.disconnect();
      return {
        cast: cast === undefined,
        stale: s.privateGiftwrapSnapshot(wrap) === undefined,
        noResume:
          s.buildPrivateGiftwrap(seal.seal, 'reviewed_private_wrap') ===
          undefined
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({ cast: true, stale: true, noResume: true });
});

test('peer and self outer copies preserve one genuine reserved rumor and route only to that pair', async ({
  page
}) => {
  await load(page);
  const result = await page.evaluate(async () => {
    const f = window.hcp076Fixture,
      s = window.hcp076;
    try {
      const selfSeal = await s.buildPrivateSeal(f.operation('self')!),
        peerSeal = await s.buildPrivateSeal(f.operation('peer')!);
      if (selfSeal.status !== 'sealed' || peerSeal.status !== 'sealed')
        throw Error('missing pair seals');
      const self = s.privateGiftwrapSnapshot(
          s.buildPrivateGiftwrap(selfSeal.seal, 'reviewed_private_wrap')!
        )!,
        peer = s.privateGiftwrapSnapshot(
          s.buildPrivateGiftwrap(peerSeal.seal, 'reviewed_private_wrap')!
        )!;
      const selfInner = f.decryptWrap(self.wire, 'self'),
        peerInner = f.decryptWrap(peer.wire, 'peer');
      return {
        selfEqual: f.decrypt(selfInner, 'self').equal,
        peerEqual: f.decrypt(peerInner, 'peer').equal,
        same:
          self.rumorHash === peer.rumorHash && self.command === peer.command,
        targets: self.destination === f.owner && peer.destination === f.peer,
        roles: self.role === 'self' && peer.role === 'peer',
        distinct: self.wire !== peer.wire
      };
    } finally {
      f.close();
    }
  });
  expect(result).toEqual({
    selfEqual: true,
    peerEqual: true,
    same: true,
    targets: true,
    roles: true,
    distinct: true
  });
});
