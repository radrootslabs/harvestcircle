import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  cp,
  link,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  realpath,
  rm,
  symlink,
  writeFile
} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { generateBuildInfo } from '../../tools/build-info.mjs';
import { auditOutput } from '../../tools/check-output.mjs';

// Allow bounded setup/copy time for 123 checks and two real compilations.
// Admission checks get 10 seconds; cold compiler work gets 60 seconds.
await test('actual output qualification', { timeout: 180000 }, async (t) => {
  /** @param {string} name @param {(context: import('node:test').TestContext) => Promise<void>} run @param {number} [timeout] */
  const check = (name, run, timeout = 10000) => t.test(name, { timeout }, run);
  const source = fileURLToPath(new URL('../../../', import.meta.url));
  // Exact HCP043 product shell bytes, immutable compatibility evidence only.
  const historicalProduct =
    '<svelte:head><title>Food — HarvestCircle</title></svelte:head>\n<div class="page page--reading stack">\n  <h1>Food</h1>\n  <p class="notice">Food details are unavailable during development.</p>\n</div>\n';
  // Frozen public-only HCP055 producers, source6023db01169aa82cb6fe68ca94e1a73750247e72.
  // Compatibility evidence only; runtime tests never require ancestor Git history.
  const historicalPublicOnly = {
    'src/routes/+layout.svelte': {
      source:
        "<script lang=\"ts\">\n  import '../theme.css';\n  import '../app.css';\n  import { onMount, setContext, type Snippet } from 'svelte';\n  import {\n    createPublicRuntimeContext,\n    mountPublicRuntime,\n    closePublicRuntime,\n    PUBLIC_RUNTIME_CONTEXT\n  } from '../lib/runtime/public-runtime.ts';\n  import { page } from '$app/state';\n  import AppShell from '../lib/components/AppShell.svelte';\n\n  const publicContext = createPublicRuntimeContext();\n  setContext(PUBLIC_RUNTIME_CONTEXT, publicContext);\n  onMount(() => {\n    mountPublicRuntime(publicContext);\n    return () => closePublicRuntime(publicContext);\n  });\n\n  let { children }: { children: Snippet } = $props();\n</script>\n\n<AppShell\n  currentPath={page.url.pathname}\n  availableRoutes={[\n    '/search',\n    '/sell',\n    '/selling',\n    '/messages',\n    '/about',\n    '/privacy'\n  ]}\n>\n  {@render children()}\n</AppShell>\n",
      sha256: '956da247f23ad9695c3c6f8bff42801aceaa5b84a2606a1a8a5263fd0c637dc7'
    },
    'src/lib/components/AppShell.svelte': {
      source:
        "<script lang=\"ts\">\n  import type { Snippet } from 'svelte';\n  import { internalHref } from '../navigation-url';\n  import Button from './primitives/Button.svelte';\n  import Disclosure from './primitives/Disclosure.svelte';\n\n  type ShellRoute =\n    '/search' | '/sell' | '/selling' | '/messages' | '/about' | '/privacy';\n  type Identity =\n    | { kind: 'guest'; onconnect?: () => void }\n    | { kind: 'connected'; publicKey: string; ondisconnect?: () => void };\n  let {\n    children,\n    currentPath = '/',\n    availableRoutes = [],\n    identity = { kind: 'guest' }\n  }: {\n    children: Snippet;\n    currentPath?: string;\n    availableRoutes?: readonly ShellRoute[];\n    identity?: Identity;\n  } = $props();\n\n  const unavailable = $derived(\n    !availableRoutes.includes('/search') ||\n      !availableRoutes.includes('/about') ||\n      !availableRoutes.includes('/privacy') ||\n      (identity.kind === 'guest'\n        ? !availableRoutes.includes('/sell') || !identity.onconnect\n        : !availableRoutes.includes('/selling') ||\n          !availableRoutes.includes('/messages') ||\n          !identity.ondisconnect)\n  );\n\n  function available(href: string) {\n    return (\n      href === '/' ||\n      availableRoutes.some((route) => route === href.split('#')[0])\n    );\n  }\n</script>\n\n{#snippet navigation(label: string, href: string, brand = false)}\n  {#if available(href) && internalHref(href)}\n    <a\n      href={internalHref(href)}\n      class=\"shell-link\"\n      class:brand\n      aria-current={currentPath === href ? 'page' : undefined}>{label}</a\n    >\n  {:else}\n    <span\n      class=\"shell-link text-muted\"\n      aria-disabled=\"true\"\n      aria-describedby=\"shell-availability\"\n      aria-current={currentPath === href ? 'page' : undefined}>{label}</span\n    >\n  {/if}\n{/snippet}\n\n<a href={internalHref('#main-content')} class=\"skip-link visually-hidden\"\n  >Skip to main content</a\n>\n<header class=\"navbar\">\n  <nav aria-label=\"Primary\" class=\"page navbar__inner\">\n    <div class=\"cluster\">\n      {@render navigation('HarvestCircle', '/', true)}\n      {@render navigation('Search', '/search')}\n      {#if identity.kind === 'guest'}\n        {@render navigation('List food', '/sell')}\n        <Button\n          label=\"Connect extension\"\n          disabled={!identity.onconnect}\n          onclick={identity.onconnect}\n        />\n      {:else}\n        {@render navigation('Messages', '/messages')}\n        {@render navigation('Selling', '/selling')}\n        <Disclosure summary=\"Identity\">\n          <p class=\"key\">{identity.publicKey}</p>\n          <Button\n            label=\"Disconnect\"\n            disabled={!identity.ondisconnect}\n            onclick={identity.ondisconnect}\n          />\n        </Disclosure>\n      {/if}\n    </div>\n    {#if unavailable}\n      <p id=\"shell-availability\" class=\"text-small text-muted\">\n        Unavailable during development: disabled navigation and actions.\n      </p>\n    {/if}\n  </nav>\n</header>\n<main id=\"main-content\" tabindex=\"-1\" class=\"page\">\n  {@render children()}\n</main>\n<footer class=\"footer\">\n  <nav aria-label=\"Footer\" class=\"page cluster\">\n    {@render navigation('About', '/about')}\n    {@render navigation('Privacy', '/privacy')}\n    {@render navigation('Help / report', '/about#help')}\n  </nav>\n</footer>\n",
      sha256: 'c8afbb1ee62a16ad47558829518b347fe3e2a312e490312ded812a8b538d828f'
    },
    'src/lib/components/AccountGate.svelte': {
      source:
        '<script lang="ts">\n  import Button from \'./primitives/Button.svelte\';\n</script>\n\n<svelte:head>\n  <title>HarvestCircle</title>\n  <meta name="robots" content="noindex" />\n</svelte:head>\n<div class="page page--reading stack">\n  <h1>Connect or unlock</h1>\n  <p>Connect an extension and unlock your account to continue.</p>\n  <p class="notice">Account access is unavailable during development.</p>\n  <div class="cluster">\n    <Button label="Connect extension" disabled />\n    <Button label="Unlock" disabled />\n  </div>\n</div>\n',
      sha256: 'd5f90fb4b9af50037ed19dde626552e82df67017a39408bdbdeff1e712c10fbd'
    }
  };
  const base = await mkdtemp(path.join(os.tmpdir(), 'hc actual output '));
  t.after(() => rm(base, { recursive: true, force: true }));
  /** @param {string} root @param {string[]} args */
  const git = (root, ...args) =>
    execFileSync('git', ['-C', root, ...args], {
      env: Object.fromEntries(
        Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_'))
      ),
      encoding: 'utf8'
    });
  // Compile actual current source once in isolation. No install/native subprocess.
  for (const name of git(
    source,
    'ls-files',
    '--cached',
    '--others',
    '--exclude-standard',
    '-z',
    '--',
    'web',
    '.gitignore',
    'core/Cargo.lock',
    'radroots.lib.source-lock.v1.toml'
  )
    .split('\0')
    .filter(Boolean)) {
    const stat = await lstat(path.join(source, name));
    assert.ok(
      stat.isFile() && stat.nlink === 1 && stat.size <= 8 * 1024 * 1024,
      'Only safe actual source fixture inputs'
    );
    await mkdir(path.dirname(path.join(base, name)), { recursive: true });
    await writeFile(
      path.join(base, name),
      await readFile(path.join(source, name))
    );
  }
  git(base, 'init', '-q');
  git(base, 'add', '.');
  git(
    base,
    '-c',
    'user.name=Output Test',
    '-c',
    'user.email=output@example.invalid',
    'commit',
    '-qm',
    'actual inputs'
  );
  await cp(
    path.join(source, 'web/node_modules'),
    path.join(base, 'web/node_modules'),
    { recursive: true, verbatimSymlinks: true }
  );
  await generateBuildInfo(path.join(base, 'web'));
  execFileSync(
    process.execPath,
    [path.join(source, 'web/node_modules/vite/bin/vite.js'), 'build'],
    { cwd: path.join(base, 'web'), stdio: 'pipe', timeout: 60000 }
  );

  /** @param {import('node:test').TestContext} t */
  async function fixture(t) {
    const root = await realpath(
      await mkdtemp(path.join(os.tmpdir(), 'hc output mutation '))
    );
    t.after(() => rm(root, { recursive: true, force: true }));
    // Audit consumers use actual client/prerendered/build bytes and all source
    // inputs. Server output and generated development/type caches are not audit
    // inputs; avoid copying them for every mutation under the fixed deadline.
    const excluded = new Set([
      path.join(base, 'web/node_modules'),
      path.join(base, 'web/.svelte-kit/output/server'),
      path.join(base, 'web/.svelte-kit/generated'),
      path.join(base, 'web/.svelte-kit/types')
    ]);
    await cp(base, root, {
      recursive: true,
      filter: (name) => !excluded.has(name)
    });
    const web = path.join(root, 'web');
    return { root, web, audit: () => auditOutput(web) };
  }
  await check(
    'actual compiled payload passes and build dispatch invokes the guard last',
    async (t) => {
      const f = await fixture(t);
      const files = await f.audit();
      // Exact eleven-route module/page/static admission; CSS stays once-imported.
      assert.equal(files.length, 47);
      // The actual SDK payload crosses the reader scratch boundary; a reused
      // scratch buffer must still preserve every compiler/static byte exactly.
      const sizes = await Promise.all(
        files
          .filter((name) => name.endsWith('.js'))
          .map(
            async (name) => (await lstat(path.join(f.web, 'build', name))).size
          )
      );
      assert.ok(sizes.some((bytes) => bytes > 64 * 1024));
      assert.ok(files.includes('search.html'));
      assert.equal(files.filter((name) => name.endsWith('.css')).length, 1);
      assert.ok(
        files.some((name) =>
          /^_app\/immutable\/assets\/[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.css$/.test(
            name
          )
        )
      );
      assert.ok(files.includes('200.html'));
      const pkg = JSON.parse(
        await readFile(path.join(f.web, 'package.json'), 'utf8')
      );
      assert.match(
        pkg.scripts.build,
        /vite build && node tools\/check-output\.mjs$/
      );
    }
  );
  await check(
    'public-only owned lifecycle remains independently compilable',
    async (t) => {
      const f = await fixture(t);
      for (const [name, value] of Object.entries(historicalPublicOnly)) {
        assert.equal(
          createHash('sha256').update(value.source).digest('hex'),
          value.sha256
        );
        await writeFile(path.join(f.web, name), value.source);
      }
      await cp(
        path.join(base, 'web/node_modules'),
        path.join(f.web, 'node_modules'),
        { recursive: true, verbatimSymlinks: true }
      );
      await refresh(f);
      execFileSync(
        process.execPath,
        [path.join(source, 'web/node_modules/vite/bin/vite.js'), 'build'],
        { cwd: f.web, stdio: 'pipe', timeout: 60000 }
      );
      const manifest = JSON.parse(
        await readFile(
          path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
          'utf8'
        )
      );
      assert.ok(
        !Object.values(manifest).some(
          (record) => record.name === 'view-context'
        )
      );
      assert.ok(
        Object.values(manifest).some((record) => record.name === 'budgets')
      );
      assert.equal((await f.audit()).length, 47);
    },
    60000
  );
  /** @param {{root: string, web: string}} f */
  async function refresh(f) {
    git(f.root, 'add', '--', 'web/src');
    await generateBuildInfo(f.web);
    const bytes = await readFile(path.join(f.web, 'static/build-info.json'));
    for (const name of [
      'build/build-info.json',
      '.svelte-kit/output/client/build-info.json'
    ])
      await writeFile(path.join(f.web, name), bytes);
  }
  await check(
    'owned root preserves its lifecycle across a 64 KiB read boundary',
    async (t) => {
      const f = await fixture(t);
      const file = path.join(f.web, 'src/routes/+layout.svelte');
      const layout = await readFile(file, 'utf8');
      // A large harmless comment after the real lifecycle must not overwrite its
      // first read chunk when the file reader reuses a scratch buffer.
      const expanded = layout.replace(
        '</script>',
        '/*' + 'owned reader boundary '.repeat(4000) + '*/\n</script>'
      );
      assert.ok(Buffer.byteLength(expanded) > 64 * 1024);
      await writeFile(file, expanded);
      await refresh(f);
      assert.equal((await f.audit()).length, 47);
    }
  );
  const sdkKey =
    'node_modules/.pnpm/applesauce-relay@6.2.1_typescript@6.0.3/node_modules/applesauce-relay/dist/negentropy.js';
  await check(
    'predecessor SDK graph remains qualified with the historical search form fixture',
    async (t) => {
      const f = await fixture(t);
      // This historical graph predates identity activation. Restore its
      // immutable public-only producers before compiling the former routes.
      for (const [name, value] of Object.entries(historicalPublicOnly))
        await writeFile(path.join(f.web, name), value.source);
      // Immutable former route bytes are a labelled verification variant only.
      const historical =
        "<script lang=\"ts\">\n  import { page } from '$app/state';\n  import SearchForm from '../../lib/components/SearchForm.svelte';\n  import {\n    normalizePublicQuery,\n    queryErrorMessage,\n    readPublicQuery\n  } from '../../lib/catalog/query-input';\n  import type { PublicQuery } from '../../lib/catalog/query-input';\n  import { searchHref } from '../../lib/routes';\n  import { internalHref } from '../../lib/navigation-url';\n  let query = $state<PublicQuery>(normalizePublicQuery(''));\n  let error = $state<string | undefined>();\n  $effect(() => {\n    query = readPublicQuery(page.url);\n    error = undefined;\n  });\n  function search(input: string) {\n    const next = normalizePublicQuery(input);\n    if (!next.ok) {\n      error = queryErrorMessage(next.error);\n      return;\n    }\n    error = undefined;\n    const target = internalHref(searchHref(next.text));\n    if (target === undefined) {\n      error = queryErrorMessage('invalid_query');\n      return;\n    }\n    globalThis.location.assign(target);\n  }\n</script>\n\n<svelte:head><title>Search food \u2014 HarvestCircle</title></svelte:head>\n<div class=\"page page--reading stack\">\n  <h1>Search food</h1>\n  <SearchForm\n    onsubmit={search}\n    initialValue={query.ok ? query.text : ''}\n    error={error ?? (query.ok ? undefined : queryErrorMessage(query.error))}\n  />\n  <p class=\"notice\">Search data is unavailable during development.</p>\n</div>\n";
      await writeFile(
        path.join(f.web, 'src/routes/search/+page.svelte'),
        historical
      );
      await writeFile(
        path.join(f.web, 'src/routes/products/[naddr=naddr]/+page.svelte'),
        historicalProduct
      );
      await refresh(f);
      await cp(
        path.join(base, 'web/node_modules'),
        path.join(f.web, 'node_modules'),
        { recursive: true, verbatimSymlinks: true }
      );
      execFileSync(
        process.execPath,
        [path.join(source, 'web/node_modules/vite/bin/vite.js'), 'build'],
        { cwd: f.web, stdio: 'pipe', timeout: 60000 }
      );
      const manifest = JSON.parse(
        await readFile(
          path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
          'utf8'
        )
      );
      assert.ok(
        Object.values(manifest).some((record) => record.name === 'dist')
      );
      assert.ok(
        !Object.values(manifest).some(
          (record) =>
            record.name === 'Disclosure' || record.name === 'navigation'
        )
      );
      assert.equal((await f.audit()).length, 45);
    },
    60000
  );
  await check(
    'HCP043 search graph remains qualified with the exact historical product shell',
    async (t) => {
      const f = await fixture(t);
      for (const [name, value] of Object.entries(historicalPublicOnly))
        await writeFile(path.join(f.web, name), value.source);
      await writeFile(
        path.join(f.web, 'src/routes/products/[naddr=naddr]/+page.svelte'),
        historicalProduct
      );
      await refresh(f);
      await cp(
        path.join(base, 'web/node_modules'),
        path.join(f.web, 'node_modules'),
        { recursive: true, verbatimSymlinks: true }
      );
      execFileSync(
        process.execPath,
        [path.join(source, 'web/node_modules/vite/bin/vite.js'), 'build'],
        { cwd: f.web, stdio: 'pipe', timeout: 60000 }
      );
      assert.equal((await f.audit()).length, 46);
    },
    60000
  );
  for (const mutation of [
    'unknown-name',
    'missing-public-key',
    'missing-dist',
    'missing-negentropy',
    'missing-root-edge',
    'random-key',
    'wrong-src',
    'wrong-version',
    'wrong-peer',
    'wrong-basename',
    'wrong-dynamic-flag',
    'entry-flag',
    'css',
    'wrong-import',
    'chunk-src',
    'chunk-entry'
  ])
    await check(
      'owned root SDK compiler admission rejects ' + mutation,
      async (t) => {
        const f = await fixture(t);
        const file = path.join(
          f.web,
          '.svelte-kit/output/client/.vite/manifest.json'
        );
        const m = JSON.parse(await readFile(file, 'utf8'));
        const root = Object.values(m).find((v) => v.name === 'nodes/0');
        const dist = Object.entries(m).find(([, v]) => v.name === 'Disclosure');
        assert.ok(m[sdkKey] && root && dist, 'Actual successor SDK identities');
        if (mutation === 'unknown-name') m[sdkKey].name = 'unapproved-sdk';
        if (
          mutation.startsWith('missing-') &&
          mutation !== 'missing-root-edge'
        ) {
          const entry = Object.entries(m).find(
            ([, v]) =>
              v.name ===
              (mutation === 'missing-dist' ? 'Disclosure' : mutation.slice(8))
          );
          assert.ok(entry);
          delete m[entry[0]];
        }
        if (mutation === 'missing-root-edge') dist[1].dynamicImports = [];
        if (
          [
            'random-key',
            'wrong-version',
            'wrong-peer',
            'wrong-basename'
          ].includes(mutation)
        ) {
          const next =
            mutation === 'random-key'
              ? '_HCrandom.js'
              : sdkKey.replace(
                  mutation === 'wrong-version'
                    ? '6.2.1'
                    : mutation === 'wrong-peer'
                      ? '6.0.3'
                      : 'negentropy.js',
                  mutation === 'wrong-basename' ? 'other.js' : '0.0.0'
                );
          m[next] = m[sdkKey];
          delete m[sdkKey];
          m[next].src = next;
          dist[1].dynamicImports = dist[1].dynamicImports.map(
            (/** @type {string} */ k) => (k === sdkKey ? next : k)
          );
        }
        if (mutation === 'wrong-src') m[sdkKey].src = '_HCwrong.js';
        if (mutation === 'wrong-dynamic-flag') m[sdkKey].isDynamicEntry = false;
        if (mutation === 'entry-flag') m[sdkKey].isEntry = false;
        if (mutation === 'css') m[sdkKey].css = [];
        if (mutation === 'wrong-import')
          m[sdkKey].imports = [
            Object.keys(m).find((k) => m[k].name === 'public-key')
          ];
        if (mutation === 'chunk-src') dist[1].src = dist[0];
        if (mutation === 'chunk-entry') dist[1].isEntry = false;
        await writeFile(file, JSON.stringify(m));
        await assert.rejects(f.audit, /compiler|runtime/i);
      }
    );
  for (const mutation of [
    'copied-files-only',
    'comment-only',
    'string-only',
    'shadowed',
    'rebound',
    'aliased',
    'missing-cleanup',
    'missing-context',
    'wrong-context'
  ])
    await check('root runtime activation rejects ' + mutation, async (t) => {
      const f = await fixture(t);
      const file = path.join(f.web, 'src/routes/+layout.svelte');
      let layout = await readFile(file, 'utf8');
      if (
        ['copied-files-only', 'comment-only', 'string-only'].includes(mutation)
      ) {
        const script = layout.slice(
          layout.indexOf('>') + 1,
          layout.indexOf('</script>')
        );
        layout =
          '<script lang="ts">' +
          (mutation === 'comment-only'
            ? '/*' + script + '*/'
            : mutation === 'string-only'
              ? 'const text = ' + JSON.stringify(script) + ';'
              : '') +
          '</script><main>Controlled root</main>';
      }
      if (mutation === 'shadowed')
        layout = layout.replace(
          'onMount(() => {',
          'onMount((publicContext) => {'
        );
      if (mutation === 'rebound')
        layout = layout.replace('const publicContext', 'let publicContext');
      if (mutation === 'aliased')
        layout = layout
          .replace(
            '    mountPublicRuntime,',
            '    mountPublicRuntime as otherMount,'
          )
          .replace(
            'mountPublicRuntime(publicContext)',
            'otherMount(publicContext)'
          );
      if (mutation === 'missing-cleanup')
        layout = layout.replace(
          'return () => closePublicRuntime(publicContext);',
          'return () => {};'
        );
      if (mutation === 'missing-context')
        layout = layout.replace(
          'setContext(PUBLIC_RUNTIME_CONTEXT, publicContext);',
          ''
        );
      if (mutation === 'wrong-context')
        layout = layout.replace(
          'closePublicRuntime(publicContext)',
          'closePublicRuntime({})'
        );
      await writeFile(file, layout);
      await refresh(f);
      await assert.rejects(f.audit, /compiler|runtime/i);
    });
  for (const mutation of [
    'missing-mount',
    'missing-subscription',
    'missing-unsubscribe',
    'missing-close',
    'wrong-context',
    'duplicate-context',
    'extra-mount',
    'aliased-mount',
    'shadowed',
    'missing-context',
    'rebound-context'
  ])
    await check('owned identity lifecycle rejects ' + mutation, async (t) => {
      const f = await fixture(t);
      const file = path.join(f.web, 'src/routes/+layout.svelte');
      const original = await readFile(file, 'utf8');
      let layout = original;
      assert.ok(layout.includes('mountIdentityView(identityContext);'));
      if (mutation === 'missing-mount')
        layout = layout.replace('mountIdentityView(identityContext);', '');
      if (mutation === 'missing-subscription')
        layout = layout.replace(
          'subscribeIdentityView(identityContext,',
          'subscribeIdentityView({},'
        );
      if (mutation === 'missing-unsubscribe')
        layout = layout.replace('      off();', '');
      if (mutation === 'missing-close')
        layout = layout.replace('closeIdentityView(identityContext);', '');
      if (mutation === 'wrong-context')
        layout = layout.replace(
          'closeIdentityView(identityContext)',
          'closeIdentityView({})'
        );
      if (mutation === 'duplicate-context')
        layout = layout.replace(
          'const identityContext = createIdentityViewContext();',
          'const identityContext = createIdentityViewContext(); const other = createIdentityViewContext();'
        );
      if (mutation === 'extra-mount')
        layout = layout.replace(
          'mountIdentityView(identityContext);',
          'mountIdentityView(identityContext); mountIdentityView(identityContext);'
        );
      if (mutation === 'aliased-mount')
        layout = layout
          .replace(
            '    mountIdentityView,',
            '    mountIdentityView as otherMount,'
          )
          .replace(
            'mountIdentityView(identityContext);',
            'otherMount(identityContext);'
          );
      if (mutation === 'shadowed')
        layout = layout.replace(
          'const off = subscribeIdentityView',
          'const identityContext = {}; const off = subscribeIdentityView'
        );
      if (mutation === 'missing-context')
        layout = layout.replace(
          'setContext(IDENTITY_VIEW_CONTEXT, identityContext);',
          ''
        );
      if (mutation === 'rebound-context')
        layout = layout.replace(
          'const identityContext =',
          'let identityContext ='
        );
      assert.notEqual(layout, original);
      await writeFile(file, layout);
      await refresh(f);
      await assert.rejects(f.audit, /Invalid owned root runtime activation/);
    });
  for (const mutation of [
    'missing-view',
    'missing-heads',
    'extra-view-field',
    'wrong-view-imports',
    'wrong-heads-imports',
    'missing-root-edge',
    'missing-gate-edge'
  ])
    await check(
      'owned identity compiler topology rejects ' + mutation,
      async (t) => {
        const f = await fixture(t),
          file = path.join(
            f.web,
            '.svelte-kit/output/client/.vite/manifest.json'
          );
        const manifest = JSON.parse(await readFile(file, 'utf8'));
        /** @param {string} name */
        const find = (name) => {
          const entry = Object.entries(manifest).find(
            ([, record]) => record.name === name
          );
          assert.ok(entry, 'Missing observed compiler role: ' + name);
          return entry;
        };
        const [identityKey, identity] = find('view-context'),
          [headsKey, heads] = find('heads');
        if (mutation === 'missing-view') delete manifest[identityKey];
        if (mutation === 'missing-heads') delete manifest[headsKey];
        if (mutation === 'extra-view-field') identity.dynamicImports = [];
        if (mutation === 'wrong-view-imports') identity.imports = [];
        if (mutation === 'wrong-heads-imports') heads.imports = [];
        if (
          mutation === 'missing-root-edge' ||
          mutation === 'missing-gate-edge'
        ) {
          const [, consumer] = find(
            mutation === 'missing-root-edge' ? 'nodes/0' : 'AccountGate'
          );
          consumer.imports = consumer.imports.filter(
            /** @param {string} dependency */
            (dependency) => dependency !== identityKey
          );
        }
        await writeFile(file, JSON.stringify(manifest));
        await assert.rejects(f.audit, /compiler/i);
      }
    );
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
    await check('root runtime admission requires owned ' + name, async (t) => {
      const f = await fixture(t);
      await rm(path.join(f.web, 'src/lib', name));
      await refresh(f);
      await assert.rejects(f.audit);
    });
  for (const routeSource of ['search/+page.svelte', 'messages/+page.svelte'])
    await check(
      routeSource +
        ' admission requires its owned source and exact module/page counterparts',
      async (t) => {
        const f = await fixture(t);
        const source = path.join(f.web, 'src/routes', routeSource);
        const original = await readFile(source);
        await rm(source);
        git(f.root, 'add', '--', 'web/src/routes/' + routeSource);
        await generateBuildInfo(f.web);
        const metadata = await readFile(
          path.join(f.web, 'static/build-info.json')
        );
        await writeFile(path.join(f.web, 'build/build-info.json'), metadata);
        await writeFile(
          path.join(f.web, '.svelte-kit/output/client/build-info.json'),
          metadata
        );
        await assert.rejects(f.audit, /Undeclared compiler module/);
        await writeFile(source, original);
      }
    );
  for (const mutation of [
    'unknown-name',
    'duplicate',
    'missing-module',
    'missing-page',
    'changed-page'
  ]) {
    await check(
      'exact search compiler admission rejects ' + mutation,
      async (t) => {
        const f = await fixture(t);
        const file = path.join(
          f.web,
          '.svelte-kit/output/client/.vite/manifest.json'
        );
        const manifest = JSON.parse(await readFile(file, 'utf8'));
        const entry = Object.entries(manifest).find(
          ([, record]) => record.name === 'routes'
        );
        assert.ok(entry, 'The actual shared routes module must exist');
        const [key, module] = entry;
        if (mutation === 'unknown-name') module.name = 'unapproved-routes';
        if (mutation === 'duplicate') manifest.duplicate = { ...module };
        if (mutation === 'missing-module') delete manifest[key];
        if (mutation === 'missing-page')
          await rm(path.join(f.web, 'build/search.html'));
        if (mutation === 'changed-page')
          await writeFile(
            path.join(f.web, 'build/search.html'),
            '<h1>Different source</h1>'
          );
        await writeFile(file, JSON.stringify(manifest));
        await assert.rejects(f.audit);
      }
    );
  }
  for (const mutation of ['missing', 'changed']) {
    await check('full-route static robots rejects ' + mutation, async (t) => {
      const f = await fixture(t);
      const target = path.join(f.web, 'build/robots.txt');
      if (mutation === 'missing') await rm(target);
      else await writeFile(target, 'User-agent: *\nAllow: /\n');
      await assert.rejects(f.audit);
    });
  }
  await check(
    'full-route compiler rejects an unowned thirteenth node',
    async (t) => {
      const f = await fixture(t);
      const file = path.join(
        f.web,
        '.svelte-kit/output/client/.vite/manifest.json'
      );
      const manifest = JSON.parse(await readFile(file, 'utf8'));
      const entry = Object.entries(manifest).find(
        ([, record]) => record.name === 'nodes/12'
      );
      assert.ok(entry);
      entry[1].name = 'nodes/13';
      await writeFile(file, JSON.stringify(manifest));
      await assert.rejects(f.audit, /Undeclared compiler module/);
    }
  );
  await check(
    'actual shared budgets chunk has owned consumers and counterpart',
    async (t) => {
      const f = await fixture(t);
      const manifest = JSON.parse(
        await readFile(
          path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
          'utf8'
        )
      );
      const entries = Object.entries(manifest).filter(
        ([, record]) => record.name === 'Button'
      );
      assert.equal(entries.length, 1);
      const [key, budget] = entries[0];
      assert.deepEqual(Object.keys(budget).sort(), ['file', 'imports', 'name']);
      const client = Object.entries(manifest).find(
        ([, record]) => record.name === 'client'
      );
      assert.ok(client);
      assert.deepEqual(budget.imports, [client[0]]);
      for (const name of ['Disclosure', 'nodes/9', 'routes'])
        assert.ok(
          Object.values(manifest)
            .find((record) => record.name === name)
            .imports.includes(key)
        );
      assert.deepEqual(
        await readFile(path.join(f.web, 'build', budget.file)),
        await readFile(
          path.join(f.web, '.svelte-kit/output/client', budget.file)
        )
      );
      assert.ok((await f.audit()).includes(budget.file));
    }
  );
  for (const producer of [
    'src/routes/products/[naddr=naddr]/+page.svelte',
    'src/lib/catalog/product-view.ts',
    'src/lib/components/ListingFacts.svelte',
    'src/lib/components/PublisherIdentity.svelte',
    'src/lib/contracts/food-availability-v1/contact-read.ts',
    'src/lib/navigation-copy.ts',
    'src/lib/runtime/public-runtime.ts',
    'src/lib/catalog/publishers.ts',
    'src/lib/catalog/resolve-head.ts'
  ]) {
    await check(
      'real product compiler rejects changed owned producer ' + producer,
      async (t) => {
        const f = await fixture(t);
        await writeFile(
          path.join(f.web, producer),
          (await readFile(path.join(f.web, producer), 'utf8')) +
            '\n// source mutation\n'
        );
        await refresh(f);
        await assert.rejects(
          f.audit,
          /Invalid owned product presentation source/
        );
      }
    );
  }
  for (const mutation of [
    'missing-publishers',
    'publisher-budget',
    'publisher-shared',
    'publisher-dynamic',
    'product-shared',
    'product-publisher',
    'product-budget',
    'product-dynamic',
    'search-publisher'
  ]) {
    await check('real product compiler rejects ' + mutation, async (t) => {
      const f = await fixture(t),
        file = path.join(
          f.web,
          '.svelte-kit/output/client/.vite/manifest.json'
        );
      const manifest = JSON.parse(await readFile(file, 'utf8'));
      const records = Object.entries(manifest);
      const find = (/** @type {string} */ name) => {
        const row = records.find(([, v]) => v.name === name);
        assert.ok(row);
        return row;
      };
      const [publisherKey, publisher] = find('publishers');
      if (mutation === 'missing-publishers') delete manifest[publisherKey];
      else if (mutation === 'publisher-dynamic')
        publisher.dynamicImports = [find('client-entry')[0]];
      else if (mutation === 'product-dynamic')
        find('nodes/7')[1].dynamicImports = [find('client-entry')[0]];
      else if (
        mutation === 'publisher-budget' ||
        mutation === 'publisher-shared'
      )
        publisher.imports = publisher.imports.filter(
          (/** @type {string} */ key) =>
            key !==
            find(mutation === 'publisher-budget' ? 'Button' : 'Disclosure')[0]
        );
      else {
        const target = mutation === 'search-publisher' ? 'nodes/9' : 'nodes/7';
        const dependency =
          mutation === 'product-shared'
            ? 'Disclosure'
            : mutation === 'product-budget'
              ? 'Button'
              : 'publishers';
        find(target)[1].imports = find(target)[1].imports.filter(
          (/** @type {string} */ key) => key !== find(dependency)[0]
        );
      }
      await writeFile(file, JSON.stringify(manifest));
      await assert.rejects(f.audit, /compiler|budgets/);
    });
  }
  for (const producer of [
    'src/routes/search/+page.svelte',
    'src/lib/catalog/search-view.ts',
    'src/lib/components/ListingRow.svelte',
    'src/lib/components/SourceStatus.svelte',
    'src/lib/navigation-scroll.ts',
    'src/lib/components/primitives/Disclosure.svelte'
  ]) {
    await check(
      'real search compiler rejects changed owned producer ' + producer,
      async (t) => {
        const f = await fixture(t);
        await writeFile(
          path.join(f.web, producer),
          (await readFile(path.join(f.web, producer), 'utf8')) +
            '\n// source mutation\n'
        );
        await refresh(f);
        await assert.rejects(
          f.audit,
          /Invalid owned search presentation source/
        );
      }
    );
  }
  for (const mutation of [
    'missing-shared',
    'missing-navigation',
    'root-edge',
    'search-edge',
    'budget-edge',
    'negentropy-edge',
    'sdk-backedge',
    'navigation-edge',
    'navigation-import',
    'navigation-dynamic',
    'shared-import'
  ]) {
    await check('real search compiler rejects ' + mutation, async (t) => {
      const f = await fixture(t);
      const file = path.join(
        f.web,
        '.svelte-kit/output/client/.vite/manifest.json'
      );
      const manifest = JSON.parse(await readFile(file, 'utf8'));
      const records = Object.entries(manifest);
      const find = (/** @type {string} */ name) => {
        const entry = records.find(([, v]) => v.name === name);
        assert.ok(entry, 'Missing actual compiler record ' + name);
        return entry;
      };
      const [sharedKey, shared] = find('Disclosure');
      const [budgetKey] = find('Button');
      if (mutation === 'missing-shared') delete manifest[sharedKey];
      else if (mutation === 'missing-navigation')
        delete manifest[find('navigation')[0]];
      else if (mutation === 'root-edge')
        find('nodes/0')[1].imports = find('nodes/0')[1].imports.filter(
          (/** @type {string} */ k) => k !== sharedKey
        );
      else if (mutation === 'search-edge')
        find('nodes/9')[1].imports = find('nodes/9')[1].imports.filter(
          (/** @type {string} */ k) => k !== sharedKey
        );
      else if (mutation === 'budget-edge')
        shared.imports = shared.imports.filter(
          (/** @type {string} */ k) => k !== budgetKey
        );
      else if (mutation === 'sdk-backedge') find('negentropy')[1].imports = [];
      else if (mutation === 'navigation-edge')
        find('nodes/9')[1].imports = find('nodes/9')[1].imports.filter(
          (/** @type {string} */ k) => k !== find('navigation')[0]
        );
      else if (mutation === 'navigation-import')
        find('navigation')[1].imports = [find('client')[0]];
      else if (mutation === 'navigation-dynamic')
        find('navigation')[1].dynamicImports = [find('client-entry')[0]];
      else if (mutation === 'shared-import')
        shared.imports = shared.imports.filter(
          (/** @type {string} */ k) => k !== find('public-key')[0]
        );
      else shared.dynamicImports = [];
      await writeFile(file, JSON.stringify(manifest));
      await assert.rejects(f.audit, /compiler|SDK|budgets/);
    });
  }
  for (const mutation of [
    'unknown-name',
    'duplicate',
    'missing-module',
    'extra-field',
    'missing-root-edge',
    'missing-routes-edge',
    'missing-counterpart',
    'changed-counterpart'
  ]) {
    await check('budgets module fails closed: ' + mutation, async (t) => {
      const f = await fixture(t);
      const file = path.join(
        f.web,
        '.svelte-kit/output/client/.vite/manifest.json'
      );
      const manifest = JSON.parse(await readFile(file, 'utf8'));
      const entry = Object.entries(manifest).find(
        ([, record]) => record.name === 'Button'
      );
      assert.ok(entry);
      const [key, budget] = entry;
      if (mutation === 'unknown-name') budget.name = 'unapproved-budgets';
      if (mutation === 'duplicate') manifest['_HCduplicate.js'] = { ...budget };
      if (mutation === 'missing-module') delete manifest[key];
      if (mutation === 'extra-field') budget.dynamicImports = [];
      if (
        mutation === 'missing-root-edge' ||
        mutation === 'missing-routes-edge'
      ) {
        const name = mutation === 'missing-root-edge' ? 'nodes/9' : 'routes';
        const consumer = Object.values(manifest).find(
          (record) => record.name === name
        );
        consumer.imports = consumer.imports.filter(
          /** @param {string} dependency */
          (dependency) => dependency !== key
        );
      }
      if (mutation === 'missing-counterpart')
        await rm(path.join(f.web, 'build', budget.file));
      if (mutation === 'changed-counterpart')
        await writeFile(
          path.join(f.web, 'build', budget.file),
          'changed counterpart'
        );
      await writeFile(file, JSON.stringify(manifest));
      await assert.rejects(
        f.audit,
        /Undeclared compiler module|Unresolved compiler import|Invalid owned budgets compiler|Static output inventory mismatch|Static output differs from owned compiler input/
      );
    });
  }
  await check(
    'actual route-state compiler module has an owned client counterpart',
    async (t) => {
      const f = await fixture(t);
      const manifest = JSON.parse(
        await readFile(
          path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
          'utf8'
        )
      );
      const entries = Object.entries(manifest).filter(
        ([, record]) => record.name === 'state'
      );
      assert.equal(entries.length, 1);
      const [key, state] = entries[0];
      assert.match(key, /^_[A-Za-z0-9_-]+\.js$/);
      assert.ok(
        state.imports.some(
          /** @param {string} dependency */
          (dependency) => manifest[dependency].name === 'client.svelte'
        )
      );
      assert.deepEqual(
        await readFile(path.join(f.web, 'build', state.file)),
        await readFile(
          path.join(f.web, '.svelte-kit/output/client', state.file)
        )
      );
      assert.ok((await f.audit()).includes(state.file));
    }
  );
  for (const mutation of [
    'unknown-name',
    'duplicate-state',
    'missing-counterpart',
    'changed-counterpart'
  ]) {
    await check('state module fails closed: ' + mutation, async (t) => {
      const f = await fixture(t);
      const file = path.join(
        f.web,
        '.svelte-kit/output/client/.vite/manifest.json'
      );
      const manifest = JSON.parse(await readFile(file, 'utf8'));
      const state = Object.values(manifest).find(
        (record) => record.name === 'state'
      );
      assert.ok(
        state,
        'The actual route-state module must exist before mutation'
      );
      if (mutation === 'unknown-name') state.name = 'state-unapproved';
      if (mutation === 'duplicate-state')
        manifest['_HCduplicate.js'] = { ...state };
      if (mutation === 'missing-counterpart')
        await rm(path.join(f.web, 'build', state.file));
      if (mutation === 'changed-counterpart')
        await writeFile(
          path.join(f.web, 'build', state.file),
          Buffer.concat([
            await readFile(path.join(f.web, 'build', state.file)),
            Buffer.from('changed counterpart')
          ])
        );
      await writeFile(file, JSON.stringify(manifest));
      await assert.rejects(
        f.audit,
        mutation.endsWith('name') || mutation === 'duplicate-state'
          ? /Undeclared compiler module/
          : /Static output inventory mismatch|Static output differs from owned compiler input/
      );
    });
  }
  await check(
    'actual shared Button compiler module has an owned client counterpart',
    async (t) => {
      const f = await fixture(t);
      const manifest = JSON.parse(
        await readFile(
          path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
          'utf8'
        )
      );
      const entries = Object.entries(manifest).filter(
        ([, record]) => record.name === 'Button'
      );
      assert.equal(entries.length, 1);
      const [key, button] = entries[0];
      assert.match(key, /^_[A-Za-z0-9_-]+\.js$/);
      assert.ok(
        button.imports.some(
          /** @param {string} dependency */
          (dependency) => manifest[dependency].name === 'client'
        )
      );
      for (const consumer of ['nodes/0', 'nodes/2']) {
        assert.ok(
          Object.values(manifest).some(
            (record) => record.name === consumer && record.imports.includes(key)
          )
        );
      }
      assert.deepEqual(
        await readFile(path.join(f.web, 'build', button.file)),
        await readFile(
          path.join(f.web, '.svelte-kit/output/client', button.file)
        )
      );
      assert.ok((await f.audit()).includes(button.file));
    }
  );
  for (const mutation of [
    'unknown-name',
    'duplicate-Button',
    'missing-counterpart',
    'changed-counterpart'
  ]) {
    await check('Button module fails closed: ' + mutation, async (t) => {
      const f = await fixture(t);
      const file = path.join(
        f.web,
        '.svelte-kit/output/client/.vite/manifest.json'
      );
      const manifest = JSON.parse(await readFile(file, 'utf8'));
      const state = Object.values(manifest).find(
        (record) => record.name === 'Button'
      );
      assert.ok(
        state,
        'The actual shared Button module must exist before mutation'
      );
      if (mutation === 'unknown-name') state.name = 'Button-unapproved';
      if (mutation === 'duplicate-Button')
        manifest['_HCduplicate.js'] = { ...state };
      if (mutation === 'missing-counterpart')
        await rm(path.join(f.web, 'build', state.file));
      if (mutation === 'changed-counterpart')
        await writeFile(
          path.join(f.web, 'build', state.file),
          Buffer.concat([
            await readFile(path.join(f.web, 'build', state.file)),
            Buffer.from('changed counterpart')
          ])
        );
      await writeFile(file, JSON.stringify(manifest));
      await assert.rejects(
        f.audit,
        mutation.endsWith('name') || mutation === 'duplicate-Button'
          ? /Undeclared compiler module/
          : /Static output inventory mismatch|Static output differs from owned compiler input/
      );
    });
  }
  for (const name of [
    'notes.txt',
    '_app/immutable/chunks/innocent.ABC12345.js',
    'docs/parent.html',
    'state.css',
    'source.map',
    'contracts/vector.json'
  ]) {
    await check(`undeclared copied output is rejected: ${name}`, async (t) => {
      const f = await fixture(t);
      const file = path.join(f.web, 'build', name);
      await mkdir(path.dirname(file), { recursive: true });
      await writeFile(file, 'ordinary public text');
      await assert.rejects(f.audit);
    });
  }
  for (const payload of [
    Buffer.from('SQLite format 3\0'),
    Buffer.from([0x7f, 0x45, 0x4c, 0x46]),
    Buffer.from([0xcf, 0xfa, 0xed, 0xfe]),
    Buffer.from('PK\x03\x04'),
    Buffer.from('MZnative'),
    Buffer.from('generated UniFFI bindings'),
    Buffer.from('HC_TEST_ONLY_SECRET'),
    Buffer.from('nsec1' + 'a'.repeat(60)),
    Buffer.from('-----BEGIN PRIVATE KEY-----'),
    Buffer.from('private_transcript: controlled disclosure'),
    Buffer.from('class TestSigner {}')
  ]) {
    await check(
      `manifest-admitted contamination ${payload.subarray(0, 18).toString('hex')}`,
      async (t) => {
        const f = await fixture(t);
        const manifest = JSON.parse(
          await readFile(
            path.join(f.web, '.svelte-kit/output/client/.vite/manifest.json'),
            'utf8'
          )
        );
        const name = Object.values(manifest).find(
          (entry) => entry.name === 'client'
        ).file;
        // Poison both sides: byte mapping alone does not qualify contents.
        for (const directory of ['build', '.svelte-kit/output/client'])
          await writeFile(path.join(f.web, directory, name), payload);
        await assert.rejects(f.audit, /contamination|binary|text/);
      }
    );
  }
  for (const name of [
    'index.html',
    '200.html',
    '_app/version.json',
    'build-info.json'
  ]) {
    await check(`missing/tampered owned output: ${name}`, async (t) => {
      const f = await fixture(t);
      const file = path.join(f.web, 'build', name);
      const original = await readFile(file);
      await rm(file);
      await assert.rejects(f.audit);
      await writeFile(file, Buffer.concat([original, Buffer.from('tampered')]));
      await assert.rejects(f.audit);
    });
  }
  for (const mutation of [
    'unknown',
    'private',
    'revision',
    'dirty',
    'lock',
    'config',
    'source',
    'generated'
  ]) {
    await check(`provenance fails closed: ${mutation}`, async (t) => {
      const f = await fixture(t);
      const metadata = JSON.parse(
        await readFile(path.join(f.web, 'build/build-info.json'), 'utf8')
      );
      if (mutation === 'unknown') metadata.web_source.unknown = 'extra';
      if (mutation === 'private')
        metadata.private_transcript = 'controlled disclosure';
      if (mutation === 'revision')
        metadata.web_source.revision = 'a'.repeat(40);
      if (mutation === 'dirty')
        metadata.web_source.dirty = !metadata.web_source.dirty;
      if (mutation === 'lock')
        await writeFile(path.join(f.web, 'pnpm-lock.yaml'), 'changed lock');
      if (mutation === 'config')
        await writeFile(path.join(f.web, 'vite.config.ts'), 'changed config');
      if (mutation === 'source')
        await writeFile(path.join(f.web, 'src/app.html'), 'changed source');
      if (mutation === 'generated')
        git(f.root, 'add', '-f', 'web/static/build-info.json');
      if (['unknown', 'private', 'revision', 'dirty'].includes(mutation)) {
        for (const directory of [
          'build',
          'static',
          '.svelte-kit/output/client'
        ])
          await writeFile(
            path.join(f.web, directory, 'build-info.json'),
            JSON.stringify(metadata)
          );
      }
      await assert.rejects(f.audit);
    });
  }
  for (const kind of [
    'symlink',
    'hardlink',
    'fifo',
    'directory',
    'oversize',
    'parent'
  ]) {
    await check(`unsafe output refuses ${kind} promptly`, async (t) => {
      const f = await fixture(t);
      const file = path.join(f.web, 'build/index.html');
      await rm(file);
      if (kind === 'symlink') await symlink('../src/app.html', file);
      if (kind === 'hardlink')
        await link(path.join(f.web, 'src/app.html'), file);
      if (kind === 'fifo') execFileSync('mkfifo', [file]);
      if (kind === 'directory') await mkdir(file);
      if (kind === 'oversize')
        await writeFile(file, Buffer.alloc(8 * 1024 * 1024 + 1));
      if (kind === 'parent') {
        await rm(path.join(f.web, 'build'), { recursive: true });
        await symlink(path.join(base, 'web/build'), path.join(f.web, 'build'));
      }
      await assert.rejects(f.audit);
    });
  }
  await check(
    'compiler manifest cannot authorize arbitrary resources, escapes or unresolved imports',
    async (t) => {
      const f = await fixture(t);
      const file = path.join(
        f.web,
        '.svelte-kit/output/client/.vite/manifest.json'
      );
      const original = await readFile(file, 'utf8');
      for (const mutation of ['extra', 'escape', 'reference', 'unknown']) {
        const manifest = JSON.parse(original);
        const record = Object.values(manifest)[0];
        if (mutation === 'extra')
          manifest.extra = {
            name: 'provider',
            file: '_app/immutable/chunks/provider.ABC12345.js'
          };
        if (mutation === 'escape') record.file = '../outside.js';
        if (mutation === 'reference') record.imports = ['absent'];
        if (mutation === 'unknown') record.secret = 'private';
        await writeFile(file, JSON.stringify(manifest));
        await assert.rejects(f.audit);
      }
    }
  );

  await check(
    'generated provenance must remain ignored even when freshly regenerated',
    async (t) => {
      const f = await fixture(t);
      await writeFile(path.join(f.web, '.gitignore'), '');
      await generateBuildInfo(f.web);
      await assert.rejects(f.audit, /must be ignored/);
    }
  );
  for (const name of [
    'static/build-info.json',
    '.svelte-kit/output/client/.vite/manifest.json',
    '.svelte-kit/output/prerendered/pages/index.html',
    '.svelte-kit/output/prerendered/pages/search.html',
    'src/routes/search/+page.svelte',
    'static/robots.txt',
    '.svelte-kit/output/prerendered/pages/messages.html'
  ]) {
    for (const kind of ['fifo', 'symlink', 'hardlink']) {
      await check(
        `unsafe compiler/static counterpart ${name}: ${kind}`,
        async (t) => {
          const f = await fixture(t);
          const file = path.join(f.web, name);
          await rm(file);
          if (kind === 'fifo') execFileSync('mkfifo', [file]);
          if (kind === 'symlink')
            await symlink(path.join(base, 'web/src/app.html'), file);
          if (kind === 'hardlink')
            await link(path.join(f.web, 'src/app.html'), file);
          await assert.rejects(f.audit);
        }
      );
    }
  }
  await check(
    'empty output directory and excessive depth are rejected',
    async (t) => {
      const f = await fixture(t);
      await mkdir(path.join(f.web, 'build/extra'));
      await assert.rejects(f.audit);
      await rm(path.join(f.web, 'build/extra'), { recursive: true });
      await mkdir(path.join(f.web, 'build/a/b/c/d/e/f/g/h/i'), {
        recursive: true
      });
      await assert.rejects(f.audit);
    }
  );
  await check(
    'forged public output failures do not echo private poisoned bytes',
    async (t) => {
      const f = await fixture(t);
      const marker = 'CONTROLLED_SENSITIVE_DO_NOT_ECHO';
      const file = path.join(f.web, 'static/build-info.json');
      await writeFile(file, marker);
      await assert.rejects(f.audit, (error) => !String(error).includes(marker));
    }
  );
  await check(
    'output entry and aggregate-byte bounds reject oversized inventories',
    async (t) => {
      const f = await fixture(t);
      for (let i = 0; i < 513; i++)
        await writeFile(path.join(f.web, 'build', `tiny-${i}.js`), 'x');
      // Confine the actual audit's allocation measurement to a fresh child;
      // prior source/output fixture copies must not enter this baseline/peak.
      const measurement = JSON.parse(
        execFileSync(
          process.execPath,
          [
            '--input-type=module',
            '-e',
            `
          const { auditOutput } = await import(process.argv[2]);
          const baseline = process.memoryUsage().arrayBuffers;
          let peak = baseline, rejection;
          const sample = () => { peak = Math.max(peak, process.memoryUsage().arrayBuffers); };
          const timer = setInterval(sample, 1);
          try { await auditOutput(process.argv[1]); }
          catch (error) { rejection = error instanceof Error ? error.message : 'unexpected'; }
          finally { sample(); clearInterval(timer); }
          if (rejection !== 'Unbounded output inventory') throw new Error('Unexpected inventory measurement rejection');
          process.stdout.write(JSON.stringify({ baseline, peak, growth: peak - baseline, rejection }));
        `,
            f.web,
            new URL('../../tools/check-output.mjs', import.meta.url).href
          ],
          { cwd: f.web, encoding: 'utf8', timeout: 8000, maxBuffer: 4096 }
        )
      );
      assert.deepEqual(Object.keys(measurement).sort(), [
        'baseline',
        'growth',
        'peak',
        'rejection'
      ]);
      assert.equal(measurement.rejection, 'Unbounded output inventory');
      for (const name of ['baseline', 'peak', 'growth'])
        assert.ok(
          Number.isSafeInteger(measurement[name]) && measurement[name] >= 0
        );
      assert.equal(measurement.growth, measurement.peak - measurement.baseline);
      t.diagnostic(
        'isolated actual inventory buffers: ' + JSON.stringify(measurement)
      );
      assert.ok(
        measurement.growth < 64 * 1024 * 1024,
        'Tiny files must not retain a maximum-sized buffer per entry: ' +
          JSON.stringify(measurement)
      );
      for (let i = 0; i < 513; i++)
        await rm(path.join(f.web, 'build', `tiny-${i}.js`));
      for (let i = 0; i < 33; i++)
        await writeFile(
          path.join(f.web, 'build', `large-${i}.js`),
          Buffer.alloc(1024 * 1024, 97)
        );
      await assert.rejects(f.audit, /Unbounded output bytes/);
    }
  );
  await check(
    'actual compiler admits nonempty approved theme/app CSS',
    async () => {
      const layout = path.join(base, 'web/src/routes/+layout.svelte');
      await writeFile(
        layout,
        (await readFile(layout, 'utf8')).replace(
          '<script lang="ts">',
          "<script lang=\"ts\">\n  import '../theme.css';\n  import '../app.css';"
        )
      );
      await writeFile(
        path.join(base, 'web/src/theme.css'),
        ':root { --hc-accent: #336633; }\n'
      );
      await writeFile(
        path.join(base, 'web/src/app.css'),
        'main { color: var(--hc-accent); }\n'
      );
      await generateBuildInfo(path.join(base, 'web'));
      execFileSync(
        process.execPath,
        [path.join(source, 'web/node_modules/vite/bin/vite.js'), 'build'],
        { cwd: path.join(base, 'web'), stdio: 'pipe', timeout: 60000 }
      );
      const files = await auditOutput(await realpath(path.join(base, 'web')));
      assert.ok(files.some((name) => name.endsWith('.css')));
    },
    60000
  );
});
