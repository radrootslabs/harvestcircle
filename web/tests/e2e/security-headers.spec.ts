import { expect, test } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { prepareSecurityHeaders } from '../../tools/security-headers.mjs';
import { encodeProductReference } from '../../src/lib/nostr/references.ts';
import { createStaticHarness } from '../integration/harness/static.ts';

const web = fileURLToPath(new URL('../../', import.meta.url));
const coordinate = encodeProductReference({
  kind: 30402,
  pubkey: '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
  identifier: 'HCP127_TEST_ONLY'
});
if (!coordinate) throw new Error('Missing qualified coordinate');
const handle = '12345678-1234-4234-8234-123456789abc';
const routes = [
  '/',
  '/search',
  '/products/' + coordinate,
  '/sell',
  '/selling',
  '/selling/drafts/' + handle,
  '/products/' + coordinate + '/edit',
  '/messages',
  '/messages/' + handle,
  '/about',
  '/privacy'
];

test('actual hashed HTTP CSP hydrates all eleven direct routes, refresh and client navigation', async ({
  browser
}) => {
  const prepared = await prepareSecurityHeaders(web);
  const server = await createStaticHarness({ headers: prepared.headers });
  const context = await browser.newContext();
  const external: string[] = [];
  const errors: string[] = [];
  try {
    await context.route('**/*', (route) => {
      if (new URL(route.request().url()).origin === server.url)
        return route.continue();
      external.push(route.request().url());
      return route.abort();
    });
    await context.addInitScript(() => {
      Reflect.set(window, 'hcp127Violations', []);
      document.addEventListener('securitypolicyviolation', (event) => {
        const rows = Reflect.get(window, 'hcp127Violations') as string[];
        rows.push(event.effectiveDirective);
      });
    });
    const page = await context.newPage();
    page.on('pageerror', (error) => errors.push(error.message));
    for (const pathname of routes) {
      for (const navigate of [
        () => page.goto(server.url + pathname),
        () => page.reload()
      ]) {
        const response = await navigate();
        expect(response?.status()).toBe(200);
        expect(response?.headers()['content-security-policy']).toBe(
          prepared.headers['Content-Security-Policy']
        );
        await expect(
          page.getByRole('link', { name: 'HarvestCircle', exact: true })
        ).toBeVisible();
        await expect(
          page
            .getByRole('navigation', { name: 'Primary' })
            .getByRole('button', { name: 'Connect extension', exact: true })
        ).toBeEnabled();
        const marker =
          pathname + ':' + (await page.evaluate(() => history.length));
        await page.evaluate(
          (value) => Reflect.set(window, 'hcp127NavigationMarker', value),
          marker
        );
        console.log(
          JSON.stringify({
            fixture: 'HCP127_SAME_DOCUMENT_NAVIGATION',
            phase: 'ready',
            pathname,
            url: page.url(),
            marker
          })
        );
        await page
          .getByRole('link', {
            name: pathname === '/search' ? 'HarvestCircle' : 'Search',
            exact: true
          })
          .click();
        await expect(page).toHaveURL(
          server.url + (pathname === '/search' ? '/' : '/search')
        );
        await expect(
          page.getByRole('textbox', { name: 'What are you looking for?' })
        ).toBeEnabled();
        expect(
          await page.evaluate(
            () => Reflect.get(window, 'hcp127NavigationMarker') as unknown
          )
        ).toBe(marker);
        console.log(
          JSON.stringify({
            fixture: 'HCP127_SAME_DOCUMENT_NAVIGATION',
            phase: 'client-target',
            pathname,
            url: page.url(),
            marker
          })
        );
        await page.goBack();
        await expect(page).toHaveURL(server.url + pathname);
        await expect(page.getByRole('main')).toBeVisible();
        expect(
          await page.evaluate(
            () => Reflect.get(window, 'hcp127NavigationMarker') as unknown
          )
        ).toBe(marker);
        console.log(
          JSON.stringify({
            fixture: 'HCP127_SAME_DOCUMENT_NAVIGATION',
            phase: 'back',
            pathname,
            url: page.url(),
            marker
          })
        );
        expect(new URL(page.url()).pathname).toBe(pathname);
        const announcer = page.locator('#svelte-announcer');
        await expect(announcer).toHaveCount(1);
        const style = await announcer.evaluate(async (element) => {
          const bytes = new TextEncoder().encode(
            element.getAttribute('style') ?? ''
          );
          const hash = await crypto.subtle.digest('SHA-256', bytes);
          const computed = getComputedStyle(element);
          return {
            sha256: [...new Uint8Array(hash)]
              .map((value) => value.toString(16).padStart(2, '0'))
              .join(''),
            position: computed.position,
            width: computed.width,
            height: computed.height,
            clipPath: computed.clipPath
          };
        });
        expect(style).toEqual({
          sha256: prepared.framework_style_attributes[0].style_sha256,
          position: 'absolute',
          width: '1px',
          height: '1px',
          clipPath: 'inset(50%)'
        });
        expect(
          await page.evaluate(
            () => Reflect.get(window, 'hcp127Violations') as string[]
          )
        ).toEqual([]);
      }
    }
    for (const asset of ['/missing.js', '/missing.css', '/missing.png']) {
      const response = await context.request.get(server.url + asset, {
        headers: { accept: 'text/html' }
      });
      expect(response.status()).toBe(404);
      expect(await response.text()).toBe('');
    }
    expect(external).toEqual([]);
    expect(errors).toEqual([]);
  } finally {
    await context.close();
    await server.close();
  }
  expect(context.pages()).toHaveLength(0);
  expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
});

test('real browser CSP denies arbitrary script, eval, hinted egress, resources, base and forms', async ({
  browser
}) => {
  const prepared = await prepareSecurityHeaders(web);
  const server = await createStaticHarness({ headers: prepared.headers });
  const context = await browser.newContext();
  const external: string[] = [];
  try {
    await context.route('**/*', (route) => {
      if (new URL(route.request().url()).origin === server.url)
        return route.continue();
      external.push(route.request().url());
      return route.abort();
    });
    const page = await context.newPage();
    await page.goto(server.url);
    await expect(
      page.getByRole('textbox', { name: 'What are you looking for?' })
    ).toBeEnabled();
    const permissions = await page.evaluate(() => {
      const policy = Reflect.get(document, 'featurePolicy') as {
        allowsFeature(feature: string): boolean;
      };
      return ['camera', 'microphone', 'geolocation', 'payment', 'usb'].map(
        (feature) => ({
          feature,
          allowed: policy.allowsFeature(feature)
        })
      );
    });
    expect(permissions).toEqual(
      ['camera', 'microphone', 'geolocation', 'payment', 'usb'].map(
        (feature) => ({ feature, allowed: false })
      )
    );
    const result = await page.evaluate(async () => {
      const violations: string[] = [];
      Reflect.set(window, 'hcp127Violations', violations);
      document.addEventListener('securitypolicyviolation', (event) =>
        violations.push(event.effectiveDirective)
      );
      const script = document.createElement('script');
      const styled = document.createElement('div');
      styled.setAttribute('style', 'position: fixed');
      document.body.append(styled);
      script.textContent = 'window.hcp127Injected = true';
      document.body.append(script);
      let connectDenied = false;
      try {
        await fetch('https://hinted.invalid/private');
      } catch {
        connectDenied = true;
      }
      const image = document.createElement('img');
      image.src = 'https://hinted.invalid/image.png';
      document.body.append(image);
      const frame = document.createElement('iframe');
      frame.src = 'https://hinted.invalid/embed';
      document.body.append(frame);
      try {
        await new FontFace(
          'HC127Denied',
          'url(https://hinted.invalid/font.woff2)'
        ).load();
      } catch {
        /* Required policy refusal is asserted below. */
      }
      const base = document.createElement('base');
      base.href = 'https://hinted.invalid/';
      document.head.append(base);
      const form = document.createElement('form');
      form.action = 'https://hinted.invalid/submit';
      document.body.append(form);
      form.submit();
      return {
        inline: Reflect.get(window, 'hcp127Injected') as unknown,
        hostileStyle: getComputedStyle(styled).position,
        connectDenied
      };
    });
    expect(result).toEqual({
      inline: undefined,
      hostileStyle: 'static',
      connectDenied: true
    });
    // Runtime.evaluate defaults to bypassing unsafe-eval CSP. Disable that
    // DevTools privilege explicitly; page.evaluate cannot qualify this check.
    // https://chromedevtools.github.io/devtools-protocol/tot/Runtime/#method-evaluate
    const session = await context.newCDPSession(page);
    try {
      const evaluated = await session.send('Runtime.evaluate', {
        expression: "window.eval('window.hcp127Evaluated = true')",
        allowUnsafeEvalBlockedByCSP: false,
        returnByValue: true
      });
      expect(evaluated.exceptionDetails?.exception?.className).toBe(
        'EvalError'
      );
      expect(evaluated.exceptionDetails?.exception?.description).toContain(
        'Content Security Policy'
      );
      expect(
        await page.evaluate(
          () => Reflect.get(window, 'hcp127Evaluated') as unknown
        )
      ).toBeUndefined();
      console.log(
        JSON.stringify({
          fixture: 'HCP127_CSP_EVAL_NO_DEVTOOLS_BYPASS',
          allowUnsafeEvalBlockedByCSP: false,
          exception: evaluated.exceptionDetails?.exception
        })
      );
    } finally {
      await session.detach();
    }
    for (const directive of [
      'script-src-elem',
      'style-src-attr',
      'script-src',
      'connect-src',
      'img-src',
      'font-src',
      'frame-src',
      'base-uri',
      'form-action'
    ])
      await expect
        .poll(() =>
          page.evaluate(
            () => Reflect.get(window, 'hcp127Violations') as string[]
          )
        )
        .toContain(directive);
    expect(page.url()).toBe(server.url + '/');
    expect(external).toEqual([]);
  } finally {
    await context.close();
    await server.close();
  }
  expect(context.pages()).toHaveLength(0);
  expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
});

test('actual frame-ancestors header refuses embedding by another controlled origin', async ({
  browser
}) => {
  const prepared = await prepareSecurityHeaders(web);
  const child = await createStaticHarness({ headers: prepared.headers });
  const parent = await createStaticHarness();
  const context = await browser.newContext();
  const denied: string[] = [];
  try {
    await context.route('**/*', (route) =>
      [parent.url, child.url].includes(new URL(route.request().url()).origin)
        ? route.continue()
        : route.abort()
    );
    const page = await context.newPage();
    page.on('console', (message) => {
      if (message.text().includes('frame-ancestors'))
        denied.push(message.text());
    });
    await page.goto(parent.url);
    await expect(
      page.getByRole('textbox', { name: 'What are you looking for?' })
    ).toBeEnabled();
    await page.evaluate((url) => {
      const frame = document.createElement('iframe');
      frame.src = url;
      document.body.append(frame);
    }, child.url);
    await expect.poll(() => denied.length).toBeGreaterThan(0);
    expect(
      page.frames().filter((frame) => frame.url().startsWith(child.url))
    ).toHaveLength(0);
  } finally {
    await context.close();
    await parent.close();
    await child.close();
  }
  expect(context.pages()).toHaveLength(0);
  expect(parent.state()).toEqual({ listening: false, childProcesses: 0 });
  expect(child.state()).toEqual({ listening: false, childProcesses: 0 });
});
