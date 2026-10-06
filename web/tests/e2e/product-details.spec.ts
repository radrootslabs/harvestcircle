import { expect, test } from '@playwright/test';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  cp,
  mkdtemp,
  readFile,
  readdir,
  rm,
  writeFile
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { naddrEncode } from 'applesauce-core/helpers/pointers';
import { finalizeEvent } from 'applesauce-core/helpers';
import WebSocket, { WebSocketServer, type RawData } from 'ws';
function wireText(bytes: RawData): string {
  const buffer = Array.isArray(bytes)
    ? Buffer.concat(bytes)
    : Buffer.isBuffer(bytes)
      ? bytes
      : Buffer.from(bytes);
  return buffer.toString('utf8');
}
import { createStaticHarness } from '../integration/harness/static.ts';
const capsule = fileURLToPath(new URL('../../../', import.meta.url));
let directory: string;
let server: Awaited<ReturnType<typeof createStaticHarness>>;
// Verification fixture: compile exact primary source, deriving only its fixed
// deployment allowlist. Never alter the primary output or production policy.
test.beforeAll(async () => {
  test.setTimeout(120000);
  directory = await mkdtemp(path.join(tmpdir(), 'hcp044-details-'));
  try {
    const checkout = path.join(directory, 'checkout'),
      web = path.join(checkout, 'web');
    execFileSync('git', [
      'clone',
      '--quiet',
      '--no-hardlinks',
      capsule,
      checkout
    ]);
    for (const relative of ['package.json', 'pnpm-lock.yaml', 'src'])
      await cp(path.join(capsule, 'web', relative), path.join(web, relative), {
        recursive: true
      });
    const identities: { path: string; sha256: string }[] = [];
    async function identify(dir: string, prefix = '') {
      for (const entry of await readdir(dir, { withFileTypes: true })) {
        const relative = prefix + entry.name;
        if (entry.isDirectory())
          await identify(path.join(dir, entry.name), relative + '/');
        else
          identities.push({
            path: relative,
            sha256: createHash('sha256')
              .update(await readFile(path.join(dir, entry.name)))
              .digest('hex')
          });
      }
    }
    await identify(path.join(web, 'src'));
    await cp(
      path.join(capsule, 'web/tools/source-boundaries.mjs'),
      path.join(web, 'tools/source-boundaries.mjs')
    );
    identities.push({
      path: 'tools/source-boundaries.mjs',
      sha256: createHash('sha256')
        .update(await readFile(path.join(web, 'tools/source-boundaries.mjs')))
        .digest('hex')
    });
    const policy =
      "import { validateRelayPolicy } from './relays.ts'; const policy=validateRelayPolicy(JSON.stringify({schemaVersion:1,public:[{origin:'wss://one.example.org',read:true,write:false,nip50:false},{origin:'wss://failed.example.org',read:true,write:false,nip50:false}],inbox:[],postingEnabled:false,messagingEnabled:false,operatorDenylist:[]})); if(!policy) throw new Error('fixture_policy'); export const deploymentRelayPolicy=policy;";
    await writeFile(
      path.join(web, 'src/lib/config/deployment-relays.ts'),
      policy
    );
    const sourceStore = execFileSync('corepack', ['pnpm', 'store', 'path'], {
      cwd: path.join(capsule, 'web')
    })
      .toString()
      .trim();
    const installed = execFileSync(
      'corepack',
      [
        'pnpm',
        'install',
        '--offline',
        '--frozen-lockfile',
        '--store-dir',
        path.dirname(sourceStore)
      ],
      { cwd: web, timeout: 90000 }
    ).toString();
    const sourceAudit = execFileSync(
      process.execPath,
      ['tools/check-source.mjs'],
      { cwd: web }
    ).toString();
    execFileSync(process.execPath, ['tools/build-info.mjs'], { cwd: web });
    const compiled = execFileSync(
      process.execPath,
      ['node_modules/vite/bin/vite.js', 'build'],
      { cwd: web, timeout: 90000 }
    ).toString();
    console.log(
      JSON.stringify({
        fixture: 'HCP044_TEST_ONLY_FIXED_POLICY',
        identities,
        policy,
        policySha256: createHash('sha256').update(policy).digest('hex'),
        sourceStore,
        installed,
        sourceAudit,
        compiled
      })
    );
    server = await createStaticHarness({ buildRoot: path.join(web, 'build') });
  } catch (error) {
    await rm(directory, { recursive: true, force: true });
    throw error;
  }
});
test.afterAll(async () => {
  if (server) await server.close();
  if (directory) await rm(directory, { recursive: true, force: true });
});
test('real product reload renders verified facts, safe guest contact, explicit copy and truthful capability/status gates', async ({
  browser
}) => {
  const corpus = JSON.parse(
    await readFile(
      path.join(capsule, 'contracts/interop/food_availability/corpus.v1.json'),
      'utf8'
    )
  ) as { vectors: { id: string; signed_wires: Record<string, string> }[] };
  const base = JSON.parse(
    corpus.vectors.find((v) => v.id.endsWith('_032'))!.signed_wires.previous
  ) as { tags: string[][]; content: string; created_at: number };
  const key = crypto.getRandomValues(new Uint8Array(32));
  const listings = ['active', 'sold', 'withdrawn', 'unsupported', 'future'].map(
    (status, n) =>
      finalizeEvent(
        {
          kind: 30402,
          created_at:
            status === 'future'
              ? Math.floor(Date.now() / 1000) + 36000
              : base.created_at - n,
          tags:
            status === 'unsupported'
              ? [
                  ['d', status],
                  ['title', 'Other format']
                ]
              : [
                  ...base.tags.map((t) =>
                    t[0] === 'd'
                      ? ['d', status]
                      : t[0] === 'title'
                        ? ['title', `Carrots ${status}`]
                        : t[0] === 'status'
                          ? ['status', status === 'sold' ? 'sold' : 'active']
                          : [...t]
                  ),
                  ['image', 'https://tracking.example.org/image.png']
                ],
          content:
            '<img src=https://tracking.example.org> Contact only if supplied.\n\nPublic contact: https://contact.example.org/collect'
        },
        key
      )
  );
  const withdrawn = listings[2];
  const deletion = finalizeEvent(
    {
      kind: 5,
      created_at: base.created_at + 1,
      tags: [['e', withdrawn.id]],
      content: ''
    },
    key
  );
  const profile = finalizeEvent(
    {
      kind: 0,
      created_at: base.created_at,
      tags: [],
      content: JSON.stringify({
        display_name: '<img src=https://tracking.example.org>'
      })
    },
    key
  );
  key.fill(0);
  const relay = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 65536
  });
  const queries: Record<string, unknown>[] = [],
    closed: string[] = [];
  relay.on('connection', (socket) => {
    socket.on('message', (bytes) => {
      const frame = JSON.parse(wireText(bytes)) as unknown[];
      if (frame[0] === 'CLOSE') {
        closed.push(String(frame[1]));
        return;
      }
      if (frame[0] !== 'REQ') return;
      const id = String(frame[1]),
        filters = frame.slice(2) as Record<string, unknown>[];
      queries.push(...filters);
      const kind = (filters[0].kinds as number[])[0];
      const values =
        kind === 0
          ? [profile]
          : kind === 30402
            ? listings.filter((v) =>
                filters.some(
                  (f) =>
                    (!f.authors ||
                      (f.authors as string[]).includes(v.pubkey)) &&
                    (!f['#d'] ||
                      (f['#d'] as string[]).includes(
                        v.tags.find((t) => t[0] === 'd')![1]
                      )) &&
                    (!f.until || v.created_at <= Number(f.until))
                )
              )
            : kind === 5
              ? [deletion]
              : [];
      for (const value of values)
        socket.send(JSON.stringify(['EVENT', id, value]));
      socket.send(JSON.stringify(['EOSE', id]));
    });
  });
  await once(relay, 'listening');
  const address = relay.address();
  if (!address || typeof address === 'string') throw Error('fixture_port');
  const context = await browser.newContext({
      viewport: { width: 320, height: 700 },
      permissions: ['clipboard-read', 'clipboard-write']
    }),
    bridges: WebSocket[] = [],
    external: string[] = [],
    failures: string[] = [];
  try {
    await context.route('**/*', (route) => {
      if (new URL(route.request().url()).origin === server.url)
        return route.continue();
      external.push(route.request().url());
      return route.abort();
    });
    await context.routeWebSocket('**/*', (route) => {
      if (new URL(route.url()).hostname === 'failed.example.org') {
        route.onMessage((message) => {
          const frame = JSON.parse(String(message)) as unknown[];
          if (frame[0] === 'REQ')
            route.send(
              JSON.stringify(['CLOSED', frame[1], 'fixture source unavailable'])
            );
        });
        return;
      }
      expect(new URL(route.url()).hostname).toBe('one.example.org');
      const bridge = new WebSocket(`ws://127.0.0.1:${address.port}`);
      bridges.push(bridge);
      const pending: (string | Buffer)[] = [];
      bridge.on('open', () => {
        for (const message of pending.splice(0)) bridge.send(message);
      });
      bridge.on('message', (bytes) => route.send(wireText(bytes)));
      route.onMessage((message) => {
        if (bridge.readyState === WebSocket.OPEN) bridge.send(message);
        else pending.push(message);
      });
      route.onClose(() => {
        bridge.close();
      });
    });
    const page = await context.newPage();
    page.on('pageerror', (e) => failures.push(e.message));
    const coordinate = (identifier: string) =>
      naddrEncode({ kind: 30402, pubkey: listings[0].pubkey, identifier });
    const href = (identifier: string) =>
      server.url + '/products/' + coordinate(identifier);
    await page.goto(href('active'));
    await expect(
      page.getByRole('heading', { name: 'Carrots active', exact: true })
    ).toBeVisible();
    await expect(
      page.getByRole('button', { name: 'Message seller', exact: true })
    ).toBeDisabled();
    await expect(
      page.getByText('In-app messaging is unavailable during development.')
    ).toBeVisible();
    await expect(
      page.getByRole('button', { name: 'Manage listing' })
    ).toHaveCount(0);
    await expect(page.locator('img,iframe,textarea')).toHaveCount(0);
    await page.getByText('Publisher identity', { exact: true }).click();
    await expect(page.getByText(profile.pubkey, { exact: true })).toBeVisible();
    await page.getByText('Other contact details', { exact: true }).click();
    await expect(
      page.getByRole('link', {
        name: 'https://contact.example.org/collect',
        exact: true
      })
    ).toHaveAttribute('href', 'https://contact.example.org/collect');
    await page.getByRole('button', { name: 'Copy link', exact: true }).click();
    await expect(page.getByRole('status')).toHaveText('Link copied.');
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
      href('active')
    );
    // Test-only anchor exercises Kit client parameter navigation on the actual
    // component; survival proves this transition did not reload the document.
    await page.evaluate((target) => {
      const link = document.createElement('a');
      link.href = target;
      link.textContent = 'Fixture next food';
      link.id = 'fixture-client-transition';
      document.body.append(link);
    }, href('sold'));
    await page
      .getByRole('link', { name: 'Fixture next food', exact: true })
      .click();
    await expect(
      page.getByText('The seller marked this listing sold.', { exact: true })
    ).toBeVisible();
    await expect(page.locator('#fixture-client-transition')).toHaveCount(1);
    await page.goBack();
    await expect(
      page.getByRole('heading', { name: 'Carrots active', exact: true })
    ).toBeVisible();
    await page.reload();
    await expect(
      page.getByRole('heading', { name: 'Carrots active', exact: true })
    ).toBeVisible();
    for (const [identifier, notice] of [
      ['sold', 'The seller marked this listing sold.'],
      ['withdrawn', 'This listing was withdrawn by its publisher.'],
      [
        'unsupported',
        'The latest known listing uses an unsupported food format.'
      ],
      [
        'future',
        'Listing time could not be confirmed. Food details are unavailable.'
      ],
      [
        'unobserved',
        'This listing was not observed in the sources checked. It may be unavailable or missing from these sources.'
      ]
    ]) {
      await page.goto(href(identifier));
      await expect(page.getByText(notice, { exact: true })).toBeVisible();
      await expect(
        page.getByRole('button', { name: 'Message seller', exact: true })
      ).toHaveCount(0);
    }
    await page.goto(server.url + '/products/invalid-reference');
    await expect(page.getByText('Not Found', { exact: true })).toBeVisible();
    expect(external).toEqual([]);
    expect(failures).toEqual([]);
    expect(await page.evaluate(() => 'nostr' in window)).toBe(false);
    console.log(
      JSON.stringify({
        fixture: 'HCP044_GENUINE_LOOPBACK_DETAIL',
        queries,
        external,
        failures
      })
    );
  } finally {
    await context.close();
    for (const bridge of bridges) bridge.terminate();
    for (const socket of relay.clients) socket.terminate();
    await new Promise<void>((resolve, reject) =>
      relay.close((e) => (e ? reject(e) : resolve()))
    );
  }
});
