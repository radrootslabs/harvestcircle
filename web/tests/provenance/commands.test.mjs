import assert from 'node:assert/strict';
import { performance } from 'node:perf_hooks';
import { spawnSync } from 'node:child_process';
import {
  mkdtemp,
  mkdir,
  readFile,
  writeFile,
  rm,
  symlink
} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

/** @param {import('node:test').TestContext} t */
async function fixture(t) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'hc web commands '));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(path.join(root, 'web/tools'), { recursive: true });
  await mkdir(path.join(root, 'bin'));
  for (const [source, target] of [
    ['../../../Makefile', 'Makefile'],
    ['../../package.json', 'web/package.json'],
    ['../../.node-version', 'web/.node-version'],
    ['../../tools/doctor.mjs', 'web/tools/doctor.mjs']
  ])
    await writeFile(
      path.join(root, target),
      await readFile(new URL(source, import.meta.url))
    );
  await symlink(process.execPath, path.join(root, 'bin/node'));
  const log = path.join(root, 'calls.frames');
  await symlink('/bin/sh', path.join(root, 'bin/corepack'));
  await writeFile(
    path.join(root, 'web/pnpm'),
    `#!/bin/sh
printf '%s\\0' "$PWD" "\${COREPACK_ENABLE_NETWORK-}" "$(( $# + 1 ))" "$0" "$@" >> "$HC_COMMAND_LOG"
version=false
failed=false
for argument do
  if [ "$argument" = '--version' ]; then version=true; fi
  if [ -n "\${HC_FAIL_COMMAND-}" ] && [ "$argument" = "$HC_FAIL_COMMAND" ]; then failed=true; fi
done
if [ "$version" = true ]; then
  printf '%s\\n' "\${HC_PNPM_VERSION-12.9.1}"
elif [ "$failed" = true ]; then
  exit 37
fi
`,
    { mode: 0o644 }
  );
  const env = {
    ...process.env,
    PATH: path.join(root, 'bin'),
    HC_COMMAND_LOG: log
  };
  /** @param {string} target @param {Record<string, string>} [extra] */
  const run = (target, extra = {}) => {
    const started = performance.now();
    const result = spawnSync(
      '/usr/bin/make',
      ['--no-print-directory', target],
      {
        cwd: root,
        env: { ...env, ...extra },
        encoding: 'utf8',
        timeout: 1800000
      }
    );
    return {
      ...result,
      diagnostic: JSON.stringify({
        target,
        status: result.status,
        signal: result.signal,
        error:
          result.error && 'code' in result.error
            ? result.error.code
            : undefined,
        elapsed_ms: performance.now() - started,
        stdout: result.stdout,
        stderr: result.stderr
      })
    };
  };
  const calls = async () => {
    const fields = (await readFile(log, 'utf8')).split('\0');
    assert.equal(fields.pop(), '');
    const records = [];
    for (let index = 0; index < fields.length;) {
      const cwd = fields[index++];
      const network = fields[index++];
      const count = Number(fields[index++]);
      assert.ok(
        Number.isSafeInteger(count) &&
          count >= 0 &&
          index + count <= fields.length
      );
      records.push({
        cwd,
        args: fields.slice(index, index + count),
        ...(network ? { network } : {})
      });
      index += count;
    }
    return records;
  };
  return { root, run, calls };
}

await test(
  'standalone web targets select web cwd and run every required lane without native tools',
  { timeout: 1800000 },
  async (t) => {
    const { root, run, calls } = await fixture(t);
    for (const target of [
      'web-doctor',
      'web-install',
      'web-check',
      'web-build',
      'web-dev'
    ]) {
      const result = run(target);
      assert.equal(result.status, 0, result.diagnostic);
    }
    const actual = await calls();
    assert.ok(actual.every((call) => call.cwd === path.join(root, 'web')));
    assert.ok(
      actual
        .filter((call) => call.args.includes('--version'))
        .every((call) => call.network === '0')
    );
    assert.deepEqual(
      actual
        .filter((call) => !call.args.includes('--version'))
        .map((call) => call.args),
      [
        ['pnpm', 'install', '--frozen-lockfile'],
        ['pnpm', 'run', 'check'],
        ['pnpm', 'run', 'lint'],
        ['pnpm', 'run', 'test:unit'],
        ['pnpm', 'run', 'test:conformance'],
        ['pnpm', 'run', 'build'],
        ['pnpm', 'run', 'dev']
      ]
    );
  }
);

await test(
  'doctor rejects a mismatched selected manager before installation',
  { timeout: 1800000 },
  async (t) => {
    const { run, calls } = await fixture(t);
    const result = run('web-install', { HC_PNPM_VERSION: '10.25.0' });
    assert.notEqual(result.status, 0, result.diagnostic);
    assert.match(
      result.stderr,
      /Expected pnpm 12\.9\.1; selected 10\.25\.0/,
      result.diagnostic
    );
    assert.equal((await calls()).length, 1);
  }
);

await test(
  'web check propagates failing commands and stops subsequent lanes',
  { timeout: 1800000 },
  async (t) => {
    const { run, calls } = await fixture(t);
    const result = run('web-check', { HC_FAIL_COMMAND: 'lint' });
    assert.notEqual(result.status, 0, result.diagnostic);
    assert.match(result.stderr, /Error 37/, result.diagnostic);
    assert.deepEqual(
      (await calls()).map((call) => call.args),
      [
        ['pnpm', '--version'],
        ['pnpm', 'run', 'check'],
        ['pnpm', 'run', 'lint']
      ]
    );
  }
);

await test(
  'missing web tooling fails without trying native tools',
  { timeout: 1800000 },
  async (t) => {
    const { root, run } = await fixture(t);
    await rm(path.join(root, 'bin/corepack'));
    const result = run('web-build');
    assert.notEqual(result.status, 0, result.diagnostic);
    assert.match(result.stderr, /ENOENT/, result.diagnostic);
  }
);

await test(
  'doctor rejects mismatched toolchain authorities',
  { timeout: 1800000 },
  async (t) => {
    const { root, run } = await fixture(t);
    await writeFile(path.join(root, 'web/.node-version'), '24.20.0\n');
    const result = run('web-doctor');
    assert.notEqual(result.status, 0, result.diagnostic);
    assert.match(
      result.stderr,
      /Web toolchain pins disagree or are not exact/,
      result.diagnostic
    );
  }
);

await test(
  'doctor rejects wrong selected Node before invoking Corepack',
  { timeout: 1800000 },
  async (t) => {
    const { root, run } = await fixture(t);
    const file = path.join(root, 'web/package.json');
    const inputs = JSON.parse(await readFile(file, 'utf8'));
    inputs.engines.node = '24.20.0';
    await writeFile(file, JSON.stringify(inputs));
    await writeFile(path.join(root, 'web/.node-version'), '24.20.0\n');
    const result = run('web-doctor');
    assert.notEqual(result.status, 0, result.diagnostic);
    assert.match(
      result.stderr,
      /Expected Node 24\.20\.0; selected 24\.21\.0/,
      result.diagnostic
    );
  }
);
