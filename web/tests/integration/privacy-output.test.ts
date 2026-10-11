import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createStaticHarness } from './harness/static.ts';

await test('HCP128 actual static Messages response keeps generic noindex metadata and condition copy', async () => {
  const server = await createStaticHarness();
  try {
    const response = await fetch(server.url + '/messages', {
      headers: { Accept: 'text/html' }
    });
    assert.equal(response.status, 200);
    const html = await response.text();
    assert.match(html, /<title>HarvestCircle<\/title>/);
    assert.match(html, /<meta name="robots" content="noindex"/);
    assert.ok(
      html.includes(
        'Messages stay hidden until you connect and explicitly unlock your inbox.'
      )
    );
    assert.ok(!html.includes('unavailable during development'));
  } finally {
    await server.close();
  }
});

await test('HCP128 every actual static artifact excludes private fixture bodies and production fixture secrets', async () => {
  const root = fileURLToPath(new URL('../../build/', import.meta.url));
  let checked = 0;
  async function inspect(directory: string) {
    for (const item of await readdir(directory, { withFileTypes: true })) {
      const file = path.join(directory, item.name);
      assert.ok(!item.isSymbolicLink());
      if (item.isDirectory()) await inspect(file);
      else {
        assert.ok(item.isFile());
        const content = await readFile(file, 'utf8');
        assert.doesNotMatch(
          content,
          /HCP104_UNSENT_MEMORY_ONLY|PRIVATE OWNER B SENTINEL|HC_TEST_ONLY_SECRET|-----BEGIN (?:[A-Z ]*PRIVATE KEY|OPENSSH)/
        );
        checked++;
      }
    }
  }
  await inspect(root);
  assert.ok(checked > 0, 'must inspect actual generated static files');
});
