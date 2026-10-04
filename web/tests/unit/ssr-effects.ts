import { vi } from 'vitest';

// Node has no window or document; ordinary feature detection remains safe.
// Imports and server rendering must not acquire browser capabilities or egress.
for (const name of ['indexedDB', 'WebSocket', 'nostr']) {
  Object.defineProperty(globalThis, name, {
    configurable: true,
    get() {
      throw new Error(`SSR accessed ${name}`);
    }
  });
}
vi.stubGlobal('fetch', () => {
  throw new Error('SSR attempted network access');
});
