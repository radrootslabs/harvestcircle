import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const gitEnv = Object.fromEntries(
  Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_'))
);
const root = fileURLToPath(new URL('../../../', import.meta.url));
/** @param {string[]} args */
const git = (...args) =>
  execFileSync('git', ['-C', root, ...args], {
    env: gitEnv,
    maxBuffer: 64 * 1024 * 1024
  });
const revision = git('rev-parse', 'HEAD').toString().trim();
const epoch = Number(
  git('show', '-s', '--format=%ct', revision).toString().trim()
);
const tree = git('ls-tree', '-rz', '--full-tree', revision)
  .toString()
  .split('\0')
  .filter(Boolean)
  .map((entry) => {
    const match = /^(100644|100755) blob ([a-f0-9]{40})\t(.+)$/.exec(entry);
    assert.ok(match, 'Full source tree must contain regular Git blobs only');
    return {
      mode: match[1] === '100755' ? 0o755 : 0o644,
      blob: match[2],
      name: match[3]
    };
  })
  .sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name)));
// One actual Git blob batch, independent from filesystem/generated outputs.
const raw = execFileSync('git', ['-C', root, 'cat-file', '--batch'], {
  input: tree.map((entry) => entry.blob).join('\n') + '\n',
  env: gitEnv,
  maxBuffer: 64 * 1024 * 1024
});
let cursor = 0;
const inputs = tree.map((entry) => {
  const end = raw.indexOf(10, cursor);
  const header = raw.subarray(cursor, end).toString().split(' ');
  assert.equal(header[0], entry.blob);
  assert.equal(header[1], 'blob');
  const size = Number(header[2]);
  const bytes = raw.subarray(end + 1, end + 1 + size);
  assert.equal(bytes.length, size);
  cursor = end + size + 2;
  return { ...entry, bytes };
});
assert.equal(cursor, raw.length);
/** @param {Buffer} header @param {number} offset @param {number} width @param {number} value */
function octal(header, offset, width, value) {
  const field = value.toString(8).padStart(width - 1, '0') + '\0';
  assert.equal(field.length, width);
  header.write(field, offset, width, 'ascii');
}
/** @param {typeof inputs} entries */
function encodeFixture(entries) {
  // Test-only canonical materialization: never a release/distribution producer.
  const blocks = [];
  for (const entry of entries) {
    const header = Buffer.alloc(512);
    let name = entry.name;
    let prefix = '';
    if (Buffer.byteLength(name) > 100) {
      const split = name.lastIndexOf('/');
      prefix = name.slice(0, split);
      name = name.slice(split + 1);
    }
    assert.ok(
      Buffer.byteLength(name) <= 100 && Buffer.byteLength(prefix) <= 155
    );
    header.write(name, 0, 100);
    octal(header, 100, 8, entry.mode);
    octal(header, 108, 8, 0);
    octal(header, 116, 8, 0);
    octal(header, 124, 12, entry.bytes.length);
    octal(header, 136, 12, epoch);
    header.fill(32, 148, 156);
    header[156] = 48;
    header.write('ustar\0', 257, 6);
    header.write('00', 263, 2);
    header.write(prefix, 345, 155);
    const checksum = header.reduce((sum, value) => sum + value, 0);
    header.write(checksum.toString(8).padStart(6, '0') + '\0 ', 148, 8);
    blocks.push(
      header,
      entry.bytes,
      Buffer.alloc((512 - (entry.bytes.length % 512)) % 512)
    );
  }
  return Buffer.concat([...blocks, Buffer.alloc(1024)]);
}
/** @param {Buffer} bytes */
function decodeAndCompare(bytes) {
  let offset = 0;
  let count = 0;
  let previous = Buffer.alloc(0);
  while (
    offset < bytes.length &&
    !bytes.subarray(offset, offset + 512).equals(Buffer.alloc(512))
  ) {
    const header = bytes.subarray(offset, offset + 512);
    assert.equal(header.length, 512, 'Truncated header');
    /** @param {number} start @param {number} width */
    const field = (start, width) => {
      const part = header.subarray(start, start + width);
      const zero = part.indexOf(0);
      if (zero !== -1)
        assert.ok(
          part.subarray(zero).every((value) => value === 0),
          'Noncanonical field padding'
        );
      return new TextDecoder('utf-8', { fatal: true }).decode(
        zero === -1 ? part : part.subarray(0, zero)
      );
    };
    /** @param {number} start @param {number} width */
    const number = (start, width) => {
      const value = field(start, width);
      assert.match(value, /^[0-7]+$/);
      return Number.parseInt(value, 8);
    };
    const prefix = field(345, 155);
    const name = (prefix ? prefix + '/' : '') + field(0, 100);
    assert.ok(
      name &&
        !name.startsWith('/') &&
        !name.split('/').some((part) => !part || part === '.' || part === '..'),
      'Unsafe archive path'
    );
    const ordered = Buffer.from(name);
    assert.ok(
      Buffer.compare(previous, ordered) < 0,
      'Bytewise order/duplicate violation'
    );
    previous = ordered;
    assert.equal(
      header[156],
      48,
      'Only regular files, no PAX/directory/link/submodule'
    );
    assert.equal(field(157, 100), '', 'No link target');
    assert.equal(header.subarray(257, 263).toString(), 'ustar\0');
    assert.equal(header.subarray(263, 265).toString(), '00');
    for (const [start, width] of [
      [265, 32],
      [297, 32]
    ])
      assert.equal(field(start, width), '', 'No owner names');
    assert.equal(number(108, 8), 0);
    assert.equal(number(116, 8), 0);
    assert.ok(
      header.subarray(329, 345).every((value) => value === 0),
      'No device metadata'
    );
    assert.ok(
      header.subarray(500).every((value) => value === 0),
      'No header extension'
    );
    assert.equal(number(136, 12), epoch, 'Actual candidate epoch');
    const expected = inputs[count++];
    assert.ok(expected, 'Extra archive entry');
    assert.equal(name, expected.name, 'Exact unprefixed full-tree path');
    assert.equal(number(100, 8), expected.mode, 'Exact Git executable mode');
    const size = number(124, 12);
    assert.equal(size, expected.bytes.length);
    const checksum = header.reduce(
      (sum, value, index) => sum + (index >= 148 && index < 156 ? 32 : value),
      0
    );
    assert.match(header.subarray(148, 156).toString(), /^[0-7]{6}\0 $/);
    assert.equal(
      Number.parseInt(header.subarray(148, 154).toString(), 8),
      checksum
    );
    offset += 512;
    assert.deepEqual(
      bytes.subarray(offset, offset + size),
      expected.bytes,
      'Actual Git blob bytes'
    );
    offset += size;
    const padding = (512 - (size % 512)) % 512;
    assert.ok(
      bytes.subarray(offset, offset + padding).every((value) => value === 0),
      'Zero content padding'
    );
    offset += padding;
  }
  assert.equal(
    count,
    inputs.length,
    'Complete tracked tree, never runtime-filtered'
  );
  assert.equal(bytes.length - offset, 1024, 'Exactly two zero trailer blocks');
  assert.ok(bytes.subarray(offset).every((value) => value === 0));
}
const archive = encodeFixture(inputs);
await test('native v3 JSON and Kotlin canonical authority remain exact and separate from web provenance', async () => {
  const contractBytes = await readFile(
    new URL(
      '../../../contracts/release/harvestcircle-artifact-contract.v3.json',
      import.meta.url
    )
  );
  const kotlin = await readFile(
    new URL(
      '../../../build-logic/contracts/src/main/kotlin/org/harvestcircle/buildlogic/contracts/HarvestCircleArtifactContract.kt',
      import.meta.url
    ),
    'utf8'
  );
  const literal = /CANONICAL_JSON: String =\s*("(?:[^"\\]|\\.)*")/.exec(kotlin);
  assert.ok(literal);
  assert.equal(contractBytes.toString(), JSON.parse(literal[1]));
  const contract = JSON.parse(contractBytes.toString());
  assert.equal(contract.contract_version, 3);
  assert.equal(contract.package_contract.filename, 'HarvestCircle-1.0.0.dmg');
  assert.equal(contract.producer.source_task, 'packageDmg');
  assert.equal(contract.source_archive.format, 'ustar');
  assert.equal(contract.source_archive.content, 'exact_source_revision_tree');
  assert.equal(contract.source_archive.pax_headers, 'forbidden');
  assert.equal(contract.source_archive.trailer, 'two_zero_blocks');
  assert.equal(contract.delivery.publication, 'unauthorized');
  assert.equal(contract.sqlite.high_level_authority, 'sqlx_only');
});
await test('actual complete HEAD Git tree fixture preserves web, licenses, corpus, modes, epoch and blobs', () => {
  decodeAndCompare(archive);
  assert.ok(inputs.some((entry) => entry.name === 'web/package.json'));
  assert.ok(inputs.some((entry) => entry.name === 'LICENSE'));
  assert.ok(inputs.some((entry) => entry.name.startsWith('LICENSES/')));
  assert.ok(
    inputs.some((entry) =>
      entry.name.startsWith('core/crates/harvestcircle_nostr/tests/fixtures/')
    )
  );
  assert.ok(inputs.some((entry) => entry.mode === 0o755));
});
for (const kind of [
  'filtered',
  'truncated',
  'trailer',
  'content',
  'mode',
  'epoch',
  'uid',
  'prefix',
  'pax',
  'directory',
  'symlink',
  'hardlink',
  'order',
  'duplicate',
  'padding'
]) {
  await test(`independent full-tree archive comparison rejects ${kind}`, () => {
    let candidate = Buffer.from(archive);
    if (kind === 'filtered')
      candidate = encodeFixture(
        inputs.filter((entry) => !entry.name.startsWith('web/'))
      );
    else if (kind === 'order')
      candidate = encodeFixture([inputs[1], inputs[0], ...inputs.slice(2)]);
    else if (kind === 'duplicate')
      candidate = encodeFixture([inputs[0], ...inputs]);
    else if (kind === 'truncated') candidate = candidate.subarray(0, 200);
    else if (kind === 'trailer')
      candidate = Buffer.concat([candidate, Buffer.alloc(512)]);
    else if (kind === 'content') candidate[512] ^= 1;
    else if (kind === 'mode')
      octal(candidate, 100, 8, inputs[0].mode === 0o644 ? 0o755 : 0o644);
    else if (kind === 'epoch') octal(candidate, 136, 12, epoch + 1);
    else if (kind === 'uid') octal(candidate, 108, 8, 1);
    else if (kind === 'prefix') candidate.write('runtime/', 0);
    else if (kind === 'padding') candidate[512 + inputs[0].bytes.length] = 1;
    else {
      const type = { pax: 120, directory: 53, symlink: 50, hardlink: 49 }[kind];
      if (type === undefined)
        throw new Error('Unknown negative archive fixture');
      candidate[156] = type;
    }
    assert.throws(() => decodeAndCompare(candidate));
  });
}
await test('ordinary git archive PAX/directory output is not the native v3 canonical fixture', () => {
  assert.throws(() =>
    decodeAndCompare(git('archive', '--format=tar', revision))
  );
});
