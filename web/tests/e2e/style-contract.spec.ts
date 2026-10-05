import { expect, test } from '@playwright/test';
import { createStaticHarness } from '../integration/harness/static.ts';

// Trusted presentation fixture only: actual compiled CSS, no mock product route.
const reference = 'TEST_PUBLIC_REFERENCE_' + '0123456789abcdef'.repeat(32);
const fixture = `<main class="page page--reading stack">
  <h1>Presentation fixture</h1>
  <nav class="navbar cluster" aria-label="Fixture navigation"><a class="button button--secondary" href="#form">Fixture controls</a><a class="button button--secondary key" href="#reference">${reference}</a></nav>
  <form id="form" class="stack">
    <div class="field-grid">
      <div class="field"><label class="label" for="query">Fixture query</label><input class="input" id="query" value="${reference}"></div>
      <div class="field"><label class="label" for="selection">Fixture selection</label><select class="input" id="selection"><option>${reference}</option></select></div>
    </div>
    <div class="field"><label class="label" for="text">Fixture text</label><textarea class="input textarea" id="text">${reference}</textarea></div>
    <div class="cluster"><button type="button" class="button button--primary">Fixture primary</button><button type="button" class="button button--secondary key">${reference}</button><button type="button" disabled class="button button--secondary">Fixture disabled</button></div>
  </form>
  <ul class="border-list"><li class="list-row"><p class="key" id="reference">${reference}</p></li></ul>
  <dl class="facts"><dt>Fixture reference</dt><dd class="key">${reference}</dd></dl>
  <div class="notice notice--warning"><p class="description">${reference}\nFixture second line</p></div>
  <article class="message"><p class="description">${reference}</p></article>
  <details class="disclosure"><summary>Fixture details</summary><p class="key">${reference}</p></details>
</main>`;

for (const width of [320, 1024]) {
  test(`compiled compositions reflow with real controls and long references at ${width}px`, async ({
    browser
  }) => {
    const server = await createStaticHarness();
    const context = await browser.newContext({
      viewport: { width, height: 900 }
    });
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
      const sheets = await page
        .locator('link[rel="stylesheet"]')
        .evaluateAll((links) =>
          links.map((link) => (link as HTMLLinkElement).href)
        );
      expect(sheets.length).toBeGreaterThan(0);
      for (const href of sheets) expect(new URL(href).origin).toBe(server.url);
      await page.setContent(
        `<!doctype html><html lang="en"><head><title>Style fixture</title>${sheets.map((href) => `<link rel="stylesheet" href="${href}">`).join('')}</head><body>${fixture}</body></html>`,
        { waitUntil: 'load' }
      );
      await expect(page.locator('.page')).toHaveCSS('box-sizing', 'border-box');
      await expect(page.locator('.key').first()).toHaveCSS(
        'overflow-wrap',
        'anywhere'
      );
      const measure = async () =>
        page.evaluate(() => ({
          width: window.innerWidth,
          scroll: document.documentElement.scrollWidth,
          page: document.querySelector('main')!.getBoundingClientRect().width,
          controls: Array.from(
            document.querySelectorAll(
              'input,textarea,select,button,a.button,summary'
            )
          ).map((element) => {
            const rect = element.getBoundingClientRect();
            return { height: rect.height, left: rect.left, right: rect.right };
          })
        }));
      const assertFit = (result: Awaited<ReturnType<typeof measure>>) => {
        expect(result.scroll).toBeLessThanOrEqual(result.width);
        expect(result.page).toBeLessThanOrEqual(Math.min(width, 672));
        expect(result.controls.length).toBe(9);
        for (const control of result.controls) {
          expect(control.height).toBeGreaterThanOrEqual(44);
          expect(control.left).toBeGreaterThanOrEqual(0);
          expect(control.right).toBeLessThanOrEqual(width);
        }
      };
      const closed = await measure();
      assertFit(closed);
      await page.getByLabel('Fixture query').focus();
      await expect(page.getByLabel('Fixture query')).toBeFocused();
      await expect(page.getByLabel('Fixture query')).toHaveCSS(
        'outline-style',
        'solid'
      );
      await page.keyboard.press('Tab');
      await expect(page.getByLabel('Fixture selection')).toBeFocused();
      await page.getByText('Fixture details', { exact: true }).click();
      await expect(page.locator('details')).toHaveAttribute('open', '');
      const opened = await measure();
      assertFit(opened);
      console.log(
        JSON.stringify({
          fixture: 'HCP012_PRESENTATION_ONLY',
          sheets,
          closed,
          opened
        })
      );
      expect(external).toEqual([]);
    } finally {
      await context.close();
      await server.close();
    }
    expect(context.pages()).toHaveLength(0);
    expect(server.state()).toEqual({ listening: false, childProcesses: 0 });
  });
}
