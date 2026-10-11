import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
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
import { format, resolveConfig } from 'prettier';
import { webTemplates } from '../tests/conformance/food-writer-cases.ts';
import { qualifiedTemplates } from '../tests/conformance/food-qualified-matrix.ts';
import { createFoodReplay } from '../tests/conformance/food-bidirectional.ts';

const web = fileURLToPath(new URL('../', import.meta.url));
const root = fileURLToPath(new URL('../../', import.meta.url));
const fixture = new URL(
  '../tests/conformance/food-writer-rust.v1.json',
  import.meta.url
);
const qualifiedFixture = new URL(
  '../tests/conformance/food-qualified-writer-rust.v1.json',
  import.meta.url
);
const args = process.argv.slice(2);
assert.ok(
  args.length === 0 ||
    (args.length === 1 && ['--write', '--write-qualified'].includes(args[0])),
  'expected no arguments, --write or browser-only --write-qualified'
);
assert.equal(process.versions.node, '24.21.0');
const run = (command, argv, cwd = root) => {
  // Compiler and cache waits on a busy host have no outer elapsed cutoff.
  const result = spawnSync(command, argv, {
    cwd,
    encoding: 'utf8',
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
  '+1.99.0',
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
async function ownedInput(name) {
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
  return bytes;
}
const readInputs = () =>
  Promise.all(names.map(async (name) => [name, await ownedInput(name)]));
const inputs = await readInputs();
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
    console.log(
      JSON.stringify({
        direction: 'WEB_TO_RUST_NEGATIVE',
        batch: 'original',
        mutation,
        exit_code: rejected.status,
        actual_stderr: rejected.stderr
      })
    );
  }
  if (args[0] === '--write') {
    // Refresh producer provenance only after actual Rust corpus equality,
    // website consumption and hostile-template rejection have succeeded.
    const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
    const provenance = JSON.parse(inputs[2][1].toString());
    assert.equal(provenance.public_source.revision, actual.revision);
    assert.equal(provenance.source_copy.sha256, digest(inputs[1][1]));
    assert.equal(provenance.corpus.sha256, digest(corpus));
    const producerNames = [
      'oracle/Cargo.toml',
      'oracle/Cargo.lock',
      'oracle/rust-toolchain.toml',
      'oracle/src/main.rs'
    ];
    assert.deepEqual(
      provenance.producer.files.map((row) => row.path).sort(),
      producerNames.toSorted()
    );
    assert.match(
      success('rustup', ['run', '1.99.0', 'rustc', '--version']),
      /^rustc 1\.99\.0 /
    );
    assert.match(
      (await ownedInput('oracle/rust-toolchain.toml')).toString(),
      /^channel = "1\.99\.0"$/m
    );
    provenance.producer.rust_toolchain = '1.99.0';
    provenance.producer.files = await Promise.all(
      producerNames.map(async (path) => {
        const bytes = await ownedInput(path);
        return { path, sha256: digest(bytes), bytes: bytes.length };
      })
    );
    await writeFile(
      new URL(
        '../../contracts/interop/food_availability/provenance.v1.json',
        import.meta.url
      ),
      JSON.stringify(provenance, null, 2) + '\n'
    );
  }
  // The replay must bind the refreshed producer provenance, never stale bytes
  // read before an explicitly requested regeneration.
  const receipt = createFoodReplay(actual, webTemplates, await readInputs());
  if (args[0] === '--write') {
    // Producer output must satisfy the same pinned formatting policy as lint.
    // Format only the actual Rust replay, before its raw-byte manifest is built.
    const filepath = fileURLToPath(fixture);
    await writeFile(
      fixture,
      await format(JSON.stringify(receipt), {
        ...(await resolveConfig(filepath)),
        filepath
      })
    );
  } else
    assert.deepEqual(
      JSON.parse(await readFile(fixture, 'utf8')),
      receipt,
      'checked website/Rust replay is stale; regenerate explicitly with --write'
    );
  // Qualify a separate browser-owned boundary batch. The original sixteen
  // recipes/shared manifest remain byte-exact and are still checked above.
  await writeFile(
    candidate,
    JSON.stringify(qualifiedTemplates, null, 2) + '\n'
  );
  const qualifiedActual = JSON.parse(
    success('cargo', [...cargo, '--consume-web', candidate])
  );
  const qualifiedControls = [
    'amount-digits',
    'fraction-digits',
    'quantity-zero',
    'quantity-unit',
    'published-unsafe',
    'created-unsafe',
    'text-format',
    'identifier-space',
    'operational-marker',
    'price-frequency'
  ];
  for (const mutation of qualifiedControls) {
    const altered = qualifiedTemplates.map((row) => ({
      ...row,
      wire_parts: {
        ...row.wire_parts,
        tags: row.wire_parts.tags.map((tag) => [...tag])
      }
    }));
    const row = altered[0];
    const tag = (name) => {
      const found = row.wire_parts.tags.find((value) => value[0] === name);
      assert.ok(found, `missing qualified ${name}`);
      return found;
    };
    if (mutation === 'amount-digits') tag('price')[1] = '1'.repeat(29);
    else if (mutation === 'fraction-digits')
      tag('price')[1] = '0.' + '0'.repeat(27) + '1';
    else if (mutation === 'quantity-zero') tag('radroots:quantity')[1] = '0';
    else if (mutation === 'quantity-unit') tag('radroots:quantity')[2] = 'lb';
    else if (mutation === 'published-unsafe')
      tag('published_at')[1] = '9007199254740992';
    else if (mutation === 'created-unsafe') row.created_at = 9007199254740992;
    else if (mutation === 'text-format') tag('title')[1] = 'Food\u200d';
    else if (mutation === 'identifier-space')
      tag('d')[1] = 'invalid identifier';
    else if (mutation === 'operational-marker')
      tag('radroots:quantity')[0] = 'radroots:bin';
    else tag('price').push('weekly');
    await writeFile(candidate, JSON.stringify(altered, null, 2) + '\n');
    const rejected = run('cargo', [...cargo, '--consume-web', candidate]);
    assert.equal(
      rejected.status,
      101,
      `actual Rust accepted qualified ${mutation} or did not execute`
    );
    assert.ok(
      rejected.stderr.includes('website template rejected:'),
      rejected.stderr
    );
    console.log(
      JSON.stringify({
        direction: 'WEB_TO_RUST_NEGATIVE',
        batch: 'qualified',
        mutation,
        exit_code: rejected.status,
        actual_stderr: rejected.stderr
      })
    );
  }
  const qualifiedReceipt = createFoodReplay(
    qualifiedActual,
    qualifiedTemplates,
    await readInputs()
  );
  if (['--write', '--write-qualified'].includes(args[0])) {
    // Browser-only mode writes only this replay. Native/shared producer and
    // original fixture/manifest outputs are checked without regeneration.
    const filepath = fileURLToPath(qualifiedFixture);
    await writeFile(
      qualifiedFixture,
      await format(JSON.stringify(qualifiedReceipt), {
        ...(await resolveConfig(filepath)),
        filepath
      })
    );
  } else
    assert.deepEqual(
      JSON.parse(await readFile(qualifiedFixture, 'utf8')),
      qualifiedReceipt,
      'qualified browser/Rust replay is stale; regenerate explicitly with --write-qualified'
    );
  // The shared consumer manifest binds these exact provenance/replay bytes.
  success(
    process.execPath,
    [
      'tools/generate-interop-manifest.mjs',
      ...(args[0] === '--write' ? ['--write'] : [])
    ],
    web
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
    'food-bidirectional.test.ts',
    'food-qualified-matrix.test.ts'
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
      qualified_writer_cases: qualifiedTemplates.length,
      deliberate_mutations_rejected_by_actual_rust:
        2 + qualifiedControls.length,
      signing: 'NOT_RUN_NO_KEYS'
    })
  );
} finally {
  await rm(temporary, { recursive: true, force: true });
}
