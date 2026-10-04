import { createServer } from 'node:http';
import { once } from 'node:events';
import { readFile, realpath } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// HC_TEST_ONLY_STATIC_SERVER: exact static files only; no dynamic fallback.
export async function createStaticHarness() {
  const root = await realpath(
    fileURLToPath(new URL('../../../build/', import.meta.url))
  );
  const server = createServer((request, response) => {
    void (async () => {
      if (request.method !== 'GET') {
        response.writeHead(405).end();
        return;
      }
      const url = new URL(request.url ?? '/', 'http://127.0.0.1');
      const requested = decodeURIComponent(url.pathname);
      const file = await realpath(
        path.resolve(
          root,
          '.' + (requested === '/' ? '/index.html' : requested)
        )
      );
      if (!file.startsWith(root + path.sep)) {
        response.writeHead(404).end();
        return;
      }
      const types: Record<string, string> = {
        '.html': 'text/html',
        '.js': 'text/javascript',
        '.css': 'text/css',
        '.json': 'application/json'
      };
      response.writeHead(200, {
        'content-type': types[path.extname(file)] ?? 'application/octet-stream'
      });
      response.end(await readFile(file));
    })().catch(() => response.writeHead(404).end());
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('Missing loopback port');
  return {
    url: `http://127.0.0.1:${address.port}`,
    state: () => ({ listening: server.listening, childProcesses: 0 }),
    async close() {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve()))
      );
    }
  };
}
