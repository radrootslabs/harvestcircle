import { test } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { request } from 'node:http';
import { readFile } from 'node:fs/promises';
import WebSocket from 'ws';
import { createRelayHarness } from './harness/relay.ts';
import { createStaticHarness } from './harness/static.ts';

await test('real loopback REQ/CLOSE and active teardown release owned resources', async () => {
  const relay = await createRelayHarness();
  const socket = new WebSocket(relay.url);
  try {
    await once(socket, 'open');
    let reply = once(socket, 'message');
    socket.send(JSON.stringify(['REQ', 'first', {}]));
    assert.deepEqual(JSON.parse(String((await reply)[0])), ['EOSE', 'first']);
    assert.equal(relay.state().subscriptions, 1);
    socket.send(JSON.stringify(['CLOSE', 'first']));
    reply = once(socket, 'message');
    socket.send(JSON.stringify(['REQ', 'second', {}]));
    await reply; // Ordered frames prove CLOSE removed first before REQ second.
    assert.equal(relay.state().subscriptions, 1);
  } finally {
    const closed = once(socket, 'close');
    await relay.close();
    await closed;
  }
  assert.deepEqual(relay.state(), {
    listening: false,
    connections: 0,
    subscriptions: 0,
    childProcesses: 0
  });
  assert.equal(socket.readyState, WebSocket.CLOSED);
});

await test('navigation fallback serves actual separate output and preserves missing assets', async () => {
  const fallback = await readFile(
    new URL('../../build/200.html', import.meta.url),
    'utf8'
  );
  const search = await readFile(
    new URL('../../build/search.html', import.meta.url),
    'utf8'
  );
  const prerendered = new Map([['/search', search]]);
  for (const pathname of [
    '/sell',
    '/selling',
    '/messages',
    '/about',
    '/privacy'
  ]) {
    prerendered.set(
      pathname,
      await readFile(
        new URL('../../build' + pathname + '.html', import.meta.url),
        'utf8'
      )
    );
  }
  for (const pathname of ['/sell', '/selling', '/messages']) {
    const html = prerendered.get(pathname);
    assert.ok(html);
    assert.match(html, /<title>HarvestCircle<\/title>/);
    assert.match(html, /name="robots" content="noindex"/);
    assert.match(
      html,
      /Private views and editing are unavailable during development/
    );
    assert.doesNotMatch(html, /<form|<input|<textarea/);
  }
  assert.notEqual(search, fallback);
  assert.match(search, /<title>Search food — HarvestCircle<\/title>/);
  assert.match(search, /Search sources are unavailable\./);
  const document = { accept: 'text/html', 'sec-fetch-dest': 'document' };
  const server = await createStaticHarness();
  try {
    for (const pathname of [
      '/search?q=food',
      '/sell',
      '/selling',
      '/messages',
      '/about',
      '/privacy',
      '/products/unsupported-coordinate',
      '/products/unsupported-coordinate/edit',
      '/selling/drafts/unknown-draft',
      '/messages/unknown-conversation'
    ]) {
      const result = await fetch(server.url + pathname, { headers: document });
      assert.equal(result.status, 200, pathname);
      assert.equal(result.headers.get('content-type'), 'text/html');
      assert.equal(
        await result.text(),
        prerendered.get(new URL(pathname, server.url).pathname) ?? fallback
      );
    }
    for (const pathname of [
      '/missing.js',
      '/missing.css',
      '/missing.json',
      '/missing.png',
      '/_app/missing',
      '/assets/missing',
      '/products/missing.js',
      '/unowned',
      '/products/',
      '/products/a/extra',
      '/products/a.b'
    ]) {
      const result = await fetch(server.url + pathname, { headers: document });
      assert.equal(result.status, 404, pathname);
      assert.doesNotMatch(result.headers.get('content-type') ?? '', /html/);
      assert.equal(await result.text(), '');
    }
    const rejectedHeaders: Record<string, string>[] = [
      { accept: '*/*' },
      { accept: 'application/json' },
      { accept: 'text/html;q=0' },
      { accept: 'text/html', 'sec-fetch-dest': 'script' }
    ];
    for (const headers of rejectedHeaders) {
      assert.equal(
        (await fetch(server.url + '/products/a', { headers })).status,
        404
      );
    }
    assert.equal(
      (
        await fetch(server.url + '/products/a', {
          method: 'POST',
          headers: document
        })
      ).status,
      405
    );
    // node:http retains raw paths that URL/fetch would normalize before sending.
    for (const pathname of [
      '/../index.html',
      '/products/../index.html',
      '/%2e%2e/index.html',
      '/products%2fa',
      '/products/%252e%252e',
      '/products/%5cfoo',
      '/products/%00',
      '/products/%zz',
      '/products/' + 'a'.repeat(2048)
    ]) {
      const status = await new Promise<number | undefined>(
        (resolve, reject) => {
          const req = request(
            server.url,
            { path: pathname, headers: document },
            (response) => {
              response.resume();
              response.on('end', () => resolve(response.statusCode));
            }
          );
          req.on('error', reject);
          req.end();
        }
      );
      assert.equal(status, 404, pathname);
    }
    const metadata = await fetch(server.url + '/build-info.json', {
      headers: document
    });
    assert.equal(metadata.status, 200);
    assert.equal(metadata.headers.get('content-type'), 'application/json');
    assert.deepEqual(
      await metadata.json(),
      JSON.parse(
        await readFile(
          new URL('../../build/build-info.json', import.meta.url),
          'utf8'
        )
      )
    );
  } finally {
    await server.close();
  }
  assert.deepEqual(server.state(), { listening: false, childProcesses: 0 });
});

await test('loopback static fixture serves actual output and closes its listener', async () => {
  const server = await createStaticHarness();
  try {
    const result = await fetch(server.url);
    assert.equal(result.status, 200);
    assert.match(
      await result.text(),
      /<a[^>]*href="\/"[^>]*>HarvestCircle<\/a>/
    );
    assert.equal((await fetch(`${server.url}/missing.js`)).status, 404);
  } finally {
    await server.close();
  }
  assert.deepEqual(server.state(), { listening: false, childProcesses: 0 });
  await assert.rejects(fetch(server.url));
});
