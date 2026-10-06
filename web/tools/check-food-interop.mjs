import assert from 'node:assert/strict';
import {
  lstat,
  mkdtemp,
  readFile,
  readdir,
  rm,
  writeFile
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { webTemplates } from '../tests/conformance/food-writer-cases.ts';
import { createFoodReplay } from '../tests/conformance/food-bidirectional.ts';

const web = fileURLToPath(new URL('../', import.meta.url));
const root = fileURLToPath(new URL('../../', import.meta.url));
const fixture = new URL(
  '../tests/conformance/food-writer-rust.v1.json',
  import.meta.url
);
const args = process.argv.slice(2);
assert.ok(
  args.length === 0 || (args.length === 1 && args[0] === '--write'),
  'expected no arguments or explicit --write'
);
assert.equal(process.versions.node, '24.21.0');
const run = (command, argv, cwd = root) => {
  const result = spawnSync(command, argv, {
    cwd,
    encoding: 'utf8',
    timeout: 180000,
    maxBuffer: 1024 * 1024
  });
  assert.ifError(result.error);
  assert.equal(result.signal, null, `${command} interrupted`);
  return result;
};
const success = (command, argv, cwd = root) => {
  const result = run(command, argv, cwd);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  return result.stdout;
};
const cargo = [
  '+1.97.1',
  'run',
  '--manifest-path',
  'contracts/interop/food_availability/oracle/Cargo.toml',
  '--locked',
  '--offline',
  '--quiet',
  '--'
];
const names = [
  'corpus.v1.json',
  'source_profile.v1.json',
  'provenance.v1.json',
  'oracle/src/main.rs'
];
const inputs = await Promise.all(
  names.map(async (name) => {
    const path = new URL(
      `../../contracts/interop/food_availability/${name}`,
      import.meta.url
    );
    const info = await lstat(path);
    assert.ok(
      info.isFile() &&
        !info.isSymbolicLink() &&
        info.size > 0 &&
        info.size <= 1024 * 1024
    );
    const bytes = await readFile(path);
    assert.equal(bytes.length, info.size);
    return [name, bytes];
  })
);
const corpus = inputs[0][1];
success(process.execPath, ['tools/check-source.mjs'], web);
success('cargo', [...cargo, '--check']);
assert.equal(
  success('cargo', [...cargo, '--emit']),
  corpus.toString(),
  'current pinned Rust corpus differs'
);
const temporary = await mkdtemp(join(tmpdir(), 'harvestcircle-food-interop-'));
try {
  const candidate = join(temporary, 'web.json');
  await writeFile(candidate, JSON.stringify(webTemplates, null, 2) + '\n');
  const actual = JSON.parse(
    success('cargo', [...cargo, '--consume-web', candidate])
  );
  const receipt = createFoodReplay(actual, webTemplates, inputs);
  for (const mutation of ['tag', 'amount']) {
    const altered = webTemplates.map((row) => ({
      ...row,
      wire_parts: {
        ...row.wire_parts,
        tags: row.wire_parts.tags.map((tag) => [...tag])
      }
    }));
    const price = altered[0].wire_parts.tags.find((tag) => tag[0] === 'price');
    assert.ok(price);
    if (mutation === 'tag') price[0] = 'different-price';
    else price[1] = '1e3';
    await writeFile(candidate, JSON.stringify(altered, null, 2) + '\n');
    const rejected = run('cargo', [...cargo, '--consume-web', candidate]);
    assert.equal(
      rejected.status,
      101,
      `actual Rust accepted ${mutation} mutation or did not execute`
    );
    assert.ok(
      rejected.stderr.includes('website template rejected:'),
      rejected.stderr
    );
  }
  if (args[0] === '--write')
    await writeFile(fixture, JSON.stringify(receipt, null, 2) + '\n');
  else
    assert.deepEqual(
      JSON.parse(await readFile(fixture, 'utf8')),
      receipt,
      'checked website/Rust replay is stale; regenerate explicitly with --write'
    );
  const tests = (
    await readdir(new URL('../tests/conformance/', import.meta.url))
  )
    .filter((name) => name.endsWith('.test.ts'))
    .sort();
  assert.ok(tests.length >= 5, 'missing conformance consumers');
  for (const required of [
    'food-reader.test.ts',
    'food-writer.test.ts',
    'food-corpus.test.ts',
    'food-bidirectional.test.ts'
  ])
    assert.ok(
      tests.includes(required),
      `missing actual conformance consumer: ${required}`
    );
  const output = success(
    process.execPath,
    ['--test', ...tests.map((name) => `tests/conformance/${name}`)],
    web
  );
  process.stdout.write(output);
  const vectors = JSON.parse(corpus.toString()).vectors;
  const readerCases = vectors.filter((row) => row.signed_wires.event).length;
  assert.equal(readerCases, 18);
  console.log(
    JSON.stringify({
      result: 'PASS',
      qualification: 'FIXTURE_ONLY_NOT_DEPLOYED_TERA',
      revision: receipt.revision,
      reader_cases: readerCases,
      writer_cases: webTemplates.length,
      deliberate_mutations_rejected_by_actual_rust: 2,
      signing: 'NOT_RUN_NO_KEYS'
    })
  );
} finally {
  await rm(temporary, { recursive: true, force: true });
}
