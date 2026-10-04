import { once } from 'node:events';
import { WebSocketServer, type WebSocket } from 'ws';

// HC_TEST_ONLY_RELAY: minimal framing fixture, not a Nostr implementation.
export async function createRelayHarness() {
  const server = new WebSocketServer({
    host: '127.0.0.1',
    port: 0,
    maxPayload: 4096
  });
  const subscriptions = new Map<WebSocket, Set<string>>();
  server.on('connection', (socket) => {
    const owned = new Set<string>();
    subscriptions.set(socket, owned);
    socket.on('close', () => subscriptions.delete(socket));
    socket.on('message', (bytes) => {
      try {
        const buffer = Array.isArray(bytes)
          ? Buffer.concat(bytes)
          : Buffer.isBuffer(bytes)
            ? bytes
            : Buffer.from(bytes);
        const frame: unknown = JSON.parse(buffer.toString('utf8'));
        if (
          !Array.isArray(frame) ||
          typeof frame[1] !== 'string' ||
          frame[1].length > 64
        )
          return socket.close(1008, 'invalid fixture frame');
        if (
          frame[0] === 'REQ' &&
          frame.length === 3 &&
          typeof frame[2] === 'object' &&
          frame[2] !== null &&
          !Array.isArray(frame[2])
        ) {
          if (owned.size >= 4)
            return socket.close(1008, 'fixture subscription limit');
          owned.add(frame[1]);
          socket.send(JSON.stringify(['EOSE', frame[1]]));
        } else if (frame[0] === 'CLOSE' && frame.length === 2)
          owned.delete(frame[1]);
        else socket.close(1008, 'unsupported fixture frame');
      } catch {
        socket.close(1008, 'invalid fixture JSON');
      }
    });
  });
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('Missing loopback port');
  return {
    url: `ws://127.0.0.1:${address.port}`,
    state: () => ({
      listening: server.address() !== null,
      connections: server.clients.size,
      subscriptions: [...subscriptions.values()].reduce(
        (total, ids) => total + ids.size,
        0
      ),
      childProcesses: 0
    }),
    async close() {
      const sockets = [...server.clients];
      const closed = sockets.map((socket) => once(socket, 'close'));
      for (const socket of sockets) socket.terminate();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve()))
      );
      await Promise.all(closed);
      subscriptions.clear();
    }
  };
}
