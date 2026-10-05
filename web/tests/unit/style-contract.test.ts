import { readFileSync } from 'node:fs';
import {
  copyFile,
  mkdir,
  mkdtemp,
  rm,
  symlink,
  writeFile
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { describe, expect, it } from 'vitest';

const root = fileURLToPath(new URL('../../', import.meta.url));
const layout = readFileSync(
  path.join(root, 'src/routes/+layout.svelte'),
  'utf8'
);
const theme = readFileSync(path.join(root, 'src/theme.css'), 'utf8');
const app = () => readFileSync(path.join(root, 'src/app.css'), 'utf8');

describe('finite two-file style contract', () => {
  it('imports theme then compositions once at the root layout', () => {
    expect(
      [...layout.matchAll(/import\s+['"]([^'"]+\.css)['"]/g)].map((m) => m[1])
    ).toEqual(['../theme.css', '../app.css']);
  });
  it('provides the declared finite composition vocabulary and layer order', () => {
    const css = app();
    expect(css).toContain('@layer reset, base, components, utilities;');
    for (const name of [
      'page',
      'page--reading',
      'stack',
      'stack--tight',
      'cluster',
      'navbar',
      'field',
      'label',
      'input',
      'textarea',
      'button',
      'button--primary',
      'button--secondary',
      'border-list',
      'list-row',
      'facts',
      'notice',
      'message',
      'disclosure',
      'text-muted',
      'visually-hidden'
    ]) {
      expect(css).toMatch(new RegExp('\\.' + name + '(?=[\\s:{,.])'));
    }
    expect(css).not.toMatch(
      /@media|@import|url\(|#[0-9a-f]{3,8}\b|\b(?:rgb|hsl)a?\(|(?:^|[;{])\s*--[\w-]+\s*:/im
    );
    const tokens = new Set(
      [...theme.matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1])
    );
    for (const match of css.matchAll(/var\((--[\w-]+)\)/g))
      expect(tokens.has(match[1])).toBe(true);
    expect(css).not.toMatch(/\b\d*\.?\d+(?:px|rem|em|ch|vh|vw)\b/);
  });
});

async function audit(extra: { name: string; content: string }) {
  const directory = await mkdtemp(path.join(tmpdir(), 'hcp012 styles '));
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
    await writeFile(path.join(directory, 'src/theme.css'), theme);
    await writeFile(
      path.join(directory, 'src/app.css'),
      '@layer reset, base, components, utilities;'
    );
    await writeFile(
      path.join(directory, 'src/routes/+layout.svelte'),
      '<script>import "../theme.css"; import "../app.css";</script>'
    );
    await mkdir(path.dirname(path.join(directory, extra.name)), {
      recursive: true
    });
    await writeFile(path.join(directory, extra.name), extra.content);
    const result = spawnSync(process.execPath, ['tools/check-source.mjs'], {
      cwd: directory,
      encoding: 'utf8',
      timeout: 8000
    });
    expect(result.error).toBeUndefined();
    return result;
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

describe('actual source guard rejects forbidden styles', () => {
  it('admits the isolated paired stylesheet positive control', async () => {
    expect(
      (
        await audit({
          name: 'src/routes/+page.svelte',
          content: '<p class="notice">Test</p>'
        })
      ).status
    ).toBe(0);
  });
  it('admits ordinary layered compositions and ignores comments', async () => {
    expect(
      (
        await audit({
          name: 'src/app.css',
          content:
            '/* @tailwind utilities; */ @layer components { .notice { padding: var(--space-2); } }'
        })
      ).status
    ).toBe(0);
  });
  it.each([
    'tailwind',
    'apply',
    'theme',
    'utility',
    'variant',
    'custom-variant',
    'config',
    'plugin',
    'source',
    'reference'
  ])('rejects Tailwind @%s', async (directive) => {
    const result = await audit({
      name: 'src/app.css',
      content: `@${directive} fixture;`
    });
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain('Tailwind CSS directives are forbidden');
  });
  it.each([
    '@\\74 ailwind utilities;',
    '@tai/**/lwind utilities;',
    '.notice { @\\61 pply bg-white; }'
  ])('rejects decoded Tailwind syntax %s', async (content) => {
    const result = await audit({ name: 'src/app.css', content });
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain('Tailwind CSS directives are forbidden');
  });
  it.each([
    [
      'component CSS',
      'src/routes/+page.svelte',
      '<p>Test</p><style>p { color: red; }</style>'
    ],
    [
      'inline attribute',
      'src/routes/+page.svelte',
      '<p style="color: red">Test</p>'
    ],
    [
      'inline directive',
      'src/routes/+page.svelte',
      '<p style:color="red">Test</p>'
    ],
    [
      'CSS in JavaScript',
      'src/routes/+page.ts',
      'export function decorate(element: HTMLElement) { element.style.color = "red"; }'
    ],
    ['Tailwind import', 'src/routes/+page.ts', 'import "tailwindcss";'],
    [
      'Tailwind directives',
      'src/app.css',
      '@tailwind utilities; .notice { @apply bg-white; }'
    ]
  ])('rejects %s', async (_name, name, content) => {
    const result = await audit({ name, content });
    expect(result.status, result.stdout + result.stderr).not.toBe(0);
  });
});
