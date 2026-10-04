import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';

// Bootstrap output conformance only; Food/Message oracle parity is not qualified.
await test('actual static shell is prerendered and excludes controlled harness code', async () => {
  const root = new URL('../../build/', import.meta.url);
  const files = await readdir(root, { recursive: true, withFileTypes: true });
  assert.ok(
    files.some((entry) => entry.isFile() && entry.name === 'index.html')
  );
  const html = await readFile(new URL('index.html', root), 'utf8');
  assert.match(html, /<h1>HarvestCircle<\/h1>/);
  assert.match(html, /<title>HarvestCircle<\/title>/);
  for (const entry of files) {
    if (!entry.isFile()) continue;
    const content = await readFile(`${entry.parentPath}/${entry.name}`, 'utf8');
    assert.doesNotMatch(
      content,
      /HC_TEST_ONLY_|createRelayHarness|createStaticHarness|installControlledProvider/
    );
  }
});
