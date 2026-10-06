import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import type { webTemplates } from './food-writer-cases.ts';
type Templates = typeof webTemplates;
type Inputs = readonly (readonly [string, Uint8Array])[];
const revision = '189c49b74b4bafc142b00b76b296477931139e72';
const producer = 'actual_public_food_constructors_and_unsigned_codec';
const digest = (bytes: Uint8Array | string) =>
  createHash('sha256').update(bytes).digest('hex');
interface RustResult {
  revision: string;
  source: string;
  signing: string;
  templates: {
    id: string;
    result: string;
    wire_parts: Templates[number]['wire_parts'];
  }[];
}
interface Replay {
  schema_version: number;
  revision: string;
  qualification: string;
  signing: string;
  web_input_sha256: string;
  inputs: Record<string, string>;
  result: RustResult;
}
export function checkFoodReplay(
  receipt: unknown,
  web: Templates,
  inputs: Inputs
) {
  assert.ok(receipt && typeof receipt === 'object' && !Array.isArray(receipt));
  const r = receipt as Replay;
  assert.equal(r.schema_version, 1);
  assert.equal(r.revision, revision);
  assert.equal(r.qualification, 'FIXTURE_ONLY_NOT_DEPLOYED_TERA');
  assert.equal(r.signing, 'NOT_RUN_NO_KEYS');
  assert.equal(r.web_input_sha256, digest(JSON.stringify(web, null, 2) + '\n'));
  const names = [
    'corpus.v1.json',
    'source_profile.v1.json',
    'provenance.v1.json',
    'oracle/src/main.rs'
  ];
  assert.deepEqual(
    inputs.map(([name]) => name),
    names
  );
  assert.deepEqual(Object.keys(r.inputs).sort(), [...names].sort());
  for (const [name, bytes] of inputs)
    assert.equal(r.inputs[name], digest(bytes), name);
  assert.equal(r.result.revision, revision);
  assert.equal(r.result.source, producer);
  assert.equal(r.result.signing, 'NOT_RUN_NO_KEYS');
  assert.equal(web.length, 16);
  assert.equal(r.result.templates.length, web.length);
  assert.equal(new Set(web.map((row) => row.id)).size, web.length);
  assert.equal(
    new Set(r.result.templates.map((row) => row.id)).size,
    web.length
  );
  for (const [index, row] of web.entries()) {
    const actual = r.result.templates[index];
    assert.equal(actual.id, row.id);
    assert.equal(actual.result, 'accepted');
    assert.deepEqual(actual.wire_parts, row.wire_parts, row.id);
  }
}
export function createFoodReplay(
  result: unknown,
  web: Templates,
  inputs: Inputs
) {
  const receipt: Replay = {
    schema_version: 1,
    revision,
    qualification: 'FIXTURE_ONLY_NOT_DEPLOYED_TERA',
    signing: 'NOT_RUN_NO_KEYS',
    web_input_sha256: digest(JSON.stringify(web, null, 2) + '\n'),
    inputs: Object.fromEntries(
      inputs.map(([name, bytes]) => [name, digest(bytes)])
    ),
    result: result as RustResult
  };
  checkFoodReplay(receipt, web, inputs);
  return receipt;
}
