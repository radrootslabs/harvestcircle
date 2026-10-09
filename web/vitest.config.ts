import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [svelte({ configFile: false })],
  resolve: { conditions: ['node'] },
  test: {
    environment: 'node',
    fileParallelism: false,
    testTimeout: 1_800_000,
    hookTimeout: 1_800_000,
    include: ['tests/unit/**/*.test.ts'],
    setupFiles: ['tests/unit/ssr-effects.ts'],
    passWithNoTests: false
  }
});
