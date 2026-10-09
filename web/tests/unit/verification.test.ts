import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ESLint } from 'eslint';
import { describe, expect, it } from 'vitest';

const root = fileURLToPath(new URL('../../', import.meta.url));
const execute = (args: string[]) =>
  spawnSync(process.execPath, args, {
    cwd: root,
    encoding: 'utf8',
    timeout: 1_800_000
  });

describe('verification rejects invalid inputs', () => {
  it('rejects a deliberate TypeScript error through the configured Svelte checker', async () => {
    const directory = await mkdtemp(path.join(root, 'tests', '.type-fixture-'));
    try {
      await writeFile(
        path.join(directory, 'invalid.ts'),
        'export const value: number = "invalid";\n'
      );
      await writeFile(
        path.join(directory, 'invalid.svelte'),
        '<script lang="ts">let value: number = "invalid";</script><p>{value}</p>'
      );
      await writeFile(
        path.join(directory, 'tsconfig.json'),
        JSON.stringify({
          extends: '../../tsconfig.json',
          include: ['invalid.ts', 'invalid.svelte'],
          exclude: []
        })
      );
      const result = execute([
        'node_modules/svelte-check/bin/svelte-check',
        '--tsconfig',
        path.join(directory, 'tsconfig.json'),
        '--fail-on-warnings'
      ]);
      expect(result.error).toBeUndefined();
      expect(result.status).toBe(1);
      expect(result.stdout).toContain('invalid.svelte');
      expect(result.stdout).toContain(
        "Type 'string' is not assignable to type 'number'"
      );
    } finally {
      await rm(directory, { recursive: true });
    }
  }, 1_800_000);

  it('rejects deliberate JavaScript and Svelte lint violations', async () => {
    const eslint = new ESLint({ cwd: root });
    const js = await eslint.lintText('const unused = 1;\n', {
      filePath: 'eslint.config.js'
    });
    expect(
      js[0].messages.some(
        (message) => message.ruleId === '@typescript-eslint/no-unused-vars'
      )
    ).toBe(true);
    const svelte = await eslint.lintText(
      '<main style="color: red">HarvestCircle</main>\n',
      { filePath: 'src/routes/+page.svelte' }
    );
    expect(
      svelte[0].messages.some(
        (message) => message.ruleId === 'svelte/no-inline-styles'
      )
    ).toBe(true);
    const typed = await eslint.lintText(
      '<script lang="ts">Promise.resolve(1);</script><main>HarvestCircle</main>',
      { filePath: 'src/routes/+layout.svelte' }
    );
    expect(
      typed[0].messages.some(
        (message) =>
          message.ruleId === '@typescript-eslint/no-floating-promises'
      )
    ).toBe(true);
  });

  it.each([
    [
      'inline.svelte',
      '<main style="color: red">unsafe style</main>',
      'inline styles'
    ],
    [
      'component.svelte',
      '<main>unsafe style</main><style>main { color: red; }</style>',
      'component CSS'
    ],
    ['outside.css', 'main { color: red; }', 'handwritten CSS'],
    [
      'native.ts',
      "import '../../../core/Cargo.toml';",
      'forbidden production import'
    ],
    [
      'provider.ts',
      "import '../../tests/unit/ssr-effects';",
      'forbidden production import'
    ],
    ['nostr.ts', "import 'applesauce-core';", 'Applesauce imports']
  ])(
    'rejects production boundary violation %s',
    async (name, content, finding) => {
      const directory = await mkdtemp(
        path.join(root, 'src', '.boundary-fixture-')
      );
      try {
        await writeFile(path.join(directory, name), content);
        const result = execute(['tools/check-source.mjs']);
        expect(result.error).toBeUndefined();
        expect(result.status).toBe(1);
        expect(result.stderr).toContain(finding);
      } finally {
        await rm(directory, { recursive: true });
      }
    }
  );

  it('fails a runner selection with zero matching tests', () => {
    const result = execute([
      'node_modules/vitest/vitest.mjs',
      'run',
      '--config',
      'vitest.config.ts',
      'no-matching-hcp007-test'
    ]);
    expect(result.error).toBeUndefined();
    expect(result.status).toBe(1);
    expect(result.stdout + result.stderr).toContain('No test files found');
  });
});
