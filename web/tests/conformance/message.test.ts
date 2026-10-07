import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { messageCorpus, actualWebMessage } from './message-cases.ts';
const corpus = messageCorpus();
void test('Message vectors bind actual pinned Rust source recipes', () => {
  assert.equal(corpus.revision, '189c49b74b4bafc142b00b76b296477931139e72');
  assert.equal(
    corpus.source_sha256,
    createHash('sha256')
      .update(
        readFileSync(
          new URL(
            '../../../contracts/interop/message/source_profile.v1.json',
            import.meta.url
          )
        )
      )
      .digest('hex')
  );
  assert.ok(corpus.vectors.length > 20);
  assert.equal(
    new Set(corpus.vectors.map((v) => v.id)).size,
    corpus.vectors.length
  );
});
for (const vector of corpus.vectors)
  void test('Message actual Rust/web case ' + vector.id, () => {
    const actual = actualWebMessage(vector);
    assert.equal(actual.status, vector.web_status);
    if (vector.web_status === 'supported') {
      assert.equal(vector.expected.status, 'supported');
      const { error, ...expected } = vector.expected;
      assert.equal(error, undefined);
      assert.deepEqual(actual, expected);
    } else if (vector.expected.status === 'supported')
      assert.ok(
        vector.policy_difference,
        'Additional canonical/budget/schema policy must be explicit rather than forcing Lib equality'
      );
  });
