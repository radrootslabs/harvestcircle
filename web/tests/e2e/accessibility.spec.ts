import { test, expect } from '@playwright/test';
import { encodeProductReference } from '../../src/lib/nostr/references.ts';
import { createStaticHarness } from '../integration/harness/static.ts';

// Actual product routes and keyboard, not a fabricated screen-reader result.
const coordinate = encodeProductReference({
  kind: 30402,
  pubkey: '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
  identifier: 'HCP140_PUBLIC_LONG_REFERENCE_' + 'e\u0301菜'.repeat(32)
});
if (!coordinate) throw Error('supported public coordinate unavailable');
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

for (const width of [320, 1024]) {
  test(`actual eleven guest pages keep keyboard landmarks, labels, contrast and reflow at ${width}px`, async ({
    browser
  }) => {
    const server = await createStaticHarness();
    const context = await browser.newContext({
      viewport: { width, height: 900 },
      reducedMotion: 'reduce'
    });
    const external: string[] = [],
      errors: string[] = [];
    try {
      await context.route('**/*', (route) => {
        if (new URL(route.request().url()).origin === server.url)
          return route.continue();
        external.push(route.request().url());
        return route.abort();
      });
      await context.addInitScript(() => {
        for (const name of ['nostr', 'WebSocket'])
          Object.defineProperty(window, name, {
            configurable: true,
            get() {
              throw Error('guest acquired ' + name);
            }
          });
      });
      for (const path of routes) {
        const page = await context.newPage();
        page.on('pageerror', (error) => errors.push(error.message));
        expect((await page.goto(server.url + path))?.status()).toBe(200);
        await expect(
          page.getByRole('navigation', { name: 'Primary', exact: true })
        ).toHaveCount(1);
        await expect(
          page.getByRole('navigation', { name: 'Footer', exact: true })
        ).toHaveCount(1);
        const main = page.getByRole('main');
        await expect(main).toHaveCount(1);
        await expect(main.getByRole('heading', { level: 1 })).toHaveCount(1);
        if (path === '/') {
          await expect(main.getByRole('heading', { level: 1 })).toHaveText(
            'Find food'
          );
          await expect(
            main.getByText(
              'Browse without connecting. Connect when you want to list food or send a message.',
              { exact: true }
            )
          ).toBeVisible();
        }
        await page.keyboard.press('Tab');
        const skip = page.getByRole('link', {
          name: 'Skip to main content',
          exact: true
        });
        await expect(skip).toBeFocused();
        await expect(skip).toHaveCSS('outline-style', 'solid');
        await page.keyboard.press('Enter');
        await expect(main).toBeFocused();
        const geometry = await page.evaluate(() => {
          function rgb(value: string) {
            const match =
              /^rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?\)$/.exec(value);
            if (!match || (match[4] !== undefined && Number(match[4]) !== 1))
              return undefined;
            return match.slice(1, 4).map(Number);
          }
          function luminance(color: number[]) {
            const linear = color.map((value) => {
              const s = value / 255;
              return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
            });
            return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
          }
          const controls = Array.from(
            document.querySelectorAll(
              'button,input,textarea,select,summary,a.shell-link,a.button'
            )
          )
            .filter((element) => element.checkVisibility())
            .map((element) => {
              const box = element.getBoundingClientRect();
              return {
                tag: element.tagName,
                height: box.height,
                left: box.left,
                right: box.right
              };
            });
          const fields = Array.from(
            document.querySelectorAll('input,textarea,select')
          )
            .filter((element) => element.checkVisibility())
            .map((element) => {
              const field = element as HTMLInputElement;
              return {
                id: field.id,
                labelled: Boolean(
                  field.labels?.length ||
                  field.getAttribute('aria-label') ||
                  field.getAttribute('aria-labelledby')
                )
              };
            });
          const contrast = Array.from(
            document.querySelectorAll(
              'main p,main h1,main h2,main label,main summary,main button,main a'
            )
          )
            .filter(
              (element) =>
                element.checkVisibility() &&
                Boolean(element.textContent?.trim())
            )
            .map((element) => {
              const foreground = rgb(getComputedStyle(element).color);
              let parent: Element | null = element,
                background: number[] | undefined;
              while (parent && !background) {
                background = rgb(getComputedStyle(parent).backgroundColor);
                parent = parent.parentElement;
              }
              if (!foreground || !background)
                throw Error('unmeasured rendered contrast');
              const a = luminance(foreground),
                b = luminance(background);
              return {
                tag: element.tagName,
                ratio: (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05)
              };
            });
          return {
            width: innerWidth,
            scroll: document.documentElement.scrollWidth,
            controls,
            fields,
            contrast
          };
        });
        expect(geometry.scroll).toBeLessThanOrEqual(geometry.width);
        expect(geometry.controls.length).toBeGreaterThan(0);
        for (const control of geometry.controls) {
          expect(control.height).toBeGreaterThanOrEqual(44);
          expect(control.left).toBeGreaterThanOrEqual(0);
          expect(control.right).toBeLessThanOrEqual(width);
        }
        for (const field of geometry.fields) expect(field.labelled).toBe(true);
        expect(geometry.contrast.length).toBeGreaterThan(0);
        for (const color of geometry.contrast)
          expect(color.ratio).toBeGreaterThanOrEqual(4.5);
        console.log(
          JSON.stringify({
            fixture: 'HCP140_ACTUAL_GUEST_ROUTES_ONLY',
            path,
            geometry,
            limits:
              'Actual viewport/keyboard/computed opaque text contrast; not manual screen reader, browser zoom, connected-account or participant qualification.'
          })
        );
        await page.close();
      }
      expect(errors).toEqual([]);
      expect(external).toEqual([]);
    } finally {
      await context.close();
      await server.close();
    }
    expect(context.pages()).toHaveLength(0);
    expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
  });
}
