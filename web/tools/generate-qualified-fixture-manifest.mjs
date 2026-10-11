// Explicit test-fixture generation only; ordinary conformance checks never write.
import assert from 'node:assert/strict';
import { lstat, readFile, writeFile } from 'node:fs/promises';
import { format } from 'prettier';
import {
  buildFixtureProvenance,
  provenanceInputPaths,
  validateFixtureProvenance
} from '../tests/conformance/provenance.ts';
assert.equal(process.version, 'v24.21.0');
const args = process.argv.slice(2);
assert.ok(args.length === 0 || (args.length === 1 && args[0] === '--write'));
const root = new URL('../../', import.meta.url);
const inputs = new Map();
for (const path of provenanceInputPaths) {
  const file = new URL(path, root);
  const info = await lstat(file);
  assert.ok(
    info.isFile() &&
      !info.isSymbolicLink() &&
      info.nlink === 1 &&
      info.size > 0 &&
      info.size <= 1024 * 1024
  );
  const raw = new Uint8Array(await readFile(file));
  assert.equal(raw.byteLength, info.size);
  inputs.set(path, raw);
}
const manifest = buildFixtureProvenance(inputs);
const output = new URL(
  'web/tests/fixtures/provenance/qualified-inputs.v1.json',
  root
);
if (args[0] === '--write') {
  try {
    const info = await lstat(output);
    assert.ok(info.isFile() && !info.isSymbolicLink() && info.nlink === 1);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  await writeFile(
    output,
    await format(JSON.stringify(manifest), { parser: 'json', tabWidth: 2 })
  );
} else {
  const info = await lstat(output);
  assert.ok(
    info.isFile() &&
      !info.isSymbolicLink() &&
      info.nlink === 1 &&
      info.size > 0 &&
      info.size <= 1024 * 1024
  );
  validateFixtureProvenance(JSON.parse(await readFile(output, 'utf8')), inputs);
}
console.log(
  JSON.stringify({
    result: 'PASS',
    action: args[0] === '--write' ? 'EXPLICIT_GENERATION' : 'CHECK_ONLY',
    inputs: manifest.inputs.length,
    producer: manifest.producer,
    qualification: manifest.qualification
  })
);
