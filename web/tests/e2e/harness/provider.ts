import type { BrowserContext } from '@playwright/test';

// HC_TEST_ONLY_PROVIDER: public-only injection; never signs or holds secret keys.
export async function installControlledProvider(context: BrowserContext) {
  await context.addInitScript(() => {
    Object.defineProperty(window, 'nostr', {
      configurable: true,
      value: Object.freeze({
        fixture: 'HC_TEST_ONLY_PROVIDER',
        getPublicKey: () => Promise.resolve('11'.repeat(32)),
        signEvent: () =>
          Promise.reject(new Error('Controlled fixture cannot sign'))
      })
    });
  });
}
