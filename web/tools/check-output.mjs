import { execFileSync } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';
import { constants } from 'node:fs';
import { lstat, open, opendir, realpath } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { deriveBuildInfo } from './build-info.mjs';

/** @param {unknown} actual @param {unknown} expected @param {string} [message] */
function equal(actual, expected, message = 'Invalid public output shape') {
  if (!isDeepStrictEqual(actual, expected)) throw new Error(message);
}
/** @param {string} text */
function parse(text) {
  try {
    return JSON.parse(text);
  } catch {
    throw new Error('Invalid static JSON');
  }
}
const maxBytes = 8 * 1024 * 1024;
/** @param {string} name */
function safeName(name) {
  if (
    !name ||
    name.includes('\\') ||
    name.split('/').some((part) => !part || part === '.' || part === '..') ||
    [...name].some(
      (character) =>
        character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127
    )
  )
    throw new Error('Unsafe output path');
}
/** @param {string} root @param {string} name */
async function readOwned(root, name) {
  safeName(name);
  const parts = name.split('/');
  let file = root;
  for (let i = 0; i < parts.length; i++) {
    file = path.join(file, parts[i]);
    const stat = await lstat(file);
    if (
      i < parts.length - 1
        ? !stat.isDirectory()
        : !stat.isFile() ||
          stat.nlink !== 1 ||
          Boolean(stat.mode & 0o111) ||
          stat.size > maxBytes
    )
      throw new Error('Unsafe output entry');
  }
  if ((await realpath(file)) !== file)
    throw new Error('Unsafe output ancestry');
  const handle = await open(
    file,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK
  );
  try {
    const stat = await handle.stat();
    if (
      !stat.isFile() ||
      stat.nlink !== 1 ||
      Boolean(stat.mode & 0o111) ||
      stat.size > maxBytes
    )
      throw new Error('Unsafe output entry');
    const chunks = [];
    let size = 0;
    for (;;) {
      const buffer = Buffer.alloc(64 * 1024);
      const { bytesRead } = await handle.read(buffer);
      if (!bytesRead) break;
      size += bytesRead;
      if (size > maxBytes) throw new Error('Oversized output entry');
      chunks.push(buffer.subarray(0, bytesRead));
    }
    return Buffer.concat(chunks);
  } finally {
    await handle.close();
  }
}
/** @param {string} root */
async function inventory(root) {
  if (!(await lstat(root)).isDirectory() || (await realpath(root)) !== root)
    throw new Error('Unsafe output root');
  const files = new Map();
  let entries = 0;
  let total = 0;
  /** @param {string} prefix */
  async function walk(prefix) {
    let childCount = 0;
    for await (const entry of await opendir(path.join(root, prefix))) {
      childCount++;
      if (++entries > 512) throw new Error('Unbounded output inventory');
      const name = prefix ? `${prefix}/${entry.name}` : entry.name;
      safeName(name);
      if (name.split('/').length > 8) throw new Error('Unbounded output depth');
      const stat = await lstat(path.join(root, name));
      if (stat.isDirectory()) {
        await walk(name);
      } else {
        const bytes = await readOwned(root, name);
        total += bytes.length;
        if (total > 32 * 1024 * 1024) throw new Error('Unbounded output bytes');
        files.set(name, bytes);
      }
    }
    if (prefix && !childCount)
      throw new Error('Undeclared empty output directory');
  }
  await walk('');
  return files;
}
/** @param {Uint8Array} bytes */
function publicText(bytes) {
  const buffer = Buffer.from(bytes);
  // Controlled binary/container/database signatures, irrespective of filename.
  const magic = [
    '7f454c46',
    'cffaedfe',
    'cefaedfe',
    'feedfacf',
    'feedface',
    'cafebabe',
    'bebafeca',
    '504b0304',
    '504b0506',
    '1f8b',
    '0061736d'
  ];
  if (
    magic.some(
      (hex) => buffer.subarray(0, hex.length / 2).toString('hex') === hex
    ) ||
    buffer.subarray(0, 2).toString() === 'MZ' ||
    buffer.subarray(0, 16).toString() === 'SQLite format 3\0'
  )
    throw new Error('Static binary contamination');
  const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  if (
    [...text].some((character) => {
      const code = character.charCodeAt(0);
      return code < 32 && ![9, 10, 13].includes(code);
    })
  )
    throw new Error('Invalid static text');
  // These are bounded contamination controls, not an arbitrary-secret scanner.
  if (
    /UniFFI|HC_TEST_ONLY_SECRET|nsec1[023456789acdefghjklmnpqrstuvwxyz]{20,}|-----BEGIN (?:[A-Z ]*PRIVATE KEY|OPENSSH)|private[_ -]transcript|TestSigner|test[_ -]provider|createRelayHarness|createStaticHarness|installControlledProvider|sourceMappingURL|radroots\.lib\.source-lock|core\/Cargo\.lock|docs\/oss\/harvestcircle|HARVESTCIRCLE_SECRET/i.test(
      text
    )
  )
    throw new Error('Static controlled contamination');
  return text;
}
/** @param {unknown} value @param {string[]} keys */
function objectKeys(value, keys) {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error('Invalid compiler object');
  if (Object.keys(value).some((key) => !keys.includes(key)))
    throw new Error('Unknown compiler field');
}
/** @param {string} webDirectory */
export async function auditOutput(webDirectory) {
  const web = path.resolve(webDirectory);
  if (path.basename(web) !== 'web' || (await realpath(web)) !== web)
    throw new Error('Output audit requires the owned web directory');
  // Pure derivation: auditing must not repair or overwrite poisoned metadata.
  const gitEnv = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_'))
  );
  try {
    execFileSync(
      'git',
      [
        '-C',
        path.dirname(web),
        'check-ignore',
        '-q',
        '--',
        'web/static/build-info.json'
      ],
      { env: gitEnv, stdio: 'pipe' }
    );
  } catch {
    throw new Error('Generated public provenance must be ignored');
  }
  const expectedMetadata = Buffer.from(
    JSON.stringify(await deriveBuildInfo(web), null, 2) + '\n'
  );
  const generated = await readOwned(web, 'static/build-info.json');
  equal(generated, expectedMetadata, 'Stale or nonpublic generated provenance');
  const client = await inventory(path.join(web, '.svelte-kit/output/client'));
  const pages = await inventory(
    path.join(web, '.svelte-kit/output/prerendered/pages')
  );
  const output = await inventory(path.join(web, 'build'));
  const manifestBytes = client.get('.vite/manifest.json');
  if (!manifestBytes) throw new Error('Missing compiler manifest');
  const manifest = parse(publicText(manifestBytes));
  objectKeys(manifest, Object.keys(manifest));
  // Exact source-owned search route admission; root-only controlled consumers
  // retain their original compiler shape. Source ancestry/link bounds also apply.
  let hasSearch = false;
  try {
    await readOwned(web, 'src/routes/search/+page.svelte');
    hasSearch = true;
  } catch (error) {
    if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT'))
      throw error;
  }
  const fullRouteSources = [
    'sell/+page.svelte',
    'selling/+page.svelte',
    'selling/drafts/[draftId=local_id]/+page.svelte',
    'products/[naddr=naddr]/+page.svelte',
    'products/[naddr=naddr]/edit/+page.svelte',
    'messages/+page.svelte',
    'messages/[conversationId=local_id]/+page.svelte',
    'about/+page.svelte',
    'privacy/+page.svelte'
  ];
  let presentRoutes = 0;
  for (const name of fullRouteSources) {
    try {
      await readOwned(web, 'src/routes/' + name);
      presentRoutes++;
    } catch (error) {
      if (!(
        error instanceof Error &&
        'code' in error &&
        error.code === 'ENOENT'
      ))
        throw error;
    }
  }
  const hasFullRoutes = presentRoutes !== 0;
  if (hasFullRoutes) {
    if (presentRoutes !== fullRouteSources.length || !hasSearch)
      throw new Error('Undeclared compiler module: incomplete route source');
    for (const name of [
      'products/[naddr=naddr]/+page.ts',
      'products/[naddr=naddr]/edit/+page.ts',
      'selling/drafts/[draftId=local_id]/+page.ts',
      'messages/[conversationId=local_id]/+page.ts'
    ])
      await readOwned(web, 'src/routes/' + name);
    for (const name of [
      'src/params.ts',
      'src/routes/+error.svelte',
      'src/lib/components/AccountGate.svelte'
    ])
      await readOwned(web, name);
  }
  // Admit compiler-owned module identities, not arbitrary extensions/copy roots.
  const names = new Set([
    'entry/app',
    'entry/start',
    'entry/payload',
    'nodes/0',
    'nodes/1',
    'nodes/2',
    'error-template',
    'preload-helper',
    'client',
    'client.svelte',
    'client-entry',
    'state',
    'Button'
  ]);
  if (hasSearch) {
    names.add('nodes/3');
    names.add('routes');
  }
  if (hasFullRoutes) {
    for (let index = 4; index <= 12; index++) names.add('nodes/' + index);
    for (const name of [
      'AccountGate',
      'references',
      'legacy',
      'rolldown-runtime',
      'shared-errors'
    ])
      names.add(name);
  }
  const admitted = new Map([['build-info.json', expectedMetadata]]);
  const seen = new Set();
  for (const [key, record] of Object.entries(manifest)) {
    objectKeys(record, [
      'file',
      'name',
      'src',
      'isEntry',
      'isDynamicEntry',
      'imports',
      'dynamicImports',
      'css'
    ]);
    if (!names.has(record.name) || seen.has(record.name))
      throw new Error('Undeclared compiler module');
    seen.add(record.name);
    const generatedModule = hasFullRoutes
      ? /^\.svelte-kit\/generated\/build\/(?:client-optimized\/(?:app|nodes\/(?:[0-9]|1[012]))|shared\/error-template)\.js$/
      : hasSearch
        ? /^\.svelte-kit\/generated\/build\/(?:client-optimized\/(?:app|nodes\/[0123])|shared\/error-template)\.js$/
        : /^\.svelte-kit\/generated\/build\/(?:client-optimized\/(?:app|nodes\/[012])|shared\/error-template)\.js$/;
    const frameworkModule =
      /^node_modules\/\.pnpm\/[^/]+\/node_modules\/@sveltejs\/kit\/src\/runtime\/client\/(?:client-entry|entry|payload)\.js$/;
    if (!(
      generatedModule.test(key) ||
      frameworkModule.test(key) ||
      /^_[A-Za-z0-9_-]+\.js$/.test(key)
    ))
      throw new Error('Undeclared compiler input');
    if (
      record.src !== undefined &&
      (record.src !== key ||
        !(generatedModule.test(key) || frameworkModule.test(key)))
    )
      throw new Error('Invalid compiler source');
    if (
      !/^_app\/immutable\/(?:(?:entry|nodes)\/[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+|chunks\/[A-Za-z0-9_-]+)\.js$/.test(
        record.file
      )
    )
      throw new Error('Undeclared compiler output');
    for (const flag of ['isEntry', 'isDynamicEntry'])
      if (record[flag] !== undefined && typeof record[flag] !== 'boolean')
        throw new Error('Invalid compiler flag');
    for (const field of ['imports', 'dynamicImports', 'css']) {
      if (
        record[field] !== undefined &&
        (!Array.isArray(record[field]) ||
          record[field].some((item) => typeof item !== 'string') ||
          new Set(record[field]).size !== record[field].length)
      )
        throw new Error('Invalid compiler references');
    }
    for (const dependency of [
      ...(record.imports ?? []),
      ...(record.dynamicImports ?? [])
    ])
      if (!Object.hasOwn(manifest, dependency))
        throw new Error('Unresolved compiler import');
    for (const file of [record.file, ...(record.css ?? [])]) {
      safeName(file);
      if (
        file !== record.file &&
        !/^_app\/immutable\/assets\/[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.css$/.test(
          file
        )
      )
        throw new Error('Undeclared compiler resource');
      const bytes = client.get(file);
      if (!bytes) throw new Error('Missing compiler output');
      publicText(bytes);
      const previous = admitted.get(file);
      if (previous && !previous.equals(bytes))
        throw new Error('Conflicting compiler output');
      admitted.set(file, bytes);
    }
  }
  equal([...seen].sort(), [...names].sort(), 'Incomplete compiler modules');
  if (hasFullRoutes) {
    const robots = await readOwned(web, 'static/robots.txt');
    publicText(robots);
    equal(
      client.get('robots.txt'),
      robots,
      'Robots differs from owned static source'
    );
    admitted.set('robots.txt', robots);
  }
  const versionBytes = client.get('_app/version.json');
  if (!versionBytes) throw new Error('Missing compiler version');
  const version = parse(publicText(versionBytes));
  equal(Object.keys(version), ['version']);
  if (
    typeof version.version !== 'string' ||
    !/^[A-Za-z0-9._-]{1,128}$/.test(version.version)
  )
    throw new Error('Invalid compiler version');
  admitted.set('_app/version.json', versionBytes);
  equal(
    [...client.keys()].sort(),
    [...admitted.keys(), '.vite/manifest.json'].sort(),
    'Compiler inventory mismatch'
  );
  equal(
    client.get('build-info.json'),
    expectedMetadata,
    'Compiler provenance mismatch'
  );
  const expectedPages = hasFullRoutes
    ? [
        'about.html',
        'index.html',
        'messages.html',
        'privacy.html',
        'search.html',
        'sell.html',
        'selling.html'
      ]
    : hasSearch
      ? ['index.html', 'search.html']
      : ['index.html'];
  equal(
    [...pages.keys()].sort(),
    expectedPages,
    'Undeclared prerendered pages'
  );
  for (const name of expectedPages) admitted.set(name, pages.get(name));
  // Adapter fallback has no separate disk input. Reconstruct the exact current
  // compiler bootstrap shell from the actual index/manifest/version and source
  // template; future framework/resource shapes require scoped tested admission.
  const index = publicText(pages.get('index.html'));
  const id = /(__sveltekit_[a-z0-9]+) = \{/.exec(index)?.[1];
  if (!id) throw new Error('Missing compiler bootstrap identity');
  const start = Object.values(manifest).find(
    (record) => record.name === 'entry/start'
  ).file;
  const app = Object.values(manifest).find(
    (record) => record.name === 'entry/app'
  ).file;
  const fallbackModules = new Set();
  const moduleVisited = new Set();
  /** @param {string} key */
  function collectModules(key) {
    if (moduleVisited.has(key)) return;
    moduleVisited.add(key);
    const record = manifest[key];
    fallbackModules.add(record.file);
    for (const dependency of record.imports ?? []) collectModules(dependency);
  }
  for (const [key, record] of Object.entries(manifest))
    if (
      ['entry/start', 'entry/app', 'nodes/0', 'client-entry'].includes(
        record.name
      )
    )
      collectModules(key);
  const preload = [
    ...index.matchAll(/<link href="\.\/([^"<>]+)" rel="modulepreload">/g)
  ]
    .map((match) => match[1])
    .filter((file) => fallbackModules.has(file));
  if (!preload.length || preload.some((file) => !admitted.has(file)))
    throw new Error('Invalid compiler preloads');
  const fallbackCSS = new Set();
  const cssVisited = new Set();
  /** @param {string} key */
  function collectCSS(key) {
    if (cssVisited.has(key)) return;
    cssVisited.add(key);
    const record = manifest[key];
    for (const file of record.css ?? []) fallbackCSS.add(file);
    for (const dependency of record.imports ?? []) collectCSS(dependency);
  }
  for (const [key, record] of Object.entries(manifest))
    if (['entry/start', 'entry/app', 'nodes/0'].includes(record.name))
      collectCSS(key);
  const stylesheetLinks = [
    ...index.matchAll(/<link href="\.\/([^"<>]+)" rel="stylesheet">/g)
  ]
    .map((match) => match[1])
    .filter((file) => fallbackCSS.has(file));
  equal(
    [...stylesheetLinks].sort(),
    [...fallbackCSS].sort(),
    'Missing owned fallback stylesheet'
  );
  const head =
    preload
      .map((file) => `<link href="/${file}" rel="modulepreload">`)
      .join('\n\t\t') +
    '\n\t\t' +
    (stylesheetLinks.length ? '\n\t\t' : '') +
    stylesheetLinks
      .map((file) => `<link href="/${file}" rel="stylesheet">`)
      .join('\n\t\t');
  const body = `\n\t\t\t<script>\n\t\t\t\t{\n\t\t\t\t\t${id} = {\n\t\t\t\t\t\tbase: "",\n\t\t\t\t\t\tversion: ${JSON.stringify(version.version)}\n\t\t\t\t\t};\n\n\t\t\t\t\tconst element = document.currentScript.parentElement;\n\n\t\t\t\t\timport("/${start}").then(async (kit) => {\n\t\t\t\t\t\tkit.init(${id});\n\t\t\t\t\t\tconst app = await import("/${app}");\n\t\t\t\t\t\tkit.start(app, element);\n\t\t\t\t\t});\n\t\t\t\t}\n\t\t\t</script>\n\t\t`;
  const template = publicText(await readOwned(web, 'src/app.html'));
  admitted.set(
    '200.html',
    Buffer.from(
      template
        .replace('%sveltekit.head%', head)
        .replace('%sveltekit.body%', body)
    )
  );
  equal(
    [...output.keys()].sort(),
    [...admitted.keys()].sort(),
    'Static output inventory mismatch'
  );
  for (const [file, expected] of admitted) {
    const bytes = output.get(file);
    publicText(bytes);
    equal(bytes, expected, 'Static output differs from owned compiler input');
  }
  return [...output.keys()].sort();
}
if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const web = fileURLToPath(new URL('../', import.meta.url));
  if ((await realpath(process.cwd())) !== (await realpath(web)))
    throw new Error('Run the output audit from web/');
  const files = await auditOutput(web);
  console.log(`Static output boundary: ${files.length} owned files checked`);
}
