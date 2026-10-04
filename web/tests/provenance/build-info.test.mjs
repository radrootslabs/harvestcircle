import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  mkdtemp,
  mkdir,
  readFile,
  writeFile,
  rm,
  symlink,
  link
} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { generateBuildInfo } from '../../tools/build-info.mjs';

/** @param {Uint8Array} bytes */
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
/** @param {string} root @param {string[]} args */
function git(root, ...args) {
  return execFileSync('git', ['-C', root, ...args], {
    encoding: 'utf8'
  }).trim();
}
/** @param {import('node:test').TestContext} t */
async function fixture(t) {
  // Actual immutable selection/lock bytes, never a native tool invocation.
  const root = await mkdtemp(path.join(os.tmpdir(), 'hc provenance '));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(path.join(root, 'web/src'), { recursive: true });
  await mkdir(path.join(root, 'core'));
  await writeFile(
    path.join(root, 'web/src/source.ts'),
    'export const value = 1;\n'
  );
  await writeFile(
    path.join(root, 'web/.gitignore'),
    '/static/build-info.json\n/build/\n'
  );
  for (const name of [
    'radroots.lib.source-lock.v1.toml',
    'core/Cargo.lock',
    'web/pnpm-lock.yaml'
  ]) {
    await writeFile(
      path.join(root, name),
      await readFile(new URL('../../../' + name, import.meta.url))
    );
  }
  git(root, 'init', '-q');
  git(root, 'add', '.');
  git(
    root,
    '-c',
    'user.name=Provenance Test',
    '-c',
    'user.email=provenance@example.invalid',
    'commit',
    '-qm',
    'source fixture'
  );
  return {
    root,
    web: path.join(root, 'web'),
    generate: () => generateBuildInfo(path.join(root, 'web'))
  };
}

await test('allowlist, actual selected oracle/locks, clean identity and deterministic generated output', async (t) => {
  const f = await fixture(t);
  const first = await f.generate();
  const bytes = await readFile(path.join(f.web, 'static/build-info.json'));
  assert.deepEqual(Object.keys(first), [
    'web_source',
    'radroots_oracle',
    'dependency_locks'
  ]);
  assert.deepEqual(Object.keys(first.web_source), [
    'revision',
    'dirty',
    'input_sha256'
  ]);
  assert.deepEqual(Object.keys(first.radroots_oracle), [
    'repository',
    'revision',
    'source_lock_sha256'
  ]);
  assert.deepEqual(Object.keys(first.dependency_locks), [
    'web_pnpm_sha256',
    'oracle_cargo_sha256'
  ]);
  assert.equal(first.web_source.revision, git(f.root, 'rev-parse', 'HEAD'));
  assert.equal(first.web_source.dirty, false);
  assert.equal(
    first.radroots_oracle.revision,
    '189c49b74b4bafc142b00b76b296477931139e72'
  );
  /** @type {Array<[keyof typeof first.dependency_locks, string]>} */
  const locks = [
    ['web_pnpm_sha256', 'web/pnpm-lock.yaml'],
    ['oracle_cargo_sha256', 'core/Cargo.lock']
  ];
  for (const [field, name] of locks) {
    assert.equal(
      first.dependency_locks[field],
      sha256(await readFile(path.join(f.root, name)))
    );
  }
  await mkdir(path.join(f.web, 'build'));
  await writeFile(path.join(f.web, 'build/generated.txt'), 'generated output');
  await writeFile(path.join(f.root, 'native-unrelated.txt'), 'not a web input');
  assert.deepEqual(await f.generate(), first);
  assert.deepEqual(
    await readFile(path.join(f.web, 'static/build-info.json')),
    bytes
  );
  assert.equal(git(f.root, 'status', '--short', '--', 'web'), '');
  assert.doesNotMatch(
    bytes.toString(),
    /example.invalid|qualified|GIT_|\/Users\/|hc provenance/
  );
});

for (const mutation of [
  'modified',
  'staged',
  'deleted',
  'untracked',
  'lock',
  'oracle'
]) {
  await test(`actual ${mutation} input dirtiness changes web identity`, async (t) => {
    const f = await fixture(t);
    const before = await f.generate();
    const file = path.join(f.web, 'src/source.ts');
    if (mutation === 'deleted') await rm(file);
    else if (mutation === 'untracked')
      await writeFile(path.join(f.web, 'src/new file.ts'), 'new source');
    else if (mutation === 'lock')
      await writeFile(path.join(f.web, 'pnpm-lock.yaml'), 'changed web lock');
    else if (mutation === 'oracle') {
      const selected = path.join(f.root, 'radroots.lib.source-lock.v1.toml');
      await writeFile(
        selected,
        (await readFile(selected, 'utf8')).replace(
          '189c49b74b4bafc142b00b76b296477931139e72',
          'a'.repeat(40)
        )
      );
    } else {
      await writeFile(file, 'export const value = 2;\n');
      if (mutation === 'staged') git(f.root, 'add', 'web/src/source.ts');
    }
    const after = await f.generate();
    assert.equal(after.web_source.dirty, true);
    assert.notEqual(
      after.web_source.input_sha256,
      before.web_source.input_sha256
    );
    assert.equal(after.web_source.revision, before.web_source.revision);
    if (mutation === 'oracle')
      assert.equal(after.radroots_oracle.revision, 'a'.repeat(40));
  });
}

await test('reject unavailable/malformed selection, mismatched lock, symlink and tracked generated metadata', async (t) => {
  const f = await fixture(t);
  const selected = path.join(f.root, 'radroots.lib.source-lock.v1.toml');
  const original = await readFile(selected);
  await writeFile(selected, 'revision = "bogus"\n');
  await assert.rejects(f.generate, /invalid public Radroots/);
  await writeFile(
    selected,
    Buffer.concat([original, Buffer.from('revision = "duplicate"\n')])
  );
  await assert.rejects(f.generate, /Malformed/);
  await rm(selected);
  await assert.rejects(f.generate, /ENOENT/);
  await writeFile(selected, original);
  const cargoLock = path.join(f.root, 'core/Cargo.lock');
  const lockBytes = await readFile(cargoLock);
  await writeFile(cargoLock, 'tampered lock');
  await assert.rejects(f.generate, /digest mismatch/);
  await writeFile(cargoLock, lockBytes);
  await symlink('source.ts', path.join(f.web, 'src/link.ts'));
  await assert.rejects(f.generate, /Symlinked/);
  await rm(path.join(f.web, 'src/link.ts'));
  await f.generate();
  git(f.root, 'add', '-f', 'web/static/build-info.json');
  await assert.rejects(f.generate, /must not be tracked/);
});

await test('no Git and enclosing parent Git cannot masquerade as capsule source', async (t) => {
  const f = await fixture(t);
  await rm(path.join(f.root, '.git'), { recursive: true });
  await assert.rejects(f.generate);
  const parent = path.dirname(f.root);
  const outer = await mkdtemp(path.join(parent, 'hc outer '));
  t.after(() => rm(outer, { recursive: true, force: true }));
  git(outer, 'init', '-q');
  const web = path.join(outer, 'capsule/web');
  await mkdir(web, { recursive: true });
  await assert.rejects(() => generateBuildInfo(web), /HarvestCircle-owned Git/);
});

await test('ambient sibling Git redirect and secrets cannot affect metadata', async (t) => {
  const f = await fixture(t);
  const before = await f.generate();
  const script = new URL('../../tools/build-info.mjs', import.meta.url);
  const output = execFileSync(
    process.execPath,
    [
      '--input-type=module',
      '-e',
      `import {generateBuildInfo} from ${JSON.stringify(script.href)}; console.log(JSON.stringify(await generateBuildInfo(${JSON.stringify(f.web)})));`
    ],
    {
      env: {
        ...process.env,
        GIT_DIR: '/unavailable/sibling',
        GIT_WORK_TREE: '/private/parent',
        HARVESTCIRCLE_SECRET: 'HC_TEST_ONLY_SECRET'
      },
      encoding: 'utf8'
    }
  );
  assert.deepEqual(JSON.parse(output), before);
  assert.doesNotMatch(
    output,
    /HC_TEST_ONLY_SECRET|private\/parent|unavailable/
  );
});

await test('hostile output entries fail promptly without altering source', async (t) => {
  const f = await fixture(t);
  await f.generate();
  const directory = path.join(f.web, 'static');
  const output = path.join(directory, 'build-info.json');
  const source = path.join(f.web, 'src/source.ts');
  const original = await readFile(source);
  for (const kind of ['fifo', 'symlink', 'hardlink', 'directory']) {
    await rm(output, { recursive: true, force: true });
    if (kind === 'fifo') execFileSync('mkfifo', [output]);
    else if (kind === 'symlink') await symlink(source, output);
    else if (kind === 'hardlink') await link(source, output);
    else await mkdir(output);
    await assert.rejects(f.generate, /Unsafe generated metadata target/);
    assert.deepEqual(await readFile(source), original);
  }
  await rm(directory, { recursive: true });
  await symlink(path.dirname(source), directory);
  await assert.rejects(
    f.generate,
    /Unsafe generated metadata directory|Symlinked build provenance input/
  );
  assert.deepEqual(await readFile(source), original);
});
