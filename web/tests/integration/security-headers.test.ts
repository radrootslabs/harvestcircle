import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  prepareSecurityHeaders,
  extractFrameworkStyleAttributes,
  relayConnectSources,
  verifySecurityHeaders
} from '../../tools/security-headers.mjs';
import { validateRelayPolicy } from '../../src/lib/config/relays.ts';
import { createStaticHarness } from './harness/static.ts';

const web = fileURLToPath(new URL('../../', import.meta.url));
const sha = (bytes: Uint8Array) =>
  createHash('sha256').update(bytes).digest('hex');

await test('actual compiled announcer extraction rejects missing, altered, duplicate, extra and substituted styles', async () => {
  const manifest = JSON.parse(
    await readFile(
      path.join(web, '.svelte-kit/output/client/.vite/manifest.json'),
      'utf8'
    )
  ) as Record<string, { name?: string; file: string }>;
  const navigation = Object.values(manifest).filter(
    (entry) => entry.name === 'navigation'
  );
  assert.equal(navigation.length, 1);
  const file = navigation[0].file;
  const source = await readFile(path.join(web, 'build', file), 'utf8');
  const literal =
    /`(<div id="svelte-announcer" aria-live="assertive" aria-atomic="true" style="[^"]+"><!><\/div>)`/.exec(
      source
    );
  assert.ok(literal);
  assert.equal(
    extractFrameworkStyleAttributes(
      file,
      Buffer.from(source),
      sha(Buffer.from(source))
    ).length,
    1
  );
  assert.throws(
    () =>
      extractFrameworkStyleAttributes(
        file,
        Buffer.from(source),
        '0'.repeat(64)
      ),
    /security_headers_stale_or_invalid/
  );
  const positive = 'const announcer = ' + literal[0] + ';';
  assert.equal(
    extractFrameworkStyleAttributes(
      file,
      Buffer.from(positive),
      sha(Buffer.from(positive))
    ).length,
    1
  );
  for (const candidate of [
    'const missing = 1;',
    positive.replace('position: absolute', 'position: fixed'),
    positive + 'const duplicate = ' + literal[0] + ';',
    positive + 'const extra = `<div style="position: fixed"></div>`;',
    positive.replace('position: absolute', 'position: ${value}'),
    positive + 'const broken = ('
  ])
    assert.throws(
      () =>
        extractFrameworkStyleAttributes(
          file,
          Buffer.from(candidate),
          sha(Buffer.from(candidate))
        ),
      /security_headers_unreviewed_style_shape/
    );
});

await test('actual command prepares external evidence, checks freshness and refuses overwrite', async () => {
  const directory = await realpath(
    await mkdtemp(path.join(tmpdir(), 'hc headers '))
  );
  const output = path.join(directory, 'headers.json');
  const command = (operation: string, destination = output) =>
    execFileSync(
      process.execPath,
      ['tools/security-headers.mjs', operation, destination],
      { cwd: web, stdio: 'pipe' }
    ).toString();
  try {
    assert.match(command('prepare'), /qualification remains pending/);
    assert.match(command('check'), /match the current audited build/);
    assert.throws(() => command('prepare'), /EEXIST/);
    assert.throws(
      () => command('prepare', path.join(web, 'build/headers.json')),
      /outside the capsule/
    );
    const candidate: unknown = JSON.parse(await readFile(output, 'utf8'));
    assert.ok(candidate && typeof candidate === 'object');
    Reflect.set(candidate, 'readiness', 'QUALIFIED');
    await writeFile(output, JSON.stringify(candidate));
    assert.throws(() => command('check'), /security_headers_stale_or_invalid/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

await test('connect-src is exactly the two admitted scopes, detached from caller hints', () => {
  const policy = validateRelayPolicy(
    JSON.stringify({
      schemaVersion: 1,
      public: [
        {
          origin: 'wss://one.example.org',
          read: true,
          write: false,
          nip50: false
        },
        {
          origin: 'wss://shared.example.org',
          read: false,
          write: true,
          nip50: false
        }
      ],
      inbox: [
        { origin: 'wss://shared.example.org', read: true, write: true },
        { origin: 'wss://two.example.org', read: true, write: false }
      ],
      postingEnabled: false,
      messagingEnabled: false,
      operatorDenylist: []
    })
  );
  assert.ok(policy);
  const sources = relayConnectSources(policy);
  assert.deepEqual(sources, [
    'wss://one.example.org',
    'wss://shared.example.org',
    'wss://two.example.org'
  ]);
  sources.push('wss://hinted.example.org');
  assert.deepEqual(relayConnectSources(policy), sources.slice(0, -1));
  assert.throws(
    () => relayConnectSources({ ...policy }),
    /relay_policy_invalid/
  );
});

await test('actual audited HTML supplies every exact script hash without unsafe policies', async () => {
  const prepared = await prepareSecurityHeaders(web);
  assert.equal(prepared.schema, 'harvestcircle.static_security_headers.v1');
  assert.equal(
    prepared.readiness,
    'SOURCE_PREPARATION_ONLY_HOST_QUALIFICATION_PENDING'
  );
  assert.equal(Object.keys(prepared.artifacts).length, 59);
  assert.equal(Object.keys(prepared.inline_scripts).length, 8);
  assert.deepEqual(prepared.connect_sources, []);
  const csp = prepared.headers['Content-Security-Policy'];
  assert.equal(prepared.framework_style_attributes.length, 1);
  const style = prepared.framework_style_attributes[0];
  assert.equal(style.owner, 'svelte-announcer');
  assert.equal(style.source_sha256, prepared.artifacts[style.file].sha256);
  const compiled = await readFile(path.join(web, 'build', style.file), 'utf8');
  const attribute =
    /id="svelte-announcer" aria-live="assertive" aria-atomic="true" style="([^"]+)"/.exec(
      compiled
    );
  assert.ok(attribute);
  assert.equal(style.style_sha256, sha(Buffer.from(attribute[1])));
  assert.equal(
    style.csp_hash,
    "'sha256-" +
      createHash('sha256').update(attribute[1]).digest('base64') +
      "'"
  );
  assert.ok(csp.includes("style-src-attr 'unsafe-hashes' " + style.csp_hash));
  assert.equal(
    prepared.compiler_manifest_sha256,
    sha(
      await readFile(
        path.join(web, '.svelte-kit/output/client/.vite/manifest.json')
      )
    )
  );
  assert.match(csp, /(?:^|; )connect-src 'none'(?:;|$)/);
  assert.doesNotMatch(
    csp,
    /unsafe-inline|unsafe-eval|wasm-unsafe-eval|data:|blob:|https:|wss:|\*/
  );
  for (const [name, artifact] of Object.entries(prepared.artifacts)) {
    const bytes = await readFile(
      new URL('../../build/' + name, import.meta.url)
    );
    assert.equal(artifact.sha256, sha(bytes), name);
    assert.equal(artifact.bytes, bytes.length, name);
    if (!name.endsWith('.html')) continue;
    const scripts = [
      ...bytes.toString('utf8').matchAll(/<script>([\s\S]*?)<\/script>/g)
    ];
    assert.equal(scripts.length, 1, name);
    const hashes = scripts.map(
      (match) =>
        "'sha256-" +
        createHash('sha256').update(match[1]).digest('base64') +
        "'"
    );
    assert.deepEqual(prepared.inline_scripts[name], hashes);
    for (const hash of hashes) assert.ok(csp.includes(hash), name);
  }
  assert.equal(prepared.headers['Referrer-Policy'], 'no-referrer');
  assert.equal(
    prepared.headers['Permissions-Policy'],
    'camera=(), microphone=(), geolocation=(), payment=(), usb=()'
  );
  for (const directive of [
    "frame-ancestors 'none'",
    "base-uri 'none'",
    "form-action 'none'",
    "object-src 'none'",
    "worker-src 'none'",
    "img-src 'self'",
    "font-src 'self'",
    "style-src 'self'"
  ])
    assert.ok(csp.includes(directive), directive);
  await verifySecurityHeaders(
    web,
    JSON.parse(JSON.stringify(prepared)) as unknown
  );
});

await test('stale, widened or falsely qualified header records are rejected', async () => {
  const original = await prepareSecurityHeaders(web);
  for (const poison of [
    'headers',
    'artifact',
    'script',
    'style',
    'manifest',
    'policy',
    'qualification'
  ]) {
    const candidate = structuredClone(original);
    if (poison === 'headers')
      candidate.headers['Content-Security-Policy'] += '; connect-src *';
    if (poison === 'artifact')
      candidate.artifacts['200.html'].sha256 = '0'.repeat(64);
    if (poison === 'script') candidate.inline_scripts['200.html'] = [];
    if (poison === 'style')
      candidate.framework_style_attributes[0].style_sha256 = '0'.repeat(64);
    if (poison === 'manifest')
      candidate.compiler_manifest_sha256 = '0'.repeat(64);
    if (poison === 'policy')
      candidate.connect_sources.push('wss://hinted.example.org');
    if (poison === 'qualification')
      Reflect.set(candidate, 'readiness', 'QUALIFIED');
    await assert.rejects(
      verifySecurityHeaders(web, candidate),
      /security_headers_stale_or_invalid/
    );
  }
});

await test('actual HTTP serves prepared headers on navigation and real missing-asset failures', async () => {
  const prepared = await prepareSecurityHeaders(web);
  const server = await createStaticHarness({ headers: prepared.headers });
  try {
    for (const pathname of [
      '/',
      '/search',
      '/sell',
      '/selling',
      '/messages',
      '/about',
      '/privacy',
      '/products/unsupported-coordinate',
      '/products/unsupported-coordinate/edit',
      '/selling/drafts/unknown-draft',
      '/messages/unknown-conversation'
    ]) {
      const response = await fetch(server.url + pathname, {
        headers: { accept: 'text/html' }
      });
      assert.equal(response.status, 200, pathname);
      for (const [name, value] of Object.entries(prepared.headers))
        assert.equal(response.headers.get(name), value, name);
      assert.equal(response.headers.get('content-type'), 'text/html');
      await response.arrayBuffer();
    }
    for (const pathname of [
      '/missing.js',
      '/missing.css',
      '/missing.png',
      '/products/missing.js'
    ]) {
      const response = await fetch(server.url + pathname, {
        headers: { accept: 'text/html' }
      });
      assert.equal(response.status, 404, pathname);
      assert.equal(
        response.headers.get('content-security-policy'),
        prepared.headers['Content-Security-Policy']
      );
      assert.doesNotMatch(response.headers.get('content-type') ?? '', /html/);
      assert.equal(await response.text(), '');
    }
    assert.equal((await fetch(server.url, { method: 'POST' })).status, 405);
  } finally {
    await server.close();
  }
  assert.deepEqual(server.state(), { listening: false, childProcesses: 0 });
});
