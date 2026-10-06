import assert from 'node:assert/strict';
import { lstat, readFile } from 'node:fs/promises';
import test from 'node:test';
import { readFoodEvent } from '../../src/lib/contracts/food-availability-v1/read.ts';
import { buildFoodTemplate } from '../../src/lib/contracts/food-availability-v1/write.ts';
import {
  interopInputPaths,
  type buildInteropManifest
} from './interop-manifest.ts';
import { consumeInteropTypescript } from './interop-consumer.ts';
import { webTemplates } from './food-writer-cases.ts';
const root = new URL('../../../', import.meta.url);
const inputs = new Map<string, Uint8Array>();
for (const path of interopInputPaths) {
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
const manifest: unknown = JSON.parse(
  await readFile(new URL('contracts/interop/manifest.json', root), 'utf8')
);
void test('shared manifest executes actual TS adapters and reports every unsupported counterpart', () => {
  const result = consumeInteropTypescript(manifest, inputs);
  assert.equal(result.executed_cases, 40);
  assert.equal(result.unsupported_cases, 26);
  assert.equal(new Set(result.cases.map((row) => row.id)).size, 66);
  console.log(JSON.stringify(result));
});
void test('changed actual reader projection fails against the pinned oracle expectation', () => {
  assert.throws(() =>
    consumeInteropTypescript(manifest, inputs, {
      read: (raw) => {
        const actual = readFoodEvent(raw);
        return actual.outcome === 'admitted'
          ? {
              ...actual,
              projection: {
                ...actual.projection,
                location: 'deliberately changed reader semantics'
              }
            }
          : actual;
      }
    })
  );
});
void test('changed actual writer output fails against retained Rust wire parts', () => {
  const writers = structuredClone(webTemplates);
  writers[0] = {
    ...writers[0],
    wire_parts: {
      ...writers[0].wire_parts,
      content:
        writers[0].wire_parts.content + 'deliberately changed writer semantics'
    }
  };
  assert.throws(() => consumeInteropTypescript(manifest, inputs, { writers }));
});
void test('changed actual boundary builder output fails rather than accepting matching metadata', () => {
  assert.throws(() =>
    consumeInteropTypescript(manifest, inputs, {
      build: (draft) => {
        const actual = buildFoodTemplate(draft);
        return actual.ok
          ? {
              ...actual,
              wire_parts: {
                ...actual.wire_parts,
                tags: actual.wire_parts.tags.map((tag) =>
                  tag[0] === 'published_at' ? [tag[0], '2'] : tag
                )
              }
            }
          : actual;
      }
    })
  );
});
void test('unsafe JavaScript timestamps cannot be promoted from unsupported to accepted', () => {
  assert.throws(() =>
    consumeInteropTypescript(manifest, inputs, {
      build: (draft) => {
        const actual = buildFoodTemplate(draft);
        return Number.isSafeInteger(draft.published_at)
          ? actual
          : buildFoodTemplate({ ...draft, published_at: 1, created_at: 1 });
      }
    })
  );
});
void test('zero matching profile cases fails', () => {
  assert.throws(
    () =>
      consumeInteropTypescript(manifest, inputs, {
        profile: 'unmatched_profile'
      }),
    /zero matching/
  );
});
void test('changed manifest vector hash fails the actual TS consumer', () => {
  const changed = structuredClone(manifest) as ReturnType<
    typeof buildInteropManifest
  >;
  changed.cases[0].fixture.sha256 = '0'.repeat(64);
  assert.throws(() => consumeInteropTypescript(changed, inputs));
});
void test('changed raw vector bytes fail the actual TS consumer', () => {
  const changed = new Map(inputs);
  const path = interopInputPaths[0];
  const bytes = inputs.get(path)!;
  changed.set(path, new Uint8Array([...bytes, 32]));
  assert.throws(() => consumeInteropTypescript(manifest, changed));
});
