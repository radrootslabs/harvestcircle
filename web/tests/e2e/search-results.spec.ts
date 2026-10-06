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
  directory = await mkdtemp(path.join(tmpdir(), 'hcp043-search-'));
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
    await cp(path.join(capsule, 'web/src'), path.join(web, 'src'), {
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
    const installed = execFileSync(
      'corepack',
      ['pnpm', 'install', '--offline', '--frozen-lockfile'],
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
        fixture: 'HCP043_TEST_ONLY_FIXED_POLICY',
        identities,
        policy,
        policySha256: createHash('sha256').update(policy).digest('hex'),
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
test('real search renders verified loopback rows, persistent partial notices, independent row actions and restored query/scroll', async ({
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
  const listings = Array.from({ length: 26 }, (_, n) =>
    finalizeEvent(
      {
        kind: 30402,
        created_at: base.created_at - n,
        tags: base.tags.map((t) =>
          t[0] === 'd'
            ? ['d', `browser_${n}`]
            : t[0] === 'title'
              ? ['title', `Carrots ${n}`]
              : [...t]
        ),
        content: base.content
      },
      key
    )
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
      viewport: { width: 320, height: 700 }
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
    await page.goto(server.url + '/search?q=carrots');
    const input = page.getByRole('textbox', {
      name: 'What are you looking for?'
    });
    await expect(input).toBeEnabled();
    await input.focus();
    const rows = page
      .getByRole('listitem')
      .filter({ has: page.getByRole('link', { name: /^Carrots / }) });
    try {
      await expect(rows).toHaveCount(20);
    } catch (error) {
      console.log(
        JSON.stringify({
          fixtureFailure: true,
          failures,
          queries,
          external,
          body: await page.locator('body').innerText()
        })
      );
      throw error;
    }
    await expect(input).toBeFocused();
    await expect(
      page.getByText('These results may be incomplete.', { exact: false })
    ).toBeVisible();
    await expect(
      page.getByRole('button', { name: 'Show more', exact: true })
    ).toBeVisible();
    const discovery = () =>
      queries.filter((q) => (q.kinds as number[])[0] === 30402 && !q.authors)
        .length;
    const before = discovery();
    await page.getByRole('button', { name: 'Show more', exact: true }).click();
    await expect(rows).toHaveCount(26);
    expect(discovery()).toBe(before);
    await page.waitForTimeout(350);
    expect(discovery()).toBe(before);
    const first = rows.first();
    await first.getByText('Publisher details', { exact: true }).click();
    await expect(
      first.getByText(profile.pubkey, { exact: true })
    ).toBeVisible();
    expect(new URL(page.url()).pathname).toBe('/search');
    await expect(
      page.getByRole('button', { name: 'Search older listings', exact: true })
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth
      )
    ).toBe(true);
    await rows.nth(3).getByRole('link').scrollIntoViewIfNeeded();
    const scroll = await page.evaluate(() => scrollY);
    await rows.nth(3).getByRole('link').click();
    await expect(page).toHaveURL(/\/products\/naddr/);
    await page.goBack();
    await expect(
      page.getByRole('textbox', { name: 'What are you looking for?' })
    ).toHaveValue('carrots');
    // Kit history snapshots and the reviewed scalar helper restore public query
    // position after genuine rows render. No private context is retained.
    await expect
      .poll(() => page.evaluate(() => scrollY))
      .toBeCloseTo(scroll, -1);
    await page
      .getByRole('textbox', { name: 'What are you looking for?' })
      .fill('turnips');
    await page.getByRole('button', { name: 'Search', exact: true }).click();
    await expect(page).toHaveURL(server.url + '/search?q=turnips');
    await expect(rows).toHaveCount(0);
    await page.goBack();
    await expect(
      page.getByRole('textbox', { name: 'What are you looking for?' })
    ).toHaveValue('carrots');
    await expect(rows).toHaveCount(20);
    await page.goForward();
    await expect(input).toHaveValue('turnips');
    await expect(rows).toHaveCount(0);
    await page.goBack();
    await expect(input).toHaveValue('carrots');
    await expect(rows).toHaveCount(20);
    const olderCount = discovery();
    await page
      .getByRole('button', { name: 'Search older listings', exact: true })
      .click();
    await expect.poll(discovery).toBe(olderCount + 1);
    await page.goto(server.url + '/about');
    await expect.poll(() => closed.length).toBeGreaterThan(0);
    expect(external).toEqual([]);
    expect(failures).toEqual([]);
    expect(await page.evaluate(() => 'nostr' in window)).toBe(false);
  } finally {
    await context.close();
    for (const bridge of bridges) bridge.terminate();
    for (const socket of relay.clients) socket.terminate();
    await new Promise<void>((resolve, reject) =>
      relay.close((e) => (e ? reject(e) : resolve()))
    );
  }
});
