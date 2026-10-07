import assert from 'node:assert/strict';
import test from 'node:test';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
const root = new URL('../../../contracts/interop/message/', import.meta.url);
void test('Message fixture provenance binds five exact portable producer inputs', () => {
  const p = JSON.parse(
    readFileSync(new URL('provenance.v1.json', root), 'utf8')
  );
  assert.equal(p.revision, '189c49b74b4bafc142b00b76b296477931139e72');
  assert.equal(Object.keys(p.inputs).length, 5);
  for (const [name, hash] of Object.entries(p.inputs))
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(name, root)))
        .digest('hex'),
      hash,
      name
    );
  assert.match(p.qualification, /actual pinned Rust/);
  assert.ok(!JSON.stringify(p).includes('/Users/'));
});
void test('explicit Message interop runner rejects unapproved arguments before Cargo or generation', () => {
  const before = readFileSync(new URL('corpus.v1.json', root));
  const result = spawnSync(
    process.execPath,
    ['tools/check-message-interop.mjs', '--other'],
    {
      cwd: new URL('../../', import.meta.url),
      encoding: 'utf8',
      timeout: 10000
    }
  );
  assert.equal(result.status, 1);
  assert.match(result.stderr, /expected no arguments or explicit --write/);
  assert.deepEqual(readFileSync(new URL('corpus.v1.json', root)), before);
});
