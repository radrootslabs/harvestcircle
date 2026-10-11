import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  lstat,
  realpath,
  readFile,
  writeFile,
  mkdtemp,
  rm
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import {
  messageReplay,
  actualWebMessage
} from '../tests/conformance/message-cases.ts';
import { qualifiedMessageReplay } from '../tests/conformance/message-qualified-matrix.ts';
const args = process.argv.slice(2);
assert.ok(
  args.length === 0 ||
    (args.length === 1 && ['--write', '--write-qualified'].includes(args[0])),
  'expected no arguments or explicit --write or --write-qualified'
);
assert.equal(process.versions.node, '24.21.0');
const root = fileURLToPath(new URL('../../', import.meta.url));
const folder = new URL('../../contracts/interop/message/', import.meta.url);
const revision = '189c49b74b4bafc142b00b76b296477931139e72';
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const cargo = [
  '+1.99.0',
  'run',
  '--manifest-path',
  'contracts/interop/message/oracle/Cargo.toml',
  '--locked',
  '--offline',
  '--quiet',
  '--'
];
function success(command, argv, cwd = root) {
  // Compiler and cache waits on a busy host have no outer elapsed cutoff.
  const r = spawnSync(command, argv, {
    cwd,
    encoding: 'utf8',
    maxBuffer: 1024 * 1024
  });
  assert.ifError(r.error);
  assert.equal(r.signal, null);
  assert.equal(r.status, 0, r.stderr || r.stdout);
  return r.stdout;
}
async function owned(name) {
  const path = new URL(name, folder),
    info = await lstat(path);
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
const profile = JSON.parse((await owned('source_profile.v1.json')).toString());
assert.equal(profile.revision, revision);
await owned('oracle/Cargo.toml');
await owned('oracle/Cargo.lock');
await owned('oracle/src/main.rs');
// Offline actual Cargo/cache identity and exact immutable source, never sibling Lib.
const metadata = JSON.parse(
  success('cargo', [
    '+1.99.0',
    'metadata',
    '--manifest-path',
    'contracts/interop/message/oracle/Cargo.toml',
    '--locked',
    '--offline',
    '--format-version',
    '1'
  ])
);
const gitPackages = metadata.packages.filter(
  (p) => typeof p.source === 'string' && p.source.startsWith('git+')
);
assert.equal(gitPackages.length, 6);
const roots = new Set();
for (const p of gitPackages) {
  assert.equal(
    p.source,
    'git+https://github.com/radrootslabs/lib?rev=' + revision + '#' + revision
  );
  roots.add(
    success(
      'git',
      ['rev-parse', '--show-toplevel'],
      dirname(p.manifest_path)
    ).trim()
  );
}
assert.equal(roots.size, 1);
const cache = [...roots][0];
assert.equal(success('git', ['rev-parse', 'HEAD'], cache).trim(), revision);
assert.equal(success('git', ['diff', '--raw'], cache), '');
assert.equal(success('git', ['diff', '--cached', '--raw'], cache), '');
const untracked = success(
  'git',
  ['ls-files', '--others', '--exclude-standard'],
  cache
).trim();
assert.ok(untracked === '' || untracked === '.cargo-ok');
if (untracked) {
  const marker = await lstat(join(cache, untracked));
  assert.ok(marker.isFile() && !marker.isSymbolicLink() && marker.size === 0);
}
assert.deepEqual(
  Object.keys(profile.pinned_lib_source_sha256).sort(),
  [
    'crates/event/src/message.rs',
    'crates/event_codec/src/message/decode.rs',
    'crates/event_codec/src/message/encode.rs',
    'crates/event_codec/src/message/tags.rs'
  ].sort()
);
for (const [name, hash] of Object.entries(profile.pinned_lib_source_sha256)) {
  const path = join(cache, name),
    info = await lstat(path);
  assert.equal(await realpath(path), path);
  assert.ok(
    info.isFile() &&
      !info.isSymbolicLink() &&
      info.size > 0 &&
      info.size <= 1024 * 1024
  );
  assert.equal(digest(await readFile(path)), hash, name);
}
const emitted = success('cargo', [...cargo, '--emit']);
const corpus = JSON.parse(emitted);
assert.equal(corpus.revision, revision);
assert.equal(
  corpus.source_sha256,
  digest(await owned('source_profile.v1.json'))
);
if (args[0] === '--write') {
  for (const name of ['corpus.v1.json', 'provenance.v1.json']) {
    try {
      const info = await lstat(new URL(name, folder));
      assert.ok(info.isFile() && !info.isSymbolicLink());
    } catch (error) {
      if (!(
        error &&
        typeof error === 'object' &&
        'code' in error &&
        error.code === 'ENOENT'
      ))
        throw error;
    }
  }
  await writeFile(new URL('corpus.v1.json', folder), emitted);
  /** @type {Record<string,string>} */ const inputs = {};
  for (const name of [
    'source_profile.v1.json',
    'corpus.v1.json',
    'oracle/Cargo.toml',
    'oracle/Cargo.lock',
    'oracle/src/main.rs'
  ])
    inputs[name] = digest(await owned(name));
  await writeFile(
    new URL('provenance.v1.json', folder),
    JSON.stringify(
      {
        schema_version: 1,
        revision,
        inputs,
        pinned_lib_source_sha256: profile.pinned_lib_source_sha256,
        qualification:
          'Generated by actual pinned Rust Message codec; explicit canonical/budget/schema web differences retained. No signature, room, extension, relay or send qualification.'
      },
      null,
      2
    ) + '\n'
  );
}
assert.equal((await owned('corpus.v1.json')).toString(), emitted);
const provenance = JSON.parse((await owned('provenance.v1.json')).toString());
assert.equal(provenance.revision, revision);
assert.deepEqual(
  provenance.pinned_lib_source_sha256,
  profile.pinned_lib_source_sha256
);
assert.deepEqual(
  Object.keys(provenance.inputs).sort(),
  [
    'source_profile.v1.json',
    'corpus.v1.json',
    'oracle/Cargo.toml',
    'oracle/Cargo.lock',
    'oracle/src/main.rs'
  ].sort()
);
for (const [name, hash] of Object.entries(provenance.inputs))
  assert.equal(digest(await owned(name)), hash);
success('cargo', [...cargo, '--check']);
for (const vector of corpus.vectors) {
  const actual = actualWebMessage(vector);
  assert.equal(actual.status, vector.web_status, vector.id);
  if (vector.web_status === 'supported') {
    const { error, ...expected } = vector.expected;
    assert.equal(error, undefined);
    assert.deepEqual(actual, expected, vector.id);
  } else if (vector.expected.status === 'supported')
    assert.ok(vector.policy_difference, vector.id);
}
const temporary = await mkdtemp(
  join(tmpdir(), 'harvestcircle-message-interop-')
);
try {
  async function consume(replay, batch) {
    assert.ok(replay.length > 0);
    const file = join(temporary, batch + '.json');
    await writeFile(file, JSON.stringify(replay));
    const result = JSON.parse(success('cargo', [...cargo, '--consume', file]));
    assert.equal(result.revision, revision);
    assert.equal(result.consumed.length, replay.length);
    assert.deepEqual(
      result.consumed,
      replay.map((v) => ({ id: v.id, message: v.expected }))
    );
    return result;
  }
  const original = messageReplay();
  assert.equal(original.length, 11);
  await consume(original, 'original');
  assert.equal(qualifiedMessageReplay.length, 16);
  const qualified = await consume(qualifiedMessageReplay, 'qualified');
  const recipePath = new URL(
    '../tests/conformance/message-qualified-matrix.ts',
    import.meta.url
  );
  const receiptPath = new URL(
    '../tests/conformance/message-qualified-writer-rust.v1.json',
    import.meta.url
  );
  const inputs = {
    'message-qualified-matrix.ts': digest(await readFile(recipePath))
  };
  for (const name of [
    'source_profile.v1.json',
    'corpus.v1.json',
    'oracle/src/main.rs'
  ])
    inputs[name] = digest(await owned(name));
  const receipt = {
    schema_version: 1,
    revision,
    recipe_sha256: digest(JSON.stringify(qualifiedMessageReplay)),
    inputs,
    consumed: qualified.consumed,
    qualification:
      'Actual pinned Rust99 consumes separate browser-owned unsigned kind 14 Message recipes; not independent-client receive/reply, encryption, extension, relay or publication qualification.'
  };
  const existing = await lstat(receiptPath);
  assert.ok(existing.isFile() && !existing.isSymbolicLink());
  if (args[0] === '--write' || args[0] === '--write-qualified')
    await writeFile(receiptPath, JSON.stringify(receipt, null, 2) + '\n');
  assert.deepEqual(JSON.parse(await readFile(receiptPath, 'utf8')), receipt);
  const controls = [
    [
      'standalone-seal-kind',
      0,
      (row) => {
        row.wire_parts.kind = 13;
      }
    ],
    [
      'missing-recipient',
      0,
      (row) => {
        row.wire_parts.tags = [];
      }
    ],
    [
      'changed-reply-parent',
      2,
      (row) => {
        row.wire_parts.tags[1][1] = '4'.repeat(64);
      }
    ],
    [
      'changed-subject',
      4,
      (row) => {
        row.wire_parts.tags[1][1] = 'Different subject';
      }
    ]
  ];
  for (const [mutation, index, change] of controls) {
    const originalRow = qualifiedMessageReplay[index];
    const row = {
      ...originalRow,
      wire_parts: {
        ...originalRow.wire_parts,
        tags: originalRow.wire_parts.tags.map((tag) => [...tag])
      }
    };
    change(row);
    const file = join(temporary, mutation + '.json');
    await writeFile(file, JSON.stringify([row]));
    const result = spawnSync('cargo', [...cargo, '--consume', file], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 1024 * 1024
    });
    assert.ifError(result.error);
    assert.equal(result.signal, null);
    assert.equal(result.status, 101, result.stderr || result.stdout);
    assert.match(result.stderr, /assertion .* failed/);
    console.log(
      JSON.stringify({
        direction: 'WEB_TO_RUST_NEGATIVE',
        mutation,
        exit_code: result.status,
        actual_stderr: result.stderr
      })
    );
  }
  console.log(
    JSON.stringify({
      result: 'PASS',
      scope: 'SOURCE_ORACLE_NOT_INDEPENDENT_CLIENT_QUALIFICATION',
      revision,
      rust_reader_cases: corpus.vectors.length,
      original_writer_cases: original.length,
      qualified_writer_cases: qualifiedMessageReplay.length,
      negative_controls: controls.length,
      keys: 'NO_KEYS'
    })
  );
} finally {
  await rm(temporary, { recursive: true, force: true });
}
