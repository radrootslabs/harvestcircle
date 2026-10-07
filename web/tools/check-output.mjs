import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import { constants } from 'node:fs';
import { lstat, open, opendir, realpath } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { deriveBuildInfo } from './build-info.mjs';
import ts from 'typescript';
import { parse as parseSvelte } from 'svelte/compiler';

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
    const buffer = Buffer.alloc(64 * 1024);
    for (;;) {
      const { bytesRead } = await handle.read(buffer);
      if (!bytesRead) break;
      size += bytesRead;
      if (size > maxBytes) throw new Error('Oversized output entry');
      chunks.push(Buffer.from(buffer.subarray(0, bytesRead)));
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
const sdkNegentropy =
  'node_modules/.pnpm/applesauce-relay@6.2.1_typescript@6.0.3/node_modules/applesauce-relay/dist/negentropy.js';
// Compiler admission follows actual root bindings, never copied files or text
// matches. This is the deliberately fixed lifecycle shape owned by HCP032.
/** @param {string} web */
async function hasOwnedPublicRuntime(web) {
  const layout = publicText(await readOwned(web, 'src/routes/+layout.svelte'));
  const component = parseSvelte(layout);
  if (!component.instance) return false;
  const script = layout.slice(
    component.instance.content.start,
    component.instance.content.end
  );
  const source = ts.createSourceFile(
    'root.ts',
    script,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS
  );
  const runtimePath = '../lib/runtime/public-runtime.ts';
  const imports = source.statements.filter(ts.isImportDeclaration);
  const runtimeImports = imports.filter(
    (node) =>
      ts.isStringLiteral(node.moduleSpecifier) &&
      node.moduleSpecifier.text === runtimePath
  );
  if (!runtimeImports.length) return false;
  /** @returns {never} */
  function fail() {
    throw new Error('Invalid owned root runtime activation');
  }
  if (runtimeImports.length !== 1 || component.module) fail();
  const required = new Map([
    [
      runtimePath,
      [
        'createPublicRuntimeContext',
        'mountPublicRuntime',
        'closePublicRuntime',
        'PUBLIC_RUNTIME_CONTEXT'
      ]
    ],
    ['svelte', ['onMount', 'setContext']]
  ]);
  const identityPath = '../lib/runtime/view-context.ts';
  const hasIdentity = imports.some(
    (node) =>
      ts.isStringLiteral(node.moduleSpecifier) &&
      node.moduleSpecifier.text === identityPath
  );
  if (hasIdentity)
    required.set(identityPath, [
      'createIdentityViewContext',
      'IDENTITY_VIEW_CONTEXT',
      'identityViewSnapshot',
      'mountIdentityView',
      'subscribeIdentityView',
      'connectIdentityView',
      'disconnectIdentityView',
      'closeIdentityView',
      'invalidateIdentityView'
    ]);
  const allowedImports = new Set();
  for (const [module, names] of required) {
    const declarations = imports.filter(
      (node) =>
        ts.isStringLiteral(node.moduleSpecifier) &&
        node.moduleSpecifier.text === module
    );
    if (declarations.length !== 1) fail();
    const clause = declarations[0].importClause;
    if (
      !clause ||
      clause.isTypeOnly ||
      clause.name ||
      !clause.namedBindings ||
      !ts.isNamedImports(clause.namedBindings)
    )
      fail();
    const elements = clause.namedBindings.elements;
    for (const name of names) {
      const bindings = elements.filter((node) => node.name.text === name);
      if (
        bindings.length !== 1 ||
        bindings[0].propertyName ||
        bindings[0].isTypeOnly
      )
        fail();
    }
    if (
      (module === runtimePath || module === identityPath) &&
      elements.length !== names.length
    )
      fail();
    allowedImports.add(declarations[0]);
  }
  const printer = ts.createPrinter({ removeComments: true });
  /** @param {import("typescript").Node} node @param {import("typescript").SourceFile} file */
  const print = (node, file) =>
    printer.printNode(ts.EmitHint.Unspecified, node, file);
  const expected = ts.createSourceFile(
    'expected.ts',
    `
    const publicContext = createPublicRuntimeContext();
    setContext(PUBLIC_RUNTIME_CONTEXT, publicContext);
    onMount(() => {
      mountPublicRuntime(publicContext);
      return () => closePublicRuntime(publicContext);
    });`,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS
  );
  const shapes = expected.statements.map((node) => print(node, expected));
  const lifecycle = source.statements.filter((node) =>
    shapes.includes(print(node, source))
  );
  if (
    lifecycle.length !== shapes.length ||
    lifecycle.some((node, i) => print(node, source) !== shapes[i])
  )
    fail();
  // HCP056 adds one separately owned identity lifecycle. The canonical public
  // activation above remains mandatory, unchanged and independently ordered.
  if (hasIdentity) {
    const identityExpected = ts.createSourceFile(
      'identity-expected.ts',
      `
  const identityContext = createIdentityViewContext();
  setContext(IDENTITY_VIEW_CONTEXT, identityContext);
  let identityView = $state(identityViewSnapshot(identityContext));
  $effect(() => {
    if (page.url.pathname) invalidateIdentityView(identityContext);
  });
  function connect() {
    return connectIdentityView(identityContext);
  }
  function disconnect() {
    disconnectIdentityView(identityContext);
  }
  const shellIdentity = $derived(
    identityView.identity.state === 'guest' ||
      identityView.identity.state === 'pending'
      ? {
          kind: 'guest' as const,
          onconnect: identityView.mounted ? connect : undefined
        }
      : {
          kind: 'connected' as const,
          publicKey: identityView.identity.publicKey,
          ondisconnect: disconnect
        }
  );

  onMount(() => {
    const off = subscribeIdentityView(identityContext, (next) => {
      identityView = next;
    });
    mountIdentityView(identityContext);
    return () => {
      off();
      closeIdentityView(identityContext);
    };
  });
`,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TS
    );
    const identityShapes = identityExpected.statements.map((node) =>
      print(node, identityExpected)
    );
    const identityLifecycle = source.statements.filter((node) =>
      identityShapes.includes(print(node, source))
    );
    if (
      identityLifecycle.length !== identityShapes.length ||
      identityLifecycle.some(
        (node, i) => print(node, source) !== identityShapes[i]
      )
    )
      fail();
    lifecycle.push(...identityLifecycle);
    publicText(await readOwned(web, 'src/lib/runtime/view-context.ts'));
    publicText(await readOwned(web, 'src/lib/runtime/identity-session.ts'));
  }
  // Reject duplicate, shadowed, reassigned, or extra uses of these bindings in
  // any other statement, including nested closures. Canonical callbacks have
  // no parameters and therefore cannot hide a second context binding.
  const bindings = new Set([
    'publicContext',
    ...[...required.values()].flat(),
    ...(hasIdentity
      ? [
          'identityContext',
          'identityView',
          'connect',
          'disconnect',
          'shellIdentity'
        ]
      : [])
  ]);
  for (const statement of source.statements) {
    if (allowedImports.has(statement) || lifecycle.includes(statement))
      continue;
    /** @param {import("typescript").Node} node */
    const visit = (node) => {
      if (ts.isIdentifier(node) && bindings.has(node.text)) fail();
      ts.forEachChild(node, visit);
    };
    visit(statement);
  }
  for (const name of [
    'runtime/public-runtime.ts',
    'nostr/request-scope.ts',
    'nostr/public-pool.ts',
    'nostr/public-store.ts',
    'nostr/request-result.ts',
    'nostr/ingress.ts',
    'nostr/verified-envelope.ts',
    'nostr/envelope-bounds.ts',
    'nostr/exports.ts',
    'catalog/observations.ts',
    'config/budgets.ts',
    'config/relays.ts',
    'config/deployment-relays.ts'
  ])
    publicText(await readOwned(web, 'src/lib/' + name));
  return hasIdentity ? 'public_identity' : 'public';
}
/** @param {string} web */
async function productPresentationAdmission(web) {
  const route = publicText(
    await readOwned(web, 'src/routes/products/[naddr=naddr]/+page.svelte')
  );
  if (!route.includes('../../../lib/catalog/product-view.ts')) return false;
  const producers = {
    'src/routes/products/[naddr=naddr]/+page.svelte':
      '1869d4dc32ea794d5ee5c1d6cc6b4aac94483482d18cff0562c0cb10087b5278',
    'src/lib/catalog/product-view.ts':
      'e3739170e3c700246a80c1358c7b96541a5cbf1e4c26905e6ca60537adfce9cf',
    'src/lib/components/ListingFacts.svelte':
      '65d009b4940e7ced61347919689877da9473a2a66a44f61cc5e7bd0b2d217215',
    'src/lib/components/PublisherIdentity.svelte':
      '90b156e2401d40079ee559d65ce66ec09ecc4246bc80cfdfc5def0e21e8ce9ff',
    'src/lib/contracts/food-availability-v1/contact-read.ts':
      '18f812812c15858768223d929bcea9d7eae5154612747016ccd17864c229420a',
    'src/lib/navigation-copy.ts':
      '1a17eac4b355d8a3fb2b55b3fe1bc856cc3b3bce07434836d708b68407686f20',
    'src/lib/runtime/public-runtime.ts':
      'fa2e27d72b82b6bef73693cf93f19614e61ea51ebd11eb40d8ca4ec723f0c14e',
    'src/lib/catalog/publishers.ts':
      'a6795a35ebbf97246e22235df1ca868432e2cccfb26a38f0ebdf4524911192e6',
    'src/lib/catalog/resolve-head.ts':
      '4f1c9f144164580da038d127ab463f9f8ba6b20e882fc31151cb29850782d633'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    if (createHash('sha256').update(bytes).digest('hex') !== pin)
      throw new Error('Invalid owned product presentation source');
  }
  return true;
}
// This exact reviewed producer set owns the shared presentation/runtime split.
// A successor changes these identities only with new compiler qualification.
/** @param {string} web */
async function searchPresentationAdmission(web) {
  const route = publicText(
    await readOwned(web, 'src/routes/search/+page.svelte')
  );
  if (!route.includes('../../lib/catalog/search-view.ts')) return false;
  const producers = {
    'src/routes/search/+page.svelte':
      '0dea04fc5a5a41599774215c9157d8eb2076eb3fa4fe6355184a9a467a9cc72a',
    'src/lib/catalog/search-view.ts':
      '398cc2db59fb78d33df8cb8ce59d4e10a84feb3842709f2a7047033424798768',
    'src/lib/components/ListingRow.svelte':
      '48c6ab9b5fe887206b77a27bd0de697d346133ecb44d8ba973c8ffacc0cf5f03',
    'src/lib/components/SourceStatus.svelte':
      'd3cdb173fdeb4e7783f4b3b909a90c32ae4a66a5d87cba6a2766e718317775de',
    'src/lib/navigation-scroll.ts':
      '9a3dcd9bbde8bcdb18d8962816fa062262505bc3e8ad34d88c78d18dba3a8398',
    'src/lib/components/primitives/Disclosure.svelte':
      'c6d8361f31218644ac487a3d43412627cb8cc50906a921b95cf053047ce32bf3'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    if (createHash('sha256').update(bytes).digest('hex') !== pin)
      throw new Error('Invalid owned search presentation source');
  }
  return true;
}
// HCP066 exact owned preference port changes the shared compiler chunk name.
// Preserve the full module/path/output rejection policy; no arbitrary admission.
/** @param {string} web */
async function preferencePublicationAdmission(web) {
  const pool = await readOwned(web, 'src/lib/nostr/public-pool.ts');
  if (!pool.toString().includes("'./inbox-preference-publication.ts'"))
    return false;
  const producers = {
    'src/lib/nostr/public-pool.ts':
      'd5054acecd074a36e68d39e1f69c86cdde6480339c7f4bf901ece151a2b6a5b7',
    'src/lib/nostr/inbox-preference-publication.ts':
      'fbad642340732bf7cf92905010af314829b80cc7b134121396f14e9bb8e765cc'
  };
  for (const [name, pin] of Object.entries(producers)) {
    const bytes = await readOwned(web, name);
    publicText(bytes);
    if (createHash('sha256').update(bytes).digest('hex') !== pin)
      throw new Error('Invalid owned preference publication source');
  }
  return true;
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
  const hasPublicRuntime = await hasOwnedPublicRuntime(web);
  if (hasPublicRuntime && !hasFullRoutes)
    throw new Error('Incomplete owned root runtime routes');
  const hasSearchPresentation =
    hasSearch && hasPublicRuntime && (await searchPresentationAdmission(web));
  const hasProductPresentation =
    hasFullRoutes &&
    hasPublicRuntime &&
    (await productPresentationAdmission(web));
  const hasPreferencePublication =
    hasPublicRuntime && (await preferencePublicationAdmission(web));
  const identityCompilerModule = hasPreferencePublication
    ? 'identity-session'
    : 'heads';
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
      ...(hasPublicRuntime === 'public_identity' ? [] : ['rolldown-runtime']),
      'shared-errors'
    ])
      names.add(name);
  }
  if (hasPublicRuntime)
    for (const name of [
      'public-key',
      ...(hasSearchPresentation ? ['Disclosure', 'navigation'] : ['dist']),
      'negentropy',
      ...(hasPublicRuntime === 'public_identity'
        ? ['view-context', identityCompilerModule]
        : ['budgets'])
    ])
      names.add(name);
  if (hasProductPresentation) names.add('publishers');
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
      (hasPublicRuntime && key === sdkNegentropy) ||
      generatedModule.test(key) ||
      frameworkModule.test(key) ||
      /^_[A-Za-z0-9_-]+\.js$/.test(key)
    ))
      throw new Error('Undeclared compiler input');
    if (
      record.src !== undefined &&
      (record.src !== key ||
        !(
          generatedModule.test(key) ||
          frameworkModule.test(key) ||
          (hasPublicRuntime && key === sdkNegentropy)
        ))
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
  if (hasPublicRuntime) {
    const records = Object.entries(manifest);
    /** @param {string} name */
    function requiredRecord(name) {
      const entry = records.find(([, record]) => record.name === name);
      if (!entry) throw new Error('Incomplete owned SDK compiler module');
      return entry;
    }
    const dist = requiredRecord(hasSearchPresentation ? 'Disclosure' : 'dist');
    const root = requiredRecord('nodes/0')[1];
    const [budgetKey, budget] = requiredRecord(
      hasPublicRuntime === 'public_identity' ? 'Button' : 'budgets'
    );
    if (
      !/^_[A-Za-z0-9_-]+\.js$/.test(budgetKey) ||
      Object.keys(budget).some(
        (field) =>
          !(
            hasPublicRuntime === 'public_identity'
              ? ['file', 'name', 'imports']
              : ['file', 'name']
          ).includes(field)
      )
    )
      throw new Error('Invalid owned budgets compiler identity');
    if (hasPublicRuntime === 'public_identity')
      equal(
        budget.imports,
        [requiredRecord('client')[0]],
        'Invalid owned budgets compiler dependencies'
      );
    for (const consumer of hasSearchPresentation
      ? [
          'Disclosure',
          'nodes/9',
          'routes',
          ...(hasProductPresentation ? ['nodes/7', 'publishers'] : [])
        ]
      : ['nodes/0', 'routes']) {
      if (!requiredRecord(consumer)[1].imports?.includes(budgetKey))
        throw new Error('Invalid owned budgets compiler edge');
    }
    const sdk = manifest[sdkNegentropy];
    if (
      !sdk ||
      sdk.name !== 'negentropy' ||
      sdk.src !== sdkNegentropy ||
      sdk.isDynamicEntry !== true ||
      sdk.isEntry !== undefined ||
      sdk.css !== undefined ||
      sdk.dynamicImports !== undefined
    )
      throw new Error('Invalid owned SDK compiler identity');
    equal(
      sdk.imports,
      hasPublicRuntime === 'public_identity'
        ? [requiredRecord(identityCompilerModule)[0], dist[0]]
        : [dist[0]],
      'Invalid owned SDK compiler dependency'
    );
    equal(
      hasSearchPresentation ? dist[1].dynamicImports : root.dynamicImports,
      [sdkNegentropy],
      'Invalid owned root compiler edge'
    );
    if (hasSearchPresentation) {
      if (
        root.dynamicImports !== undefined ||
        !root.imports?.includes(dist[0]) ||
        !requiredRecord('nodes/9')[1].imports?.includes(dist[0])
      )
        throw new Error('Invalid owned shared compiler edge');
      const navigation = requiredRecord('navigation');
      if (!requiredRecord('nodes/9')[1].imports?.includes(navigation[0]))
        throw new Error('Invalid owned navigation compiler edge');
      equal(
        navigation[1].imports,
        ['entry/payload', 'shared-errors', 'client', 'client.svelte'].map(
          (name) => requiredRecord(name)[0]
        ),
        'Invalid owned navigation compiler dependencies'
      );
      if (navigation[1].dynamicImports !== undefined)
        throw new Error('Invalid owned navigation compiler dynamic imports');
      equal(
        dist[1].imports,
        (hasPublicRuntime === 'public_identity'
          ? [
              'public-key',
              'preload-helper',
              identityCompilerModule,
              'client',
              'Button'
            ]
          : [
              'rolldown-runtime',
              'preload-helper',
              'public-key',
              'client',
              'budgets'
            ]
        ).map((name) => requiredRecord(name)[0]),
        'Invalid owned shared compiler dependencies'
      );
      for (const [key, record] of [dist, navigation]) {
        if (
          !/^_[A-Za-z0-9_-]+\.js$/.test(key) ||
          record.src !== undefined ||
          record.isEntry !== undefined ||
          record.isDynamicEntry !== undefined ||
          record.css !== undefined
        )
          throw new Error('Invalid owned search compiler chunk');
      }
    }
    if (hasPublicRuntime === 'public_identity') {
      /** @type {[string, string[]][]} */
      const identityDependencies = [
        [
          'view-context',
          [
            ...(hasPreferencePublication ? [] : ['public-key']),
            identityCompilerModule,
            'client',
            'Button'
          ]
        ],
        [identityCompilerModule, ['public-key', 'Button']]
      ];
      for (const [name, dependencies] of identityDependencies) {
        const [key, record] = requiredRecord(name);
        if (
          !/^_[A-Za-z0-9_-]+\.js$/.test(key) ||
          Object.keys(record).some(
            (field) => !['file', 'name', 'imports'].includes(field)
          )
        )
          throw new Error('Invalid owned identity compiler chunk');
        equal(
          record.imports,
          dependencies.map((dependency) => requiredRecord(dependency)[0]),
          'Invalid owned identity compiler dependencies'
        );
      }
      const [identityKey] = requiredRecord('view-context');
      if (
        !root.imports?.includes(identityKey) ||
        !requiredRecord('AccountGate')[1].imports?.includes(identityKey)
      )
        throw new Error('Invalid owned identity compiler consumer');
    }
    for (const name of hasSearchPresentation
      ? ['public-key']
      : ['dist', 'public-key']) {
      const [key, record] = requiredRecord(name);
      if (
        !/^_[A-Za-z0-9_-]+\.js$/.test(key) ||
        record.src !== undefined ||
        record.isEntry !== undefined ||
        record.isDynamicEntry !== undefined ||
        record.css !== undefined
      )
        throw new Error('Invalid owned SDK compiler chunk');
    }
  }
  if (hasProductPresentation) {
    const records = Object.entries(manifest);
    const entry = (/** @type {string} */ name) => {
      const row = records.find(([, r]) => r.name === name);
      if (!row) throw new Error('Missing product compiler module');
      return row;
    };
    const [publisherKey, publisher] = entry('publishers');
    if (
      !/^_[A-Za-z0-9_-]+\.js$/.test(publisherKey) ||
      Object.keys(publisher).some(
        (field) => !['file', 'name', 'imports'].includes(field)
      )
    )
      throw new Error('Invalid owned product compiler identity');
    equal(
      publisher.imports,
      (hasPublicRuntime === 'public_identity'
        ? ['public-key', identityCompilerModule, 'Disclosure', 'Button']
        : ['public-key', 'Disclosure', 'budgets']
      ).map((name) => entry(name)[0]),
      'Invalid owned product publisher compiler dependencies'
    );
    equal(
      entry('nodes/7')[1].imports,
      (hasPublicRuntime === 'public_identity'
        ? [
            'public-key',
            'references',
            identityCompilerModule,
            'Disclosure',
            'client',
            'Button',
            'client.svelte',
            'state',
            'publishers'
          ]
        : [
            'rolldown-runtime',
            'references',
            'Disclosure',
            'client',
            'budgets',
            'client.svelte',
            'state',
            'Button',
            'publishers'
          ]
      ).map((name) => entry(name)[0]),
      'Invalid owned product compiler dependencies'
    );
    equal(
      entry('nodes/9')[1].imports,
      (hasPublicRuntime === 'public_identity'
        ? [
            'references',
            identityCompilerModule,
            'Disclosure',
            'client',
            'Button',
            'client.svelte',
            'navigation',
            'state',
            'routes',
            'publishers'
          ]
        : [
            'references',
            'Disclosure',
            'client',
            'budgets',
            'client.svelte',
            'navigation',
            'state',
            'Button',
            'routes',
            'publishers'
          ]
      ).map((name) => entry(name)[0]),
      'Invalid owned search product compiler dependencies'
    );
    if (
      entry('nodes/7')[1].dynamicImports !== undefined ||
      entry('nodes/9')[1].dynamicImports !== undefined
    )
      throw new Error('Invalid owned product compiler dynamic imports');
  }
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
