import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/e2e',
  testMatch: '**/*.spec.ts',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 1_800_000,
  expect: { timeout: 300_000 },
  use: { browserName: 'chromium', headless: true, serviceWorkers: 'block' },
  reporter: 'list'
});
