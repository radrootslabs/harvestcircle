import assert from 'node:assert/strict';
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
import { setInterval, clearInterval } from 'node:timers';
import { fileURLToPath } from 'node:url';
import { generateBuildInfo } from '../../tools/build-info.mjs';
import { auditOutput } from '../../tools/check-output.mjs';

await test('actual output qualification', { timeout: 30000 }, async (t) => {
  const source = fileURLToPath(new URL('../../../', import.meta.url));
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
    { cwd: path.join(base, 'web'), stdio: 'pipe', timeout: 10000 }
  );

  /** @param {import('node:test').TestContext} t */
  async function fixture(t) {
    const root = await realpath(
      await mkdtemp(path.join(os.tmpdir(), 'hc output mutation '))
    );
    t.after(() => rm(root, { recursive: true, force: true }));
    await cp(base, root, {
      recursive: true,
      filter: (name) => name !== path.join(base, 'web/node_modules')
    });
    const web = path.join(root, 'web');
    return { root, web, audit: () => auditOutput(web) };
  }
  await t.test(
    'actual compiled payload passes and build dispatch invokes the guard last',
    async (t) => {
      const f = await fixture(t);
      const files = await f.audit();
      // Shared unchanged Button now serves both shell and form; CSS stays once-imported.
      assert.equal(files.length, 18);
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
  await t.test(
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
    await t.test('state module fails closed: ' + mutation, async (t) => {
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
  await t.test(
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
    await t.test('Button module fails closed: ' + mutation, async (t) => {
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
    await t.test(`undeclared copied output is rejected: ${name}`, async (t) => {
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
    await t.test(
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
    await t.test(`missing/tampered owned output: ${name}`, async (t) => {
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
    await t.test(`provenance fails closed: ${mutation}`, async (t) => {
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
    await t.test(`unsafe output refuses ${kind} promptly`, async (t) => {
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
  await t.test(
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

  await t.test(
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
    '.svelte-kit/output/prerendered/pages/index.html'
  ]) {
    for (const kind of ['fifo', 'symlink', 'hardlink']) {
      await t.test(
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
  await t.test(
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
  await t.test(
    'forged public output failures do not echo private poisoned bytes',
    async (t) => {
      const f = await fixture(t);
      const marker = 'CONTROLLED_SENSITIVE_DO_NOT_ECHO';
      const file = path.join(f.web, 'static/build-info.json');
      await writeFile(file, marker);
      await assert.rejects(f.audit, (error) => !String(error).includes(marker));
    }
  );
  await t.test(
    'output entry and aggregate-byte bounds reject oversized inventories',
    async (t) => {
      const f = await fixture(t);
      for (let i = 0; i < 513; i++)
        await writeFile(path.join(f.web, 'build', `tiny-${i}.js`), 'x');
      const baseline = process.memoryUsage().arrayBuffers;
      let peak = baseline;
      const timer = setInterval(() => {
        peak = Math.max(peak, process.memoryUsage().arrayBuffers);
      }, 1);
      try {
        await assert.rejects(f.audit, /Unbounded output inventory/);
      } finally {
        clearInterval(timer);
      }
      assert.ok(
        peak - baseline < 64 * 1024 * 1024,
        'Tiny files must not retain a maximum-sized buffer per entry'
      );
      t.diagnostic(`many-small-entry buffer growth: ${peak - baseline} bytes`);
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
  await t.test(
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
        [path.join(base, 'web/node_modules/vite/bin/vite.js'), 'build'],
        { cwd: path.join(base, 'web'), stdio: 'pipe', timeout: 10000 }
      );
      const files = await auditOutput(await realpath(path.join(base, 'web')));
      assert.ok(files.some((name) => name.endsWith('.css')));
    }
  );
});
