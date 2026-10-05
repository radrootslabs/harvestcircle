import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import {
  copyFile,
  link,
  mkdir,
  mkdtemp,
  readFile,
  realpath,
  rm,
  symlink,
  writeFile
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const root = fileURLToPath(new URL('../../', import.meta.url));
/** @typedef {{directory: string, put: (name: string, content: string | Uint8Array) => Promise<void>, execute: (args?: string[]) => import('node:child_process').SpawnSyncReturns<string>, pkg: Record<string, unknown>}} Fixture */
/** @param {(context: Fixture) => Promise<void>} run */
async function fixture(run) {
  const directory = await realpath(
    await mkdtemp(path.join(tmpdir(), 'hcr010 source '))
  );
  /** @type {Fixture['put']} */
  const put = async (name, content) => {
    await mkdir(path.dirname(path.join(directory, name)), { recursive: true });
    await writeFile(path.join(directory, name), content);
  };
  try {
    await mkdir(path.join(directory, 'tools'));
    await mkdir(path.join(directory, 'src/routes'), { recursive: true });
    for (const name of [
      'package.json',
      'tsconfig.json',
      'vite.config.ts',
      'tools/check-source.mjs',
      'tools/source-boundaries.mjs'
    ])
      await copyFile(path.join(root, name), path.join(directory, name));
    await symlink(
      path.join(root, 'node_modules'),
      path.join(directory, 'node_modules')
    );
    await put(
      'src/lib/navigation-url.ts',
      await readFile(path.join(root, 'src/lib/navigation-url.ts'), 'utf8')
    );
    await put('src/routes/+page.ts', 'export const value = 1;');
    const execute = (args = ['tools/check-source.mjs']) => {
      const result = spawnSync(process.execPath, args, {
        cwd: directory,
        encoding: 'utf8',
        timeout: 8000
      });
      assert.equal(result.error, undefined);
      return result;
    };
    const pkg = JSON.parse(
      await readFile(path.join(directory, 'package.json'), 'utf8')
    );
    await run({ directory, put, execute, pkg });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}
/** @param {string} directory */
async function confineSvelte(directory) {
  await rm(path.join(directory, 'node_modules'));
  await mkdir(path.join(directory, 'node_modules'));
  await symlink(
    path.join(root, 'node_modules/typescript'),
    path.join(directory, 'node_modules/typescript')
  );
  const { cp } = await import('node:fs/promises');
  await cp(
    await realpath(path.join(root, 'node_modules/svelte')),
    path.join(directory, 'node_modules/svelte'),
    { recursive: true }
  );
  const svelteRoot = await realpath(path.join(root, 'node_modules/svelte'));
  const sveltePackage = JSON.parse(
    await readFile(path.join(svelteRoot, 'package.json'), 'utf8')
  );
  for (const dependency of Object.keys(sveltePackage.dependencies)) {
    const destination = path.join(directory, 'node_modules', dependency);
    await mkdir(path.dirname(destination), { recursive: true });
    await symlink(path.join(path.dirname(svelteRoot), dependency), destination);
  }
}

/** @param {import('node:child_process').SpawnSyncReturns<string>} result @param {RegExp} diagnostic */
const reject = (result, diagnostic) => {
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, diagnostic);
};

for (const [name, content, diagnostic] of [
  [
    'native extension import',
    "import '../../../core/Cargo.toml';",
    /forbidden production import/
  ],
  [
    'native dynamic import',
    "void import('../../../core/native.js');",
    /forbidden production import/
  ],
  [
    'test reexport',
    "export * from '../../tests/e2e/harness/provider';",
    /forbidden production import/
  ],
  [
    'dynamic test signer',
    "void import('../../tests/signer');",
    /forbidden production import/
  ],
  [
    'NDK dynamic client',
    "void import('@nostr-dev-kit/ndk');",
    /forbidden production import/
  ],
  [
    'nostr-tools dynamic client',
    "void import('nostr-tools/pool');",
    /forbidden production import/
  ],
  [
    'computed import',
    "const name = 'nostr-tools'; void import(name);",
    /computed production imports/
  ],
  [
    'template computed import',
    'const name = "client"; void import(`./${name}`);',
    /computed production imports/
  ],
  [
    'glob import',
    "const files = import.meta.glob('./*.ts');",
    /computed production imports/
  ],
  [
    'require loader',
    "const client = require('nostr-tools');",
    /loader\/provider\/credential/
  ],
  [
    'raw socket',
    "const socket = new WebSocket('wss://example.invalid');",
    /loader\/provider\/credential/
  ],
  [
    'indirect socket',
    "const socket = new globalThis['WebSocket']('wss://example.invalid');",
    /indirect production loaders/
  ],
  [
    'secret generation',
    'export const key = generateSecretKey();',
    /credential API/
  ],
  [
    'copied test marker',
    "export const marker = 'HC_TEST_ONLY_PROVIDER';",
    /test provider code/
  ],
  [
    'unknown package',
    "import 'some-unresolved-package';",
    /forbidden production import/
  ],
  ['unknown local', "import './missing';", /unresolved production import/],
  ['query loader', "import './safe.ts?raw';", /unsafe production import/],
  [
    'escaped POSIX filename',
    "import '..\\\\tests\\\\signer';",
    /unsafe production import/
  ],
  [
    'Applesauce outside adapter',
    "import 'applesauce-core';",
    /Applesauce imports/
  ],
  [
    'removed Kit3 alias',
    "import '$lib/signer';",
    /forbidden production import/
  ],
  [
    'source reference',
    '/// <reference path="../../tests/signer.ts" />\nexport {};',
    /reference directives/
  ]
])
  test(`rejects ${name}`, async () => {
    await fixture(async ({ put, execute }) => {
      if (typeof content !== 'string' || !(diagnostic instanceof RegExp))
        throw new Error('Invalid mutation case');
      await put('src/routes/+page.ts', content);
      reject(execute(), diagnostic);
    });
  });

test('follows extension and directory-index reexports to test provider', async () => {
  await fixture(async ({ put, execute }) => {
    await put('src/routes/+page.ts', "export * from '../lib/bridge.js';");
    await put('src/lib/bridge.ts', "export * from './client';");
    await put(
      'src/lib/client/index.ts',
      "export * from '../../../tests/e2e/harness/provider';"
    );
    reject(execute(), /forbidden production import/);
  });
});
test('local cycles, emitted extensions and test-only harnesses pass', async () => {
  await fixture(async ({ put, execute }) => {
    await put('src/routes/+page.ts', "export * from '../lib/bridge.js';");
    await put(
      'src/lib/bridge.ts',
      "export * from './client'; export const bridge = 1;"
    );
    await put('src/lib/client/index.ts', "export { bridge } from '../bridge';");
    await put(
      'tests/harness/provider.ts',
      "export const testSigner = new WebSocket('ws://127.0.0.1');"
    );
    const result = execute();
    assert.equal(result.status, 0, result.stderr);
  });
});
for (const kind of ['package', 'conditional package', 'Vite', 'TypeScript'])
  test(`rejects test reachability through ${kind} aliases`, async () => {
    await fixture(async ({ directory, put, execute, pkg }) => {
      await put('src/routes/+page.ts', "export * from '#bridge';");
      if (kind.includes('package')) {
        pkg.imports = {
          '#bridge':
            kind === 'package'
              ? './tests/signer.ts'
              : { browser: './tests/signer.ts', default: './src/lib/safe.ts' }
        };
        await put('package.json', JSON.stringify(pkg));
        await put('src/lib/safe.ts', 'export const safe = 1;');
      } else if (kind === 'Vite') {
        const vite = await readFile(
          path.join(directory, 'vite.config.ts'),
          'utf8'
        );
        await put(
          'vite.config.ts',
          vite.replace(
            'plugins: [',
            "resolve: { alias: { '#bridge': './tests/signer.ts' } }, plugins: ["
          )
        );
      } else
        await put(
          'tsconfig.json',
          JSON.stringify({
            extends: '$app/tsconfig',
            compilerOptions: {
              strict: true,
              paths: { '#bridge': ['./tests/signer.ts'] }
            }
          })
        );
      reject(execute(), /forbidden production import/);
    });
  });
test('valid Kit3 package wildcard aliases resolve local index modules', async () => {
  await fixture(async ({ put, execute, pkg }) => {
    pkg.imports = { '#modules/*': './src/lib/*' };
    await put('package.json', JSON.stringify(pkg));
    await put('src/routes/+page.ts', "export * from '#modules/client';");
    await put('src/lib/client/index.ts', 'export const client = 1;');
    assert.equal(execute().status, 0);
  });
});
test('npm package alias cannot conceal forbidden dependency identity', async () => {
  await fixture(async ({ put, execute, pkg }) => {
    pkg.dependencies = { innocent: 'npm:nostr-tools@2.0.0' };
    await put('package.json', JSON.stringify(pkg));
    await put('src/routes/+page.ts', "export * from 'innocent/pool';");
    reject(execute(), /forbidden production import/);
  });
});
for (const [name, content, diagnostic] of [
  [
    'component.svelte',
    '<p>bad</p><style>p { color: red }</style>',
    /component CSS/
  ],
  ['inline.svelte', '<p style="color: red">bad</p>', /inline styles/],
  ['directive.svelte', '<p style:color="red">bad</p>', /inline styles/],
  ['extra.css', 'p { color: red }', /handwritten CSS/],
  [
    'app.html',
    '<body style="color: red">%sveltekit.body%</body>',
    /inline styles/
  ]
])
  test(`rejects ${name} style violation`, async () => {
    await fixture(async ({ put, execute }) => {
      if (typeof content !== 'string' || !(diagnostic instanceof RegExp))
        throw new Error('Invalid mutation case');
      await put(`src/${name}`, content);
      reject(execute(), diagnostic);
    });
  });
test('two CSS paths must be imported once in order by root layout', async () => {
  await fixture(async ({ put, execute }) => {
    await put('src/theme.css', ':root { --color: black }');
    await put('src/app.css', 'p { color: var(--color) }');
    await put(
      'src/routes/+layout.svelte',
      '<script>import "../theme.css"; import "../app.css";</script><p>good</p>'
    );
    assert.equal(execute().status, 0);
    await put(
      'src/routes/+layout.svelte',
      '<script>import "../app.css"; import "../theme.css";</script><p>bad</p>'
    );
    reject(execute(), /exactly once/);
  });
});
for (const kind of ['symlink', 'hardlink', 'invalid UTF8', 'oversize', 'FIFO'])
  test(`rejects ${kind} before parsers`, async () => {
    await fixture(async ({ directory, put, execute }) => {
      const target = path.join(directory, 'src/unsafe.ts');
      if (kind === 'symlink')
        await symlink(path.join(root, 'tests/e2e/harness/provider.ts'), target);
      else if (kind === 'hardlink')
        await link(path.join(directory, 'src/routes/+page.ts'), target);
      else if (kind === 'invalid UTF8')
        await put('src/unsafe.ts', Buffer.from([0xff]));
      else if (kind === 'oversize')
        await put('src/unsafe.ts', Buffer.alloc(8 * 1024 * 1024 + 1, 32));
      else assert.equal(spawnSync('mkfifo', [target]).status, 0);
      reject(execute(), /symlinks|nonregular|Unsafe source|encoded data/);
    });
  });
for (const script of ['check', 'lint', 'build', 'test:unit', 'dev'])
  test(`actual ${script} stops on unsafe production edge`, async () => {
    await fixture(async ({ directory, put }) => {
      await put('src/routes/+page.ts', "void import('nostr-tools');");
      const result = spawnSync('corepack', ['pnpm', 'run', script], {
        cwd: directory,
        encoding: 'utf8',
        timeout: 8000
      });
      assert.equal(result.error, undefined);
      assert.equal(result.status, 1, result.stdout + result.stderr);
      assert.match(result.stderr, /forbidden production import nostr-tools/);
      assert.doesNotMatch(
        result.stdout,
        /svelte-check found|vite v.*building|RUN\s+v/
      );
    });
  });

test('Svelte template expressions cannot dynamically load forbidden clients', async () => {
  await fixture(async ({ put, execute }) => {
    await put(
      'src/routes/+page.svelte',
      '<button onclick={() => import("nostr-tools")}>bad</button>'
    );
    reject(execute(), /forbidden production import/);
  });
});
test('app.html inline executable scripts cannot bypass source graph', async () => {
  await fixture(async ({ put, execute }) => {
    await put(
      'src/app.html',
      '<script type="module">import "nostr-tools";</script>'
    );
    reject(execute(), /external scripts/);
  });
});
for (const css of [
  '@import "../../tests/styles.css";',
  'p { background: url(../../core/native) }',
  '@\\69mport "../../tests/styles.css";',
  'p { background: u/**/rl(../../tests/data) }'
])
  test(`rejects CSS loader ${css}`, async () => {
    await fixture(async ({ put, execute }) => {
      await put('src/theme.css', css);
      reject(execute(), /CSS import\/url loading/);
    });
  });
test('JS style assignment cannot bypass two CSS files', async () => {
  await fixture(async ({ put, execute }) => {
    await put('src/routes/+page.ts', 'document.body.style.color = "red";');
    reject(execute(), /JS-generated inline styles/);
  });
});
test('configuration execution and alternate configs fail closed', async () => {
  await fixture(async ({ put, execute }) => {
    await put('vite.config.js', 'export default {}');
    reject(execute(), /competing source configuration/);
  });
});
test('actual pinned Svelte exports and Kit3 framework modules pass in confined fixture', async () => {
  await fixture(async ({ directory, put, execute }) => {
    await confineSvelte(directory);
    await put(
      'src/routes/+page.ts',
      'import { writable } from "svelte/store"; export { writable }; export { goto } from "$app/navigation";'
    );
    const result = execute();
    assert.equal(result.status, 0, result.stderr);
    await put('src/routes/+page.ts', 'import "svelte/nonexistent-export";');
    reject(execute(), /unresolved production import/);
  });
});

test('type imports and Vite URL references cannot hide test reachability', async () => {
  await fixture(async ({ put, execute }) => {
    await put(
      'src/routes/+page.ts',
      'export type Client = typeof import("../../tests/signer");'
    );
    reject(execute(), /forbidden production import/);
    await put(
      'src/routes/+page.ts',
      'export const client = new URL("../../tests/signer", import.meta.url);'
    );
    reject(execute(), /forbidden production import/);
  });
});
test('production package imports exclude compiler and server tooling', async () => {
  await fixture(async ({ put, execute }) => {
    await put(
      'src/routes/+page.ts',
      'import "svelte/compiler"; import "@sveltejs/kit/node";'
    );
    reject(execute(), /forbidden production import/);
  });
});

test('npm aliases cannot hide compiler/server subpaths', async () => {
  await fixture(async ({ put, execute, pkg }) => {
    pkg.dependencies = {
      view: 'npm:svelte@5.57.1',
      framework: 'npm:@sveltejs/kit@3.0.0'
    };
    await put('package.json', JSON.stringify(pkg));
    await put(
      'src/routes/+page.ts',
      'import "view/compiler"; import "framework/node";'
    );
    reject(execute(), /forbidden production import view\/compiler/);
    assert.match(
      execute().stderr,
      /forbidden production import framework\/node/
    );
  });
});

for (const [name, markup] of [
  [
    'nested external script',
    '<svelte:head><script src="/tests/provider.js"></script></svelte:head>'
  ],
  [
    'nested inline script',
    '<svelte:head><script>import("nostr-tools")</script></svelte:head>'
  ],
  [
    'nested module script',
    '<div><script type="module">import "nostr-tools"</script></div>'
  ],
  [
    'nested style',
    '<svelte:head><style>p { color: red }</style></svelte:head>'
  ],
  [
    'nested CSS import',
    '<div><style>@import "/tests/style.css";</style></div>'
  ],
  [
    'stylesheet link',
    '<svelte:head><link rel="stylesheet" href="/native.css" /></svelte:head>'
  ],
  [
    'alternate stylesheet tokens',
    '<link rel="alternate stylesheet" href="/native.css" />'
  ],
  [
    'entity stylesheet token',
    '<link rel="style&#115;heet" href="/native.css" />'
  ],
  [
    'modulepreload link',
    '<link rel="modulepreload" href="/tests/provider.js" />'
  ],
  [
    'script preload',
    '<link rel="preload" as="script" href="/tests/provider.js" />'
  ],
  ['computed link relation', '<link rel={relation} href="/favicon.ico" />'],
  ['computed resource href', '<link rel="icon" href={resource} />'],
  [
    'computed resource type',
    '<link rel="icon" type={resourceType} href="/favicon.ico" />'
  ],
  ['spread link loader', '<link {...attributes} />'],
  ['spread style/event attributes', '<div {...attributes}>content</div>'],
  [
    'computed element tag',
    '<svelte:element this={tag} src="/tests/provider.js" />'
  ],
  [
    'static resource element tag',
    '<svelte:element this="script" src="/tests/provider.js" />'
  ],
  ['frame document', '<iframe src="/tests/provider.html"></iframe>'],
  [
    'frame srcdoc',
    '<iframe srcdoc="<script>import(\'nostr-tools\')</script>"></iframe>'
  ],
  ['object resource', '<object data="/tests/provider.html"></object>'],
  ['embed resource', '<embed src="/tests/provider.html" />'],
  ['raw HTML injection', '<div>{@html markup}</div>'],
  [
    'executable link URL',
    '<a href="java&#x09;script:import(\'nostr-tools\')">bad</a>'
  ],
  [
    'remote favicon resource',
    '<link rel="icon" href="https://foreign.invalid/favicon.ico" />'
  ]
])
  test(`markup guard rejects ${name}`, async () => {
    await fixture(async ({ put, execute }) => {
      await put('src/routes/+page.svelte', markup);
      const result = execute();
      assert.equal(result.status, 1, result.stdout + result.stderr);
      assert.match(result.stderr, /markup|favicon resources/);
    });
  });

for (const markup of [
  '<html><head><script src="/tests/provider.js"></script></head></html>',
  '<html><head><script type="module">import "nostr-tools"</script></head></html>',
  '<html><head><style>p { color: red }</style></head></html>',
  '<html><head><link rel="stylesheet" href="/tests/style.css" /></head></html>',
  '<html><head><link rel="modulepreload" href="/tests/provider.js" /></head></html>',
  '<HTML><HEAD><SCRIPT src="/tests/provider.js"></SCRIPT></HEAD></HTML>',
  '<HTML><HEAD><LINK REL="stylesheet" HREF="/tests/style.css" /></HEAD></HTML>',
  '<html><body><iframe srcdoc="<script>import(\'nostr-tools\')</script>"></iframe></body></html>',
  '<html><body onload="import(\'nostr-tools\')"></body></html>',
  '<html><head><link rel={relation} href="/favicon.ico" /></head></html>'
])
  test(`HTML template rejects active resource ${markup}`, async () => {
    await fixture(async ({ put, execute }) => {
      await put('src/app.html', markup);
      reject(execute(), /markup/);
    });
  });

test('legitimate shell modules, metadata and owned favicon links pass', async () => {
  await fixture(async ({ put, execute }) => {
    await put(
      'src/routes/+page.svelte',
      '<script lang="ts">const title: string = "HarvestCircle";</script><svelte:head><title>{title}</title><meta name="description" content="Public food listings" /><link rel="icon" href="/favicon.ico" /></svelte:head><svelte:element this="main"><p>{title}</p></svelte:element>'
    );
    await put(
      'src/app.html',
      '<!doctype html><html lang="en"><head><meta charset="utf-8" /><meta name="viewport" content="width=device-width, initial-scale=1" /><link rel="icon" href="%sveltekit.assets%/favicon.ico" />%sveltekit.head%</head><body><div>%sveltekit.body%</div></body></html>'
    );
    const result = execute();
    assert.equal(result.status, 0, result.stderr);
  });
});

for (const script of ['check', 'lint', 'build', 'test:unit', 'dev'])
  test(`actual ${script} stops on nested markup loader`, async () => {
    await fixture(async ({ directory, put }) => {
      await put(
        'src/routes/+page.svelte',
        '<svelte:head><script src="/tests/provider.js"></script></svelte:head>'
      );
      const result = spawnSync('corepack', ['pnpm', 'run', script], {
        cwd: directory,
        encoding: 'utf8',
        timeout: 8000
      });
      assert.equal(result.error, undefined);
      assert.equal(result.status, 1, result.stdout + result.stderr);
      assert.match(result.stderr, /raw markup scripts/);
      assert.doesNotMatch(
        result.stdout,
        /svelte-check found|vite v.*building|RUN\s+v/
      );
    });
  });

for (const source of [
  'const script = document.createElement("script");',
  'const stylesheet = document.createElement("link");',
  'const tag = "script"; const script = document.createElement(tag);',
  'const script = document.createElementNS("http://www.w3.org/2000/svg", "script");',
  'document.body.innerHTML = "<script src=/tests/provider.js></script>";',
  'document.body.insertAdjacentHTML("beforeend", "<style>body{color:red}</style>");',
  'document.body["outerHTML"] = "<iframe srcdoc=unsafe></iframe>";',
  'document.write("<script src=/tests/provider.js></script>");'
])
  test(`JS cannot construct raw resource markup: ${source}`, async () => {
    await fixture(async ({ put, execute }) => {
      await put('src/routes/+page.ts', source);
      reject(execute(), /markup/);
    });
  });

test('supported direct UI focus remains allowed', async () => {
  await fixture(async ({ put, execute }) => {
    await put(
      'src/routes/+page.ts',
      'export function focusLabel() { document.getElementById("label")?.focus(); }'
    );
    const result = execute();
    assert.equal(result.status, 0, result.stderr);
  });
});

for (const source of [
  '/** @type {import("../../tests/signer").Signer} */\nexport const signer = {};',
  '/** @import { Signer } from "../../tests/signer" */\nexport const signer = {};'
])
  test(`JSDoc type edges cannot reach test artifacts: ${source}`, async () => {
    await fixture(async ({ put, execute }) => {
      await put('src/routes/+page.js', source);
      reject(execute(), /forbidden production import/);
    });
  });
test('Svelte const declarations cannot hide client imports', async () => {
  await fixture(async ({ put, execute }) => {
    await put(
      'src/routes/+page.svelte',
      '{#if true}{@const client = import("nostr-tools")}<p>{client}</p>{/if}'
    );
    reject(execute(), /forbidden production import/);
  });
});
for (const source of [
  'const script = document["createElement"]("script");',
  'const create = document.createElement; const script = create("script");',
  'const script = document.createElement.call(document, "script");',
  'const method = "createElement"; const script = document[method]("script");'
])
  test(`indirect DOM constructors cannot load client markup: ${source}`, async () => {
    await fixture(async ({ put, execute }) => {
      await put('src/routes/+page.ts', source);
      reject(execute(), /markup/);
    });
  });

const resourceHandle =
  'const link = document.querySelector<HTMLLinkElement>("link[rel=icon]"); if (!link) throw new Error("missing");';
for (const mutation of [
  'link.rel = "stylesheet"; link.href = "https://example.invalid/extra.css";',
  'link["rel"] = "stylesheet"; link["href"] = "/extra.css";',
  'const field = "href"; link[field] = "/tests/provider.js";',
  'link.setAttribute("rel", "stylesheet"); link.setAttribute("href", "/extra.css");',
  'link.setAttributeNS(null, "href", "/extra.css");',
  'link.toggleAttribute("rel", true);',
  'link.relList.add("stylesheet");',
  'const alias = link; alias.href = "../../../core/native";',
  'const setter = link.setAttribute; setter.call(link, "href", "/extra.css");',
  'link.setAttribute.bind(link)("href", "/extra.css");',
  'Object.assign(link, { rel: "stylesheet", href: "/extra.css" });',
  'Object.defineProperty(link, "href", { value: "/extra.css" });',
  'Object.defineProperties(link, { href: { value: "/extra.css" } });',
  'Reflect.set(link, "href", "/extra.css");',
  'Reflect.defineProperty(link, "href", { value: "/extra.css" });',
  'const patch = Object.assign; patch(link, { href: "/extra.css" });',
  'const attribute = link.getAttributeNode("href"); if(attribute) attribute.value = "/extra.css";',
  'const attribute = document.createAttribute("href"); attribute.value = "/extra.css"; link.setAttributeNode(attribute);',
  'link.attributes.setNamedItem(document.createAttribute("href"));',
  'link.sheet?.replaceSync("@import /tests/style.css");',
  'delete link.rel;',
  '[link.href] = ["/extra.css"];'
])
  test(`opaque resource mutations fail closed: ${mutation}`, async () => {
    await fixture(async ({ put, execute }) => {
      await put('src/routes/+page.ts', resourceHandle + mutation);
      reject(execute(), /DOM|mutation|reflection|model/);
    });
  });

for (const source of [
  'export function rewrite(opaque: unknown) { (opaque as { href: string }).href = "/tests/provider.js"; }',
  'const model = { href: "/safe" }; function rewrite(model: { href: string }) { model.href = "/tests/provider.js"; }',
  'const model = { href: "/safe" }; const shadow = (model: { href: string }) => Object.assign(model, { href: "/extra.css" });',
  'const reflect = Reflect; reflect.set(window.document, "adoptedStyleSheets", []);',
  'const { document: dom } = window; const link = dom.querySelector("link"); link.href = "/extra.css";',
  'const selector = document.querySelector; const link = selector("link"); link.href = "/extra.css";',
  'const capability = (event: Event) => { const link = event.currentTarget; Object.assign(link, { rel: "stylesheet" }); };',
  'const data = { link: { href: "/safe" } }; data.link = (window as unknown as { evil: { href: string } }).evil;',
  'const prototype = ({}).constructor.constructor; prototype("return document")();'
])
  test(`unmanaged or indirect mutation capabilities fail closed: ${source}`, async () => {
    await fixture(async ({ put, execute }) => {
      await put('src/routes/+page.ts', source);
      reject(execute(), /DOM|capability|reflection|model/);
    });
  });

for (const markup of [
  '<link bind:this={link} rel="icon" href="/favicon.ico" />',
  '<div use:mutate>content</div>',
  '<div {@attach mutate}>content</div>',
  '<div transition:mutate>content</div>'
])
  test(`markup cannot expose unmanaged DOM handles: ${markup}`, async () => {
    await fixture(async ({ put, execute }) => {
      await put(
        'src/routes/+page.svelte',
        '<script lang="ts">let link; const mutate = (node) => node;</script>' +
          markup
      );
      reject(execute(), /DOM bindings\/actions\/attachments/);
    });
  });

test('owned plain model mutations, lexical aliases, reflection and UI leaf operations remain admitted', async () => {
  await fixture(async ({ put, execute }) => {
    await put(
      'src/routes/+page.ts',
      'const model = { href: "/home", rel: "icon", counter: 0, style: { color: "description" } }; const alias = model; alias.href = "/search"; model["rel"] = "metadata"; model.counter++; Object.assign(alias, { href: "/sell" }); Reflect.set(model, "rel", "model"); Object.defineProperty(model, "label", { value: "HarvestCircle" }); model.style.color = "plain data"; export function focus() { document.querySelector("#subject")?.focus(); } export function read() { return document.getElementById("subject")?.textContent; }'
    );
    const result = execute();
    assert.equal(result.status, 0, result.stderr);
  });
});

for (const source of [
  'import { createRawSnippet } from "svelte"; export const snippet = createRawSnippet(() => ({ render: () => "<link rel=stylesheet href=/extra.css>" }));',
  'import { createRawSnippet as raw } from "svelte"; export const snippet = raw(() => ({ render: () => "<script src=/tests/provider.js></script>" }));',
  'import * as renderer from "svelte"; export const snippet = renderer["create" + "RawSnippet"](() => ({ render: () => "<style>body{color:red}</style>" }));',
  'export * from "svelte";',
  'export { createRawSnippet as raw } from "svelte";',
  'const renderer = await import("svelte"); export const snippet = renderer.createRawSnippet;',
  'import { render } from "svelte/server"; export { render };'
])
  test(`pinned Svelte raw rendering remains unavailable: ${source}`, async () => {
    await fixture(async ({ directory, put, execute }) => {
      await confineSvelte(directory);
      await put('src/routes/+page.ts', source);
      reject(
        execute(),
        /createRawSnippet|raw-render|forbidden production import svelte\/server/
      );
    });
  });

test('actual pinned safe Svelte imports remain available', async () => {
  await fixture(async ({ directory, put, execute }) => {
    await confineSvelte(directory);
    await put(
      'src/routes/+page.ts',
      'import { onMount } from "svelte"; import type { Snippet } from "svelte"; export { onMount }; export type { Snippet };'
    );
    const result = execute();
    assert.equal(result.status, 0, result.stderr);
  });
});

for (const mutation of [
  'opaque.rel = "stylesheet";',
  'opaque["href"] = "/extra.css";',
  'opaque.setAttribute("rel", "stylesheet");',
  'const setter = opaque.setAttribute; setter.apply(opaque, ["href", "/extra.css"]);',
  'Object.assign(opaque, { rel: "stylesheet" });',
  'const attribute = opaque.attributes.item(0); attribute.nodeValue = "/extra.css";',
  'opaque.relList.add("stylesheet");',
  'Reflect.set(opaque, "href", "/extra.css");'
])
  test(`mutation grammar rejects opaque inputs without DOM acquisition: ${mutation}`, async () => {
    await fixture(async ({ put, execute }) => {
      await put(
        'src/routes/+page.ts',
        'export function mutate(opaque: unknown) { ' +
          mutation.replaceAll(
            'opaque.',
            '(opaque as Record<string, unknown>).'
          ) +
          ' }'
      );
      reject(execute(), /DOM|mutation|reflection|model/);
    });
  });

test('string-named raw-render exports cannot bypass semantic import admission', async () => {
  await fixture(async ({ directory, put, execute }) => {
    await confineSvelte(directory);
    await put(
      'src/routes/+page.ts',
      'import { "createRawSnippet" as raw } from "svelte"; export { raw };'
    );
    reject(execute(), /raw-render factories/);
  });
});

for (const source of [
  'new Worker("https://example.invalid/client.js");',
  'new SharedWorker("/tests/client.js");',
  'importScripts("https://example.invalid/client.js");',
  'navigator.serviceWorker.register("/tests/client.js");',
  'const capabilities = navigator; capabilities["service" + "Worker"].register("/tests/client.js");',
  'window.navigator["service" + "Worker"].register("/tests/client.js");',
  'const registrations = window.navigator.serviceWorker; registrations.register("/tests/client.js");',
  'const audio = new AudioContext(); audio.audioWorklet.addModule("/tests/client.js");'
])
  test(`browser worker loading cannot bypass source imports: ${source}`, async () => {
    await fixture(async ({ put, execute }) => {
      await put('src/routes/+page.ts', source);
      reject(execute(), /loader|worker|DOM|capability/);
    });
  });

test('approved navigator capabilities and plain model flags remain admitted', async () => {
  await fixture(async ({ put, execute }) => {
    await put(
      'src/routes/+page.ts',
      'const model = { serviceWorker: false }; export const disabled = model.serviceWorker; export function online() { return navigator.onLine; } export function estimate() { return navigator.storage.estimate(); } export function copy(value: string) { return window.navigator.clipboard.writeText(value); }'
    );
    const result = execute();
    assert.equal(result.status, 0, result.stderr);
  });
});

const helperImport =
  "import { internalHref, navigationHref } from '../lib/navigation-url';";
for (const [
  name,
  code,
  accepted
] of /** @type {Array<[string,string,boolean]>} */ ([
  ['concat executable', "<a href={'java'+'script:alert(1)'}>bad</a>", false],
  [
    'shorthand executable',
    "<script>const href='javascript:alert(1)';</script><a {href}>bad</a>",
    false
  ],
  [
    'mixed executable',
    '<script>const scheme=\'java\';</script><a href="{scheme}script:alert(1)">bad</a>',
    false
  ],
  [
    'conditional executable',
    "<a href={true ? '/safe' : 'javascript:alert(1)'}>bad</a>",
    false
  ],
  [
    'alias executable',
    "<script>const a='javascript:alert(1)'; const href=a;</script><a {href}>bad</a>",
    false
  ],
  [
    'safe aliases',
    "<script>const a='/search'; const href=a+'?q=food';</script><a {href}>safe</a>",
    true
  ],
  [
    'safe template',
    "<script>const id='example';</script><a href={`/products/${id}`}>safe</a>",
    true
  ],
  [
    'wrapped route',
    `<script lang="ts">${helperImport} const route=(id: string) => '/products/'+id; const id='example';</script><a href={internalHref(route(id))}>safe</a>`,
    true
  ],
  [
    'wrapped contact',
    `<script lang="ts">${helperImport} const contact: unknown='https://example.com';</script><a href={navigationHref(contact)}>safe</a>`,
    true
  ],
  [
    'forged spelling',
    "<script>const internalHref=x=>x;</script><a href={internalHref('javascript:alert(1)')}>bad</a>",
    false
  ],
  [
    'each shadows helper',
    `<script>${helperImport} const funcs=[x=>x];</script>{#each funcs as internalHref}<a href={internalHref('javascript:alert(1)')}>bad</a>{/each}`,
    false
  ],
  [
    'snippet shadows helper',
    `<script>${helperImport}</script>{#snippet row(internalHref)}<a href={internalHref('javascript:alert(1)')}>bad</a>{/snippet}`,
    false
  ],
  [
    'const shadows helper',
    `<script>${helperImport}</script>{#if true}{@const internalHref = x=>x}<a href={internalHref('javascript:alert(1)')}>bad</a>{/if}`,
    false
  ],
  [
    'module scope helper',
    `<script module>${helperImport}</script><a href={internalHref('/products/a')}>safe</a>`,
    true
  ],
  [
    'instance shadows module',
    `<script module>${helperImport}</script><script>const internalHref=x=>x;</script><a href={internalHref('javascript:alert(1)')}>bad</a>`,
    false
  ],
  [
    'module sees no instance helper',
    `<script module>export const wrong=internalHref('javascript:alert(1)');</script><script>${helperImport}</script><a href={wrong}>bad</a>`,
    false
  ],
  [
    'unknown prop',
    '<script>let {href}=$props();</script><a {href}>bad</a>',
    false
  ],
  [
    'cast prop',
    '<script lang="ts">let {href}: {href:string}=$props();</script><a href={href as string}>bad</a>',
    false
  ],
  [
    'resource validator cannot admit remote image',
    `<script>${helperImport}</script><img src={navigationHref('https://example.com/image.png')} alt="bad"/>`,
    false
  ],
  [
    'remote image',
    '<img src="https://example.com/image.png" alt="bad"/>',
    false
  ],
  [
    'remote srcset',
    '<img srcset="https://example.com/image.png 2x" alt="bad"/>',
    false
  ],
  [
    'remote poster',
    '<video poster="https://example.com/image.png"></video>',
    false
  ],
  ['form computed', "<form action={'java'+'script:alert(1)'}></form>", false],
  ['URL control', '<a href="https://user:secret@example.com">bad</a>', false],
  ['backslash', '<a href="/\\evil.test">bad</a>', false],
  [
    'namespace SVG',
    '<svg><a xlink:href="javascript:alert(1)">bad</a></svg>',
    false
  ]
])) {
  test(`D07 markup: ${name}`, async () =>
    fixture(async ({ put, execute }) => {
      await put('src/routes/+page.svelte', code);
      const result = execute();
      assert.equal(
        result.status,
        accepted ? 0 : 1,
        result.stdout + result.stderr
      );
    }));
}

for (const [
  name,
  code,
  accepted
] of /** @type {Array<[string,string,boolean]>} */ ([
  ['Location literal', "location.assign('/search');", true],
  ['Location qualified literal', "window.location.replace('/search');", true],
  ['Location href literal', "location.href='/search';", true],
  ['Location write literal', "window.location='/search';", true],
  ['Location executable', "location.assign('javascript:alert(1)');", false],
  ['Location external', "location.assign('https://example.com');", false],
  [
    'Location escaped',
    "const target=location; target.assign('/search');",
    false
  ],
  [
    'Location destructured',
    "const {assign}=location; assign('/search');",
    false
  ],
  ['Location computed', "location['assign']('/search');", false],
  [
    'validated Location',
    `${helperImport} const target=internalHref('/search'); if(target !== undefined) location.assign(target);`,
    true
  ],
  [
    'undefined Location',
    `${helperImport} location.assign(internalHref('/search'));`,
    false
  ],
  [
    'asserted Location',
    `${helperImport} location.assign(internalHref('/search')!);`,
    false
  ],
  [
    'Kit literal',
    "import {goto as navigate} from '$app/navigation'; navigate('/search');",
    true
  ],
  [
    'Kit validated',
    `${helperImport} import {goto} from '$app/navigation'; const target=internalHref('/search'); if(target) goto(target);`,
    true
  ],
  [
    'Kit undefined',
    `${helperImport} import {goto} from '$app/navigation'; goto(internalHref('/search'));`,
    false
  ],
  [
    'Kit executable',
    "import {goto} from '$app/navigation'; goto('javascript:alert(1)');",
    false
  ],
  [
    'Kit escaped',
    "import {goto} from '$app/navigation'; const redirect=goto; redirect('javascript:alert(1)');",
    false
  ],
  [
    'callback timer',
    'setTimeout(()=>{},0); setInterval(function(){},100);',
    true
  ],
  [
    'named callback timer',
    'function callback(){} setTimeout(callback,0);',
    true
  ],
  [
    'const callback timer',
    'const callback=()=>{}; setInterval(callback,100);',
    true
  ],
  [
    'shadowed timer ordinary model',
    "const setTimeout=(value:string)=>value; setTimeout('data');",
    true
  ],
  ['string timer', "setTimeout('alert(1)',0);", false],
  ['unknown timer', 'setTimeout(callback,0);', false],
  ['escaped timer', "const later=setTimeout; later('alert(1)',0);", false],
  ['qualified timer', 'window.setTimeout(()=>{},0);', false],
  ['computed timer', 'globalThis["setTimeout"](()=>{},0);', false],
  [
    'destructured timer',
    'const {setTimeout: later}=window; later(()=>{},0);',
    false
  ],
  [
    'reassigned callback',
    'let cb=()=>{}; cb=external; setTimeout(cb,0);',
    false
  ],
  [
    'parameter callback unsupported',
    'function schedule(cb:()=>void){setTimeout(cb,0)}',
    false
  ],
  [
    'literal replace',
    "const value='one two'; export const normalized=value.replace(/\\s+/g,' ');",
    true
  ],
  [
    'alias replace',
    "const value='one two'; const alias=value; alias.replace('two','three');",
    true
  ],
  [
    'fake replace',
    "const value=external as string; value.replace('two','three');",
    false
  ],
  [
    'DOM replace',
    "document.querySelector('link').relList.replace('icon','stylesheet');",
    false
  ],
  ['local Set', "const seen=new Set<string>(); seen.add('public-id');", true],
  [
    'shadowed Set',
    "function f(Set:any){const seen=new Set();seen.add('x')}",
    false
  ],
  [
    'Set alias',
    "const seen=new Set(); const other=seen; other.add('x');",
    false
  ],
  ['Set rebound', "let seen=new Set();seen=external;seen.add('x');", false],
  [
    'Set escaped add',
    "const seen=new Set(); const add=seen.add; add('x');",
    false
  ],
  ['opaque add', "(external as Set<string>).add('x');", false],
  [
    'DOM add',
    "document.querySelector('link').relList.add('stylesheet');",
    false
  ]
])) {
  test(`D07 JavaScript: ${name}`, async () =>
    fixture(async ({ put, execute }) => {
      await put('src/routes/+page.ts', code);
      const result = execute();
      assert.equal(
        result.status,
        accepted ? 0 : 1,
        result.stdout + result.stderr
      );
    }));
}

test('D07 helper runtime validates unknown values without effects or coercion', async () => {
  const { internalHref, navigationHref } =
    await import('../../src/lib/navigation-url.ts');
  const evil = {
    toString() {
      throw new Error('coercion');
    }
  };
  for (const value of [
    evil,
    null,
    undefined,
    5,
    [],
    {},
    '',
    '//evil.test',
    '/\\evil.test',
    'javascript:alert(1)',
    'data:text/html,x',
    'https://u:p@example.com',
    'https://@example.com',
    'https:////@example.com',
    'https:///@example.com',
    'https:////example.com',
    'mailto://evil.example/path',
    'tel://evil.example/path',
    '\ud800',
    '/foo\nbar',
    '/%0d%0aevil',
    '/%5cevil',
    '/%zz',
    'a'.repeat(8193)
  ]) {
    assert.equal(internalHref(value), undefined);
    assert.equal(navigationHref(value), undefined);
  }
  for (const value of [
    '/search?q=food',
    '#item',
    '?query=food',
    '/products/example'
  ]) {
    assert.equal(internalHref(value), value);
    assert.equal(navigationHref(value), value);
  }
  for (const value of [
    'https://example.com/path',
    'HTTPS://example.com/path',
    'https://example.com/@path?email=buyer@example.com',
    'mailto:buyer@example.com',
    'tel:+123456789'
  ]) {
    assert.equal(internalHref(value), undefined);
    assert.equal(navigationHref(value), value);
  }
  assert.equal(internalHref('/' + 'é'.repeat(4095)), '/' + 'é'.repeat(4095));
  assert.equal(internalHref('/' + 'é'.repeat(4096)), undefined);
  assert.equal(internalHref('/' + 'a'.repeat(8191)), '/' + 'a'.repeat(8191));
  let mutable = '/safe';
  assert.equal(internalHref(mutable), '/safe');
  mutable = 'javascript:alert(1)';
  assert.equal(internalHref(mutable), undefined);
});

test('D07 helper pin rejects source tampering and spoofed module exports', async () =>
  fixture(async ({ put, execute }) => {
    await put(
      'src/lib/navigation-url.ts',
      'export const internalHref=(value:unknown)=>value;'
    );
    await put(
      'src/routes/+page.svelte',
      `<script>${helperImport}</script><a href={internalHref('/safe')}>bad</a>`
    );
    reject(execute(), /source identity/);
  }));

test('D07 alias and named re-export retain exact helper export identity', async () =>
  fixture(async ({ put, execute }) => {
    await put(
      'src/lib/links.ts',
      "export {internalHref as routeHref} from './navigation-url';"
    );
    await put(
      'src/routes/+page.svelte',
      "<script>import {routeHref as validated} from '../lib/links'; const unknownRoute='/search';</script><a href={validated(unknownRoute)}>safe</a>"
    );
    assert.equal(execute().status, 0);
  }));

test('D07 positives compile with actual Svelte and TypeScript', async () =>
  fixture(async ({ put, execute, directory }) => {
    const { compile } = await import('svelte/compiler');
    const source = `<script lang="ts">${helperImport} let {href}: {href: unknown} = $props(); const route=(id:string)=>'/products/'+id;</script><a href={navigationHref(href)}>contact</a><a href={internalHref(route('example'))}>product</a>`;
    await put('src/routes/+page.svelte', source);
    assert.equal(execute().status, 0);
    const output = compile(source, {
      filename: path.join(directory, 'src/routes/+page.svelte'),
      generate: 'server'
    });
    assert.ok(output.js.code.length > 0);
    assert.deepEqual(output.warnings, []);
    const { default: ts } = await import('typescript');
    const navigation = `import {internalHref} from './src/lib/navigation-url'; import {goto} from '$app/navigation'; export function search(input:unknown){const target=internalHref(input); if(target !== undefined) void goto(target);} export const normalize='one two'.replace(/\\s+/g,' '); const seen=new Set<string>();seen.add('id'); setTimeout(()=>seen.add('next'),0);`;
    await put('positive.ts', navigation);
    const program = ts.createProgram([path.join(directory, 'positive.ts')], {
      strict: true,
      noEmit: true,
      target: ts.ScriptTarget.ESNext,
      module: ts.ModuleKind.ESNext,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
      skipLibCheck: true,
      types: ['@sveltejs/kit']
    });
    assert.deepEqual(
      ts
        .getPreEmitDiagnostics(program)
        .map((d) => ts.flattenDiagnosticMessageText(d.messageText, '\n')),
      []
    );
  }));

for (const [
  name,
  code,
  accepted
] of /** @type {Array<[string,string,boolean]>} */ ([
  [
    'namespace helper',
    "<script>import * as links from '../lib/navigation-url'; const route='/safe';</script><a href={links.internalHref(route)}>safe</a>",
    true
  ],
  [
    'await shadows helper',
    `<script>${helperImport} const pending=Promise.resolve(x=>x);</script>{#await pending then internalHref}<a href={internalHref('javascript:alert(1)')}>bad</a>{/await}`,
    false
  ],
  [
    'reassigned helper',
    `<script>${helperImport} internalHref=x=>x;</script><a href={internalHref('javascript:alert(1)')}>bad</a>`,
    false
  ],
  [
    'spoofed import',
    "<script>import {internalHref} from '../lib/spoof';</script><a href={internalHref('javascript:alert(1)')}>bad</a>",
    false
  ],
  [
    'namespace escape',
    "<script>import * as nav from '$app/navigation'; const copy=nav; copy.goto('javascript:alert(1)');</script>",
    false
  ],
  [
    'namespace Kit navigation',
    "<script>import * as nav from '$app/navigation'; const go=()=>nav.goto('/search');</script><button onclick={go}>Search</button>",
    true
  ],
  [
    'reassigned Kit capability',
    "<script>import {goto} from '$app/navigation'; goto=x=>x; goto('javascript:alert(1)');</script>",
    false
  ]
]))
  test(`D07 binding identity: ${name}`, async () =>
    fixture(async ({ put, execute }) => {
      await put(
        'src/lib/spoof.ts',
        'export const internalHref=(value:unknown)=>value;'
      );
      await put('src/routes/+page.svelte', code);
      const result = execute();
      assert.equal(result.status, accepted ? 0 : 1, result.stderr);
    }));

for (const [code, accepted] of /** @type {Array<[string,boolean]>} */ ([
  [
    "const text='one two';const normalized=text.replace(/\\s+/g,' ');normalized.replace('two','three');",
    true
  ],
  [
    "const seen=new Set<string>(); seen.add('id'); const count=seen.size; const exists=seen.has('id');",
    true
  ],
  ["const seen=new Set<string>(); external(seen); seen.add('id');", false],
  ['const seen=new Set<string>(); export const leak=seen;', false],
  ["const seen=new Set<string>(); const read=seen.has; read('id');", false]
]))
  test(`D07 concrete receivers: ${code}`, async () =>
    fixture(async ({ put, execute }) => {
      await put('src/routes/+page.ts', code);
      const result = execute();
      assert.equal(result.status, accepted ? 0 : 1, result.stderr);
    }));

test('D07 package import alias retains helper identity without evaluating configuration', async () =>
  fixture(async ({ put, execute, pkg }) => {
    pkg.imports = { '#links': './src/lib/navigation-url.ts' };
    await put('package.json', JSON.stringify(pkg));
    await put(
      'src/routes/+page.svelte',
      "<script>import {internalHref as validated} from '#links';const input='/safe';</script><a href={validated(input)}>safe</a>"
    );
    assert.equal(execute().status, 0);
  }));

for (const command of ['check', 'lint', 'build', 'test:unit', 'dev'])
  test(`D07 actual ${command} stops before framework dispatch on computed executable URL`, async () =>
    fixture(async ({ put, directory }) => {
      await put(
        'src/routes/+page.svelte',
        "<a href={'java'+'script:import(\"nostr-tools\")'}>bad</a>"
      );
      const result = spawnSync('corepack', ['pnpm', 'run', command], {
        cwd: directory,
        encoding: 'utf8',
        timeout: 8000
      });
      assert.equal(result.error, undefined);
      assert.equal(result.status, 1, result.stdout + result.stderr);
      assert.match(result.stderr, /unsafe markup URL/);
      assert.doesNotMatch(
        result.stdout,
        /svelte-check found|vite v.*building|RUN\s+v/
      );
    }));

test('D07 component props are validated at the actual element sink and compile', async () =>
  fixture(async ({ put, execute, directory }) => {
    const { compile } = await import('svelte/compiler');
    const component =
      '<script lang="ts">import {navigationHref} from \'./navigation-url\'; let {href}: {href:unknown}=$props();</script><a href={navigationHref(href)}>Contact</a>';
    const page =
      "<script lang=\"ts\">import Link from '../lib/Link.svelte';const contact:unknown='https://example.com';</script><Link href={contact}/>";
    await put('src/lib/Link.svelte', component);
    await put('src/routes/+page.svelte', page);
    assert.equal(execute().status, 0);
    for (const [source, name] of [
      [component, 'src/lib/Link.svelte'],
      [page, 'src/routes/+page.svelte']
    ]) {
      const compiled = compile(source, {
        filename: path.join(directory, name),
        generate: 'server'
      });
      assert.ok(compiled.js.code.length);
      assert.deepEqual(compiled.warnings, []);
    }
    await put(
      'src/lib/Link.svelte',
      '<script lang="ts">let {href}: {href:string}=$props();</script><a {href}>Bad</a>'
    );
    reject(execute(), /unsafe markup URL/);
  }));

test('D07 helper byte pin includes the leading UTF8 BOM', async () =>
  fixture(async ({ put, execute }) => {
    await put(
      'src/lib/navigation-url.ts',
      '\ufeff' +
        (await readFile(path.join(root, 'src/lib/navigation-url.ts'), 'utf8'))
    );
    reject(execute(), /source identity/);
  }));

for (const producer of [
  "import {goto} from '$app/navigation'; export {goto};",
  "import {goto as imported} from '$app/navigation'; export {imported as redirected};",
  "import * as navigation from '$app/navigation'; export {navigation};",
  "import {goto} from '$app/navigation'; export default goto;"
])
  test(`D07 local Kit exports cannot launder navigation: ${producer}`, async () =>
    fixture(async ({ put, execute }) => {
      await put('src/lib/escaped.ts', producer);
      await put(
        'src/routes/+page.ts',
        "import {goto as redirected} from '../lib/escaped'; redirected('javascript:alert(1)');"
      );
      reject(execute(), /escaped (goto capability|navigation namespace)/);
    }));

test('D07 named module Kit reexports retain sink identity for actual consumers', async () =>
  fixture(async ({ put, execute }) => {
    await put(
      'src/lib/navigation.ts',
      "export {goto as navigate} from '$app/navigation';"
    );
    await put(
      'src/routes/+page.ts',
      "import {navigate} from '../lib/navigation'; navigate('/search');"
    );
    assert.equal(execute().status, 0);
    await put(
      'src/routes/+page.ts',
      "import {navigate} from '../lib/navigation'; navigate('javascript:alert(1)');"
    );
    reject(execute(), /defined validated internal URL/);
  }));

test('D07 local helper reexports are untrusted until explicitly resolved at a sink', async () =>
  fixture(async ({ put, execute }) => {
    await put(
      'src/lib/indirect.ts',
      "import {internalHref} from './navigation-url'; export {internalHref as validated};"
    );
    await put(
      'src/routes/+page.svelte',
      "<script>import {validated} from '../lib/indirect';const route='/safe';</script><a href={validated(route)}>Unknown binding</a>"
    );
    reject(execute(), /unsafe markup URL/);
  }));

test('D07 literal Svelte element tags share URL context admission and compile', async () =>
  fixture(async ({ put, execute, directory }) => {
    const { compile } = await import('svelte/compiler');
    const positive =
      "<svelte:element this={'a'} href='https://example.com'>Contact</svelte:element>";
    await put('src/routes/+page.svelte', positive);
    assert.equal(execute().status, 0);
    const compiled = compile(positive, {
      filename: path.join(directory, 'src/routes/+page.svelte'),
      generate: 'server'
    });
    assert.ok(compiled.js.code.length);
    assert.deepEqual(compiled.warnings, []);
    for (const source of [
      "<svelte:element this='a' href={'java'+'script:alert(1)'}>Bad</svelte:element>",
      "<svelte:element this='img' src='https://example.com/image.png' alt='Bad'/>",
      "<svelte:element this='image' href='https://example.com/image.png'/>",
      "<script>const tag='a';</script><svelte:element this={tag} href='https://example.com'>Unsupported</svelte:element>"
    ]) {
      await put('src/routes/+page.svelte', source);
      reject(execute(), /unsafe markup URL|computed markup resource elements/);
    }
  }));

for (const barrel of [
  "export * from './bridge';",
  "export * as navigation from './bridge';"
])
  test(`D07 Kit capability cannot escape renamed bridge through ${barrel}`, async () =>
    fixture(async ({ put, execute }) => {
      await put(
        'src/lib/bridge.ts',
        "export {goto as redirect} from '$app/navigation';"
      );
      await put('src/lib/barrel.ts', barrel);
      await put(
        'src/routes/+page.ts',
        "import {redirect} from '../lib/barrel';redirect('javascript:alert(1)');"
      );
      reject(execute(), /navigation star\/namespace reexport/);
    }));

test('D07 renamed Kit namespace aliases cannot escape actual binding identity', async () =>
  fixture(async ({ put, execute }) => {
    await put(
      'src/lib/bridge.ts',
      "export {goto as redirect} from '$app/navigation';"
    );
    await put(
      'src/routes/+page.ts',
      "import * as navigation from '../lib/bridge'; const escaped=navigation;escaped.redirect('javascript:alert(1)');"
    );
    reject(execute(), /escaped navigation namespace/);
  }));

for (const source of [
  "location.assign(setTimeout('alert(1)',0) ? '/one' : '/two');",
  "import {goto} from '$app/navigation'; location.assign(goto('javascript:alert(1)') ? '/one' : '/two');",
  "window.location.href=setInterval('alert(1)',100) ? '/one' : '/two';"
])
  test(`D07 accepted Location URL does not hide hostile expression descendants: ${source}`, async () =>
    fixture(async ({ put, execute }) => {
      await put('src/routes/+page.ts', source);
      reject(
        execute(),
        /direct function callback|defined validated internal URL/
      );
    }));

test('D07 Location conditional callback expressions remain admitted with complete descendant audit', async () =>
  fixture(async ({ put, execute }) => {
    await put(
      'src/routes/+page.ts',
      "location.assign(setTimeout(()=>{},0) ? '/one' : '/two'); window.location.replace('/search'); window.location.href='/search';"
    );
    const result = execute();
    assert.equal(result.status, 0, result.stderr);
  }));

for (const source of [
  "Set = fake; const seen=new Set(); seen.add('id');",
  "Set ||= fake; const seen=new Set(); seen.add('id');",
  "Set = fake; const seen=new Set(); const read=seen.has; read('id');"
])
  test(`D07 global collection constructor rebinding is untrusted: ${source}`, async () =>
    fixture(async ({ put, execute }) => {
      await put('src/routes/+page.ts', source);
      const result = execute();
      assert.equal(result.status, 1, result.stdout + result.stderr);
    }));

for (const href of [
  'https:////@example.com',
  'https:///@example.com',
  'https:////example.com'
])
  test(`D07 malformed HTTPS authority cannot normalize away credentials: ${href}`, async () =>
    fixture(async ({ put, execute }) => {
      await put(
        'src/routes/+page.svelte',
        `<a href=${JSON.stringify(href)}>Bad</a>`
      );
      reject(execute(), /unsafe markup URL/);
    }));

const smilMutation =
  "<svg width='300' height='100'><a id='link' href='/safe'><text x='10' y='50'>Click</text></a><set href='#link' attributeName='href' to='javascript:window.__smilExecuted=true' begin='0s' fill='freeze'/></svg>";
for (const tag of [
  'set',
  'animate',
  'animateMotion',
  'animateTransform',
  'discard',
  'SET',
  'svg:animate',
  'svg:animateTransform'
])
  test(`D07 declarative SVG mutation tag is rejected: ${tag}`, async () =>
    fixture(async ({ put, execute }) => {
      await put(
        'src/routes/+page.svelte',
        `<svg><${tag} href='#link' attributeName='href' to='javascript:alert(1)'/></svg>`
      );
      reject(execute(), /declarative SVG resource\/style mutation/);
    }));

for (const tag of [
  'set',
  'animate',
  'animateMotion',
  'animateTransform',
  'discard',
  'SVG:SET'
])
  test(`D07 literal Svelte element cannot construct SVG mutation: ${tag}`, async () =>
    fixture(async ({ put, execute }) => {
      await put(
        'src/routes/+page.svelte',
        `<svg><svelte:element this={${JSON.stringify(tag)}} href='#link' attributeName='href' to='javascript:alert(1)'/></svg>`
      );
      reject(execute(), /declarative SVG resource\/style mutation/);
    }));

test('D07 actual SMIL href mutation fixture rejects while static SVG compiles', async () =>
  fixture(async ({ put, execute, directory }) => {
    await put('src/routes/+page.svelte', smilMutation);
    reject(execute(), /declarative SVG resource\/style mutation/);
    const source =
      "<svg viewBox='0 0 100 100' role='img' aria-label='Static graphic'><path d='M0 0 L20 20'/><a href='/safe'><text x='10' y='50'>Safe link</text></a></svg>";
    await put('src/routes/+page.svelte', source);
    assert.equal(execute().status, 0);
    const { compile } = await import('svelte/compiler');
    const compiled = compile(source, {
      filename: path.join(directory, 'src/routes/+page.svelte'),
      generate: 'server'
    });
    assert.ok(compiled.js.code.length);
    assert.deepEqual(compiled.warnings, []);
  }));

for (const source of [
  'export function motion(element:Element){element.animate([{opacity:0},{opacity:1}],100);}',
  'export function motion(element:Element){const animate=element.animate;animate.call(element,[{opacity:0},{opacity:1}],100);}',
  "export function motion(element:Element){const key='animate';const escaped=element[key];escaped([{opacity:0},{opacity:1}],100);}",
  "export function motion(element:Element){const escaped=element['ani'+'mate'];escaped([{opacity:0},{opacity:1}],100);}",
  "export function motion(element:Element,flag:boolean){const key=flag?'animate':'setKeyframes';const escaped=element[key];escaped([{opacity:0},{opacity:1}],100);}",
  'export function motion(effect:KeyframeEffect){effect.setKeyframes([{opacity:0},{opacity:1}]);}',
  'export function motion(effect:KeyframeEffect){const update=effect.setKeyframes;update([{opacity:0},{opacity:1}]);}',
  'export function motion(element:Element){new KeyframeEffect(element,[{opacity:0},{opacity:1}],100);}',
  'const Effect=KeyframeEffect;new Effect(opaque,[{opacity:0},{opacity:1}],100);',
  'new Animation(opaque);',
  'const CreateAnimation=Animation;new CreateAnimation(opaque);',
  "document.createElementNS('http://www.w3.org/2000/svg','set');"
])
  test(`D07 imperative style/animation capability is rejected: ${source}`, async () =>
    fixture(async ({ put, execute }) => {
      await put('src/routes/+page.ts', source);
      const result = execute();
      assert.equal(result.status, 1, result.stdout + result.stderr);
      assert.match(
        result.stderr,
        /mutation\/setter|animation constructors|SVG resource\/style constructors|DOM handles/
      );
    }));

test('D07 animation names remain ordinary owned model data', async () =>
  fixture(async ({ put, execute }) => {
    await put(
      'src/routes/+page.ts',
      "const model={animate:false,setKeyframes:0}; model.animate=true;model.setKeyframes+=1; export const flag=model.animate;export const revision=model.setKeyframes; function Animation(value:string){return value} export const label=Animation('ordinary model');"
    );
    const result = execute();
    assert.equal(result.status, 0, result.stderr);
  }));

for (const command of ['check', 'lint', 'build', 'test:unit', 'dev'])
  test(`D07 actual ${command} stops before framework dispatch on SVG URL mutation`, async () =>
    fixture(async ({ put, directory }) => {
      await put('src/routes/+page.svelte', smilMutation);
      const result = spawnSync('corepack', ['pnpm', 'run', command], {
        cwd: directory,
        encoding: 'utf8',
        timeout: 8000
      });
      assert.equal(result.error, undefined);
      assert.equal(result.status, 1, result.stdout + result.stderr);
      assert.match(result.stderr, /declarative SVG resource\/style mutation/);
      assert.doesNotMatch(
        result.stdout,
        /svelte-check found|vite v.*building|RUN\s+v/
      );
    }));
