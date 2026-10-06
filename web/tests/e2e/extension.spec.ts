import { expect, test, type Page } from '@playwright/test';
import { build } from 'vite';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createStaticHarness } from '../integration/harness/static.ts';
import type * as Identity from '../../src/lib/runtime/identity-session.ts';
declare global {
  interface Window {
    hcp051: typeof Identity;
    hcp051Session: Identity.IdentitySession;
    hcp051Counts: {
      keys: number;
      signs: number;
      encrypts: number;
      decrypts: number;
    };
    hcp051Key: string;
    hcp051Release: () => void;
  }
}
const owner =
  '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
const peer = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
let server: Awaited<ReturnType<typeof createStaticHarness>>, bundle: string;
test.beforeAll(async () => {
  server = await createStaticHarness();
  const source = fileURLToPath(
    new URL('../../src/lib/runtime/identity-session.ts', import.meta.url)
  );
  const result = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: { entry: source, name: 'hcp051', formats: ['iife'] }
    }
  });
  const output = Array.isArray(result) ? result[0] : result;
  if (!('output' in output)) throw new Error('missing bundle');
  const chunks = output.output.filter((item) => item.type === 'chunk');
  expect(chunks).toHaveLength(1);
  const moduleIds = Object.keys(chunks[0].modules);
  expect(
    moduleIds.filter((id) =>
      /(?:^|\/)node_modules\/(?:@capacitor\/core|nostr-signer-capacitor-plugin)(?:\/|$)|\/(?:private-key-signer|password-signer|nostr-connect-signer)\.[cm]?[jt]s(?:$|\?)/.test(
        id
      )
    )
  ).toEqual([]);
  expect(moduleIds.some((id) => id.includes('extension-signer'))).toBe(true);
  bundle = chunks[0].code;
  console.log(
    JSON.stringify({
      moduleIds,
      fixture: 'HCP051_ACTUAL_EXTENSION_SIGNER_CONTROLLED_PROVIDER',
      bundleSha256: createHash('sha256').update(bundle).digest('hex'),
      claim:
        'real Chromium and installed SDK; controlled provider is not named real extension qualification'
    })
  );
});
test.afterAll(async () => server.close());
async function load(
  page: Page,
  mode:
    | 'signing'
    | 'messaging'
    | 'missing'
    | 'denied'
    | 'invalid'
    | 'pending'
    | 'connected' = 'signing'
) {
  await page.goto(server.url + '/search');
  await page.evaluate(
    ({ owner, mode }) => {
      window.hcp051Counts = { keys: 0, signs: 0, encrypts: 0, decrypts: 0 };
      window.hcp051Key = owner;
      const provider: {
        getPublicKey: () => Promise<string>;
        signEvent?: () => Promise<never>;
        nip44?: {
          encrypt: (key: string, body: string) => Promise<string>;
          decrypt: (key: string, body: string) => Promise<string>;
        };
      } = {
        async getPublicKey() {
          window.hcp051Counts.keys++;
          if (mode === 'denied') throw new Error('controlled provider refusal');
          if (mode === 'invalid') return 'f'.repeat(64);
          if (mode === 'pending')
            await new Promise<void>((resolve) => {
              window.hcp051Release = resolve;
            });
          return window.hcp051Key;
        }
      };
      if (mode !== 'connected')
        provider.signEvent = () => {
          window.hcp051Counts.signs++;
          return Promise.reject(new Error('Connect must never sign'));
        };
      if (mode === 'messaging')
        provider.nip44 = {
          encrypt(key, body) {
            window.hcp051Counts.encrypts++;
            if (key !== window.hcp051Key)
              return Promise.reject(new Error('wrong self key'));
            return Promise.resolve('controlled:' + body);
          },
          decrypt(key, body) {
            window.hcp051Counts.decrypts++;
            if (key !== window.hcp051Key)
              return Promise.reject(new Error('wrong self key'));
            return Promise.resolve(body.replace(/^controlled:/, ''));
          }
        };
      Object.defineProperty(window, 'nostr', {
        configurable: true,
        value: mode === 'missing' ? undefined : provider
      });
    },
    { owner, mode }
  );
  await page.addScriptTag({ content: bundle });
  await page.evaluate(() => {
    window.hcp051Session = window.hcp051.createIdentitySession();
  });
}
async function connect(page: Page) {
  return page.evaluate(() =>
    window.hcp051.connectIdentity(window.hcp051Session)
  );
}
test('load and construction are inert; explicit Connect obtains key once without signing or probing', async ({
  page
}) => {
  await load(page);
  expect(await page.evaluate(() => window.hcp051Counts)).toEqual({
    keys: 0,
    signs: 0,
    encrypts: 0,
    decrypts: 0
  });
  expect(await connect(page)).toMatchObject({
    state: 'signing_only',
    publicKey: owner
  });
  expect(await page.evaluate(() => window.hcp051Counts)).toEqual({
    keys: 1,
    signs: 0,
    encrypts: 0,
    decrypts: 0
  });
});
for (const [mode, reason] of [
  ['missing', 'missing'],
  ['denied', 'refused'],
  ['invalid', 'invalid_key']
] as const)
  test(`${mode} provider leaves guest with safe finite diagnostics`, async ({
    page
  }) => {
    await load(page, mode);
    expect(await connect(page)).toEqual({ state: 'guest', reason });
    expect(
      await page.evaluate(
        () => window.hcp051Counts.signs + window.hcp051Counts.encrypts
      )
    ).toBe(0);
  });
test('key-only connected state does not promise signing or messaging', async ({
  page
}) => {
  await load(page, 'connected');
  expect(await connect(page)).toMatchObject({
    state: 'connected',
    publicKey: owner
  });
});
test('fresh SDK instance detects changed provider key despite the original signer cache', async ({
  page
}) => {
  await load(page);
  await connect(page);
  await page.evaluate((peer) => {
    window.hcp051Key = peer;
  }, peer);
  expect(
    await page.evaluate(() =>
      window.hcp051.recheckIdentityOwner(window.hcp051Session)
    )
  ).toEqual({ state: 'guest', reason: 'changed_key' });
  expect(await page.evaluate(() => window.hcp051Counts.keys)).toBe(2);
});
test('messaging method presence does not probe; explicit reviewed self-copy is required', async ({
  page
}) => {
  await load(page, 'messaging');
  expect(await connect(page)).toMatchObject({ state: 'signing_only' });
  expect(
    await page.evaluate(() =>
      window.hcp051.probeIdentityMessaging(window.hcp051Session, 'unreviewed')
    )
  ).toMatchObject({ state: 'signing_only' });
  expect(await page.evaluate(() => window.hcp051Counts.encrypts)).toBe(0);
  expect(
    await page.evaluate(() =>
      window.hcp051.probeIdentityMessaging(
        window.hcp051Session,
        'reviewed_self_copy'
      )
    )
  ).toMatchObject({ state: 'messaging_capable', publicKey: owner });
  expect(await page.evaluate(() => window.hcp051Counts)).toEqual({
    keys: 3,
    signs: 0,
    encrypts: 1,
    decrypts: 1
  });
});
test('late pending connection cannot restore a disconnected owner and original slot is retained', async ({
  page
}) => {
  await load(page, 'pending');
  const waiting = connect(page);
  await expect
    .poll(() => page.evaluate(() => window.hcp051Counts.keys))
    .toBe(1);
  await page.evaluate(() =>
    window.hcp051.disconnectIdentity(window.hcp051Session)
  );
  expect(await connect(page)).toEqual({
    state: 'guest',
    reason: 'disconnected'
  });
  expect(await page.evaluate(() => window.hcp051Counts.keys)).toBe(1);
  await page.evaluate(() => window.hcp051Release());
  expect(await waiting).toEqual({ state: 'guest', reason: 'disconnected' });
});
test('missing NIP44 and refusal keep signing-only; no legacy encryption or automatic action', async ({
  page
}) => {
  await load(page);
  await connect(page);
  expect(
    await page.evaluate(() =>
      window.hcp051.probeIdentityMessaging(
        window.hcp051Session,
        'reviewed_self_copy'
      )
    )
  ).toMatchObject({ state: 'signing_only' });
  expect(await page.evaluate(() => window.hcp051Counts.encrypts)).toBe(0);
});

test('reviewed encryption refusal retains public signing candidate with safe messaging refusal state', async ({
  page
}) => {
  await load(page, 'messaging');
  await connect(page);
  await page.evaluate(() => {
    const provider = (
      window as unknown as {
        nostr: { nip44: { encrypt: () => Promise<never> } };
      }
    ).nostr;
    provider.nip44.encrypt = () => {
      window.hcp051Counts.encrypts++;
      return Promise.reject(
        new Error('controlled private refusal text must not be exposed')
      );
    };
  });
  const state = await page.evaluate(() =>
    window.hcp051.probeIdentityMessaging(
      window.hcp051Session,
      'reviewed_self_copy'
    )
  );
  expect(state).toMatchObject({
    state: 'signing_only',
    publicKey: owner,
    messaging: 'refused'
  });
  expect(JSON.stringify(state)).not.toContain('controlled private');
  expect(await page.evaluate(() => window.hcp051Counts.decrypts)).toBe(0);
});
test('disconnect during pending encryption keeps slot and prevents later decrypt or capability resurrection', async ({
  page
}) => {
  await load(page, 'messaging');
  await connect(page);
  await page.evaluate(() => {
    const provider = (
      window as unknown as {
        nostr: {
          nip44: { encrypt: (key: string, body: string) => Promise<string> };
        };
      }
    ).nostr;
    provider.nip44.encrypt = (_key, body) => {
      window.hcp051Counts.encrypts++;
      return new Promise((resolve) => {
        window.hcp051Release = () => resolve('controlled:' + body);
      });
    };
  });
  const pending = page.evaluate(() =>
    window.hcp051.probeIdentityMessaging(
      window.hcp051Session,
      'reviewed_self_copy'
    )
  );
  await expect
    .poll(() => page.evaluate(() => window.hcp051Counts.encrypts))
    .toBe(1);
  await page.evaluate(() =>
    window.hcp051.disconnectIdentity(window.hcp051Session)
  );
  expect(await connect(page)).toEqual({
    state: 'guest',
    reason: 'disconnected'
  });
  await page.evaluate(() => window.hcp051Release());
  expect(await pending).toEqual({ state: 'guest', reason: 'disconnected' });
  expect(await page.evaluate(() => window.hcp051Counts.decrypts)).toBe(0);
});
test('repeat Connect detects a changed owner instead of silently adopting it', async ({
  page
}) => {
  await load(page);
  await connect(page);
  await page.evaluate((peer) => {
    window.hcp051Key = peer;
  }, peer);
  expect(await connect(page)).toEqual({
    state: 'guest',
    reason: 'changed_key'
  });
});

for (const property of ['nostr', 'getPublicKey', 'signEvent'] as const)
  test(`reentrant ${property} inspection disconnects before any new key request`, async ({
    page
  }) => {
    await load(page);
    await page.evaluate((property) => {
      const provider = window.nostr as {
        getPublicKey: () => Promise<string>;
        signEvent: () => Promise<never>;
      };
      const value = property === 'nostr' ? provider : provider[property];
      Object.defineProperty(
        property === 'nostr' ? window : provider,
        property,
        {
          configurable: true,
          get() {
            window.hcp051.disconnectIdentity(window.hcp051Session);
            return value;
          }
        }
      );
    }, property);
    expect(await connect(page)).toEqual({
      state: 'guest',
      reason: 'disconnected'
    });
    expect(await page.evaluate(() => window.hcp051Counts.keys)).toBe(0);
  });

for (const property of ['nip44', 'encrypt', 'decrypt'] as const)
  test(`reentrant ${property} inspection disconnects before any new encryption`, async ({
    page
  }) => {
    await load(page, 'messaging');
    await connect(page);
    await page.evaluate((property) => {
      const provider = window.nostr as {
        nip44: {
          encrypt: (key: string, body: string) => Promise<string>;
          decrypt: (key: string, body: string) => Promise<string>;
        };
      };
      const cipher = provider.nip44;
      const value = property === 'nip44' ? cipher : cipher[property];
      Object.defineProperty(
        property === 'nip44' ? provider : cipher,
        property,
        {
          configurable: true,
          get() {
            window.hcp051.disconnectIdentity(window.hcp051Session);
            return value;
          }
        }
      );
    }, property);
    expect(
      await page.evaluate(() =>
        window.hcp051.probeIdentityMessaging(
          window.hcp051Session,
          'reviewed_self_copy'
        )
      )
    ).toEqual({ state: 'guest', reason: 'disconnected' });
    expect(await page.evaluate(() => window.hcp051Counts.encrypts)).toBe(0);
    expect(await page.evaluate(() => window.hcp051Counts.decrypts)).toBe(0);
  });

test('Connect refreshes signing shape after awaited provider key resolution', async ({
  page
}) => {
  await load(page, 'pending');
  const waiting = connect(page);
  await expect
    .poll(() => page.evaluate(() => window.hcp051Counts.keys))
    .toBe(1);
  await page.evaluate(() => {
    const provider = window.nostr as { signEvent?: unknown };
    delete provider.signEvent;
    window.hcp051Release();
  });
  expect(await waiting).toMatchObject({ state: 'connected', publicKey: owner });
});

test('reentrant recheck shape inspection cannot restore a disconnected capability', async ({
  page
}) => {
  await load(page);
  await connect(page);
  await page.evaluate(() => {
    const provider = window.nostr as { signEvent: () => Promise<never> };
    const value = provider.signEvent;
    Object.defineProperty(provider, 'signEvent', {
      get() {
        window.hcp051.disconnectIdentity(window.hcp051Session);
        return value;
      }
    });
  });
  expect(
    await page.evaluate(() =>
      window.hcp051.recheckIdentityOwner(window.hcp051Session)
    )
  ).toEqual({ state: 'guest', reason: 'disconnected' });
});

for (const property of ['nip44', 'encrypt', 'decrypt'] as const)
  test(`same-key recheck downgrades removed ${property} without automatic reprobe`, async ({
    page
  }) => {
    await load(page, 'messaging');
    await connect(page);
    expect(
      await page.evaluate(() =>
        window.hcp051.probeIdentityMessaging(
          window.hcp051Session,
          'reviewed_self_copy'
        )
      )
    ).toMatchObject({ state: 'messaging_capable' });
    await page.evaluate((property) => {
      const provider = window.nostr as {
        nip44?: { encrypt?: unknown; decrypt?: unknown };
      };
      if (property === 'nip44') delete provider.nip44;
      else if (provider.nip44) delete provider.nip44[property];
    }, property);
    expect(
      await page.evaluate(() =>
        window.hcp051.recheckIdentityOwner(window.hcp051Session)
      )
    ).toMatchObject({
      state: 'signing_only',
      publicKey: owner,
      messaging: 'unsupported'
    });
    expect(await page.evaluate(() => window.hcp051Counts.encrypts)).toBe(1);
    expect(await page.evaluate(() => window.hcp051Counts.decrypts)).toBe(1);
  });

test('disconnect inside an already admitted SDK key call rejects its result without another effect', async ({
  page
}) => {
  await load(page);
  await page.evaluate(() => {
    const provider = window.nostr as { getPublicKey: () => Promise<string> };
    provider.getPublicKey = () => {
      window.hcp051Counts.keys++;
      window.hcp051.disconnectIdentity(window.hcp051Session);
      return Promise.resolve(window.hcp051Key);
    };
  });
  expect(await connect(page)).toEqual({
    state: 'guest',
    reason: 'disconnected'
  });
  expect(await page.evaluate(() => window.hcp051Counts)).toEqual({
    keys: 1,
    signs: 0,
    encrypts: 0,
    decrypts: 0
  });
});

for (const property of ['nip44', 'signEvent'] as const)
  test(`final probe key response removes ${property} before capability publication`, async ({
    page
  }) => {
    await load(page, 'messaging');
    await connect(page);
    await page.evaluate((property) => {
      const provider = window.nostr as {
        getPublicKey: () => Promise<string>;
        nip44?: unknown;
        signEvent?: unknown;
      };
      provider.getPublicKey = () => {
        window.hcp051Counts.keys++;
        if (window.hcp051Counts.keys === 3) delete provider[property];
        return Promise.resolve(window.hcp051Key);
      };
    }, property);
    const result = await page.evaluate(() =>
      window.hcp051.probeIdentityMessaging(
        window.hcp051Session,
        'reviewed_self_copy'
      )
    );
    expect(result).toMatchObject(
      property === 'nip44'
        ? { state: 'signing_only', messaging: 'unsupported' }
        : { state: 'connected' }
    );
    expect(await page.evaluate(() => window.hcp051Counts)).toEqual({
      keys: 3,
      signs: 0,
      encrypts: 1,
      decrypts: 1
    });
  });
