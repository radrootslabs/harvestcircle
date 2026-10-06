import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { webTemplates } from './food-writer-cases.ts';
import { checkFoodReplay } from './food-bidirectional.ts';

const root = new URL(
  '../../../contracts/interop/food_availability/',
  import.meta.url
);
const receipt = JSON.parse(
  await readFile(new URL('./food-writer-rust.v1.json', import.meta.url), 'utf8')
) as unknown;
const hashes = await Promise.all(
  [
    'corpus.v1.json',
    'source_profile.v1.json',
    'provenance.v1.json',
    'oracle/src/main.rs'
  ].map(async (name) => [name, await readFile(new URL(name, root))] as const)
);
void test('actual website templates replay exact checked pinned Rust consumption without signing or deployment', () => {
  checkFoodReplay(receipt, webTemplates, hashes);
});
for (const mutation of ['tag', 'amount'] as const)
  void test(`bidirectional fixture comparison fails after deliberate ${mutation} mutation`, () => {
    const altered = webTemplates.map((row) => ({
      ...row,
      wire_parts: {
        ...row.wire_parts,
        tags: row.wire_parts.tags.map((tag) => [...tag])
      }
    }));
    const price = altered[0].wire_parts.tags.find((tag) => tag[0] === 'price')!;
    if (mutation === 'tag') price[0] = 'different-price';
    else price[1] = '1';
    assert.throws(() => checkFoodReplay(receipt, altered, hashes));
  });
void test('replay refuses wrong public revision, producer identity and fixture-only claims', () => {
  const original = receipt as {
    result: Record<string, unknown>;
    inputs: Record<string, string>;
  };
  for (const change of [
    { result: { ...original.result, source: 'fixture_echo' } },
    { inputs: { ...original.inputs, 'oracle/src/main.rs': '0'.repeat(64) } },
    { revision: '0'.repeat(40) },
    { qualification: 'DEPLOYED_TERA' },
    { signing: 'SIGNED' },
    { web_input_sha256: '0'.repeat(64) }
  ])
    assert.throws(() =>
      checkFoodReplay(
        { ...(receipt as object), ...change },
        webTemplates,
        hashes
      )
    );
});
void test('replay refuses missing duplicate or altered Rust output cases', () => {
  const original = receipt as {
    result: { templates: { id: string; wire_parts: { content: string } }[] };
  };
  for (const mutation of ['missing', 'duplicate', 'content'] as const) {
    const changed = structuredClone(original);
    if (mutation === 'missing') changed.result.templates.pop();
    else if (mutation === 'duplicate')
      changed.result.templates[1].id = changed.result.templates[0].id;
    else changed.result.templates[0].wire_parts.content += 'changed';
    assert.throws(() => checkFoodReplay(changed, webTemplates, hashes));
  }
});
