import { test } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
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

await test('loopback static fixture serves actual output and closes its listener', async () => {
  const server = await createStaticHarness();
  try {
    const result = await fetch(server.url);
    assert.equal(result.status, 200);
    assert.match(await result.text(), /<h1>HarvestCircle<\/h1>/);
    assert.equal((await fetch(`${server.url}/missing.js`)).status, 404);
  } finally {
    await server.close();
  }
  assert.deepEqual(server.state(), { listening: false, childProcesses: 0 });
  await assert.rejects(fetch(server.url));
});
