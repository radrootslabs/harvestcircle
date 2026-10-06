// Test-only generation and validation. Ordinary web commands never regenerate.
import assert from 'node:assert/strict';
import { lstat, readFile, writeFile } from 'node:fs/promises';
import { TextEncoder } from 'node:util';
import {
  buildInteropManifest,
  interopInputPaths,
  validateInteropManifest
} from '../tests/conformance/interop-manifest.ts';
import { webTemplates } from '../tests/conformance/food-writer-cases.ts';
assert.equal(process.version, 'v24.21.0');
const args = process.argv.slice(2);
assert.ok(args.length === 0 || (args.length === 1 && args[0] === '--write'));
const root = new URL('../../', import.meta.url);
const templatePath =
  'contracts/interop/food_availability/web_templates.v1.json';
const manifestPath = 'contracts/interop/manifest.json';
const generatedTemplates = new TextEncoder().encode(
  JSON.stringify(webTemplates, null, 2) + '\n'
);
const inputs = new Map();
for (const path of interopInputPaths) {
  if (path === templatePath && args[0] === '--write') {
    inputs.set(path, generatedTemplates);
    continue;
  }
  const file = new URL(path, root);
  const info = await lstat(file);
  assert.ok(
    info.isFile() &&
      !info.isSymbolicLink() &&
      info.size > 0 &&
      info.size <= 1024 * 1024
  );
  const bytes = new Uint8Array(await readFile(file));
  assert.equal(bytes.byteLength, info.size);
  inputs.set(path, bytes);
}
const manifest = buildInteropManifest(inputs, webTemplates);
if (args[0] === '--write') {
  for (const [path, bytes] of [
    [templatePath, generatedTemplates],
    [manifestPath, JSON.stringify(manifest, null, 2) + '\n']
  ]) {
    const file = new URL(path, root);
    try {
      const info = await lstat(file);
      assert.ok(info.isFile() && !info.isSymbolicLink() && info.nlink === 1);
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
    await writeFile(file, bytes);
  }
} else {
  const file = new URL(manifestPath, root);
  const info = await lstat(file);
  assert.ok(
    info.isFile() &&
      !info.isSymbolicLink() &&
      info.size > 0 &&
      info.size <= 1024 * 1024
  );
  validateInteropManifest(
    JSON.parse(await readFile(file, 'utf8')),
    inputs,
    webTemplates
  );
}
console.log(
  JSON.stringify({
    result: 'PASS',
    public_cases: 40,
    website_cases: 16,
    signed_reader_cases: 18,
    revision: manifest.oracle.revision,
    native_consumer: 'PENDING_HCR014',
    qualification: manifest.qualification,
    action: args[0] === '--write' ? 'EXPLICIT_GENERATION' : 'CHECK_ONLY'
  })
);
