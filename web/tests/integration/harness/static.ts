import { createServer } from 'node:http';
import { once } from 'node:events';
import { readFile, realpath, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// HC_TEST_ONLY_STATIC_SERVER: target-neutral static host contract, not an app server.
// Admission is URL syntax only; client route owners validate coordinates and authority.
function isNavigationPath(pathname: string) {
  if (new TextEncoder().encode(pathname).length > 2048) return false;
  const segments = pathname.slice(1).split('/');
  if (segments.some((segment) => !/^[A-Za-z0-9_-]+$/.test(segment)))
    return pathname === '/';
  const publicShells = new Set([
    'search',
    'sell',
    'selling',
    'messages',
    'about',
    'privacy'
  ]);
  return (
    (segments.length === 1 && publicShells.has(segments[0])) ||
    (segments[0] === 'products' &&
      (segments.length === 2 ||
        (segments.length === 3 && segments[2] === 'edit'))) ||
    (segments[0] === 'selling' &&
      segments[1] === 'drafts' &&
      segments.length === 3) ||
    (segments[0] === 'messages' && segments.length === 2)
  );
}

function acceptsHtml(accept: string) {
  return accept.split(',').some((entry) => {
    const [type, ...parameters] = entry.trim().split(';');
    if (type?.trim().toLowerCase() !== 'text/html') return false;
    const quality = parameters.find((parameter) =>
      parameter.trim().startsWith('q=')
    );
    return !quality || Number(quality.trim().slice(2)) > 0;
  });
}

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
      // Validate before URL parsing can normalize literal or encoded dot segments.
      const raw = (request.url ?? '/').split('?')[0];
      if (!raw.startsWith('/') || /%2f|%5c|%25/i.test(raw)) {
        response.writeHead(404).end();
        return;
      }
      const requested = decodeURIComponent(raw);
      if (
        /[\\#]/u.test(requested) ||
        [...requested].some(
          (character) =>
            character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127
        ) ||
        requested
          .split('/')
          .some((segment) => segment === '.' || segment === '..')
      ) {
        response.writeHead(404).end();
        return;
      }
      async function existing(relative: string) {
        try {
          const candidate = await realpath(path.resolve(root, '.' + relative));
          if (
            !candidate.startsWith(root + path.sep) ||
            !(await stat(candidate)).isFile()
          )
            return null;
          return candidate;
        } catch {
          return null;
        }
      }
      let file = await existing(requested === '/' ? '/index.html' : requested);
      const navigation = isNavigationPath(requested);
      if (!file && navigation) {
        file =
          (await existing(requested + '.html')) ??
          (await existing(requested + '/index.html'));
      }
      if (
        !file &&
        navigation &&
        acceptsHtml(request.headers.accept ?? '') &&
        (!request.headers['sec-fetch-dest'] ||
          request.headers['sec-fetch-dest'] === 'document')
      ) {
        file = await existing('/200.html');
      }
      if (!file) {
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
