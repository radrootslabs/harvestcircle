import { loadConfig } from '@sveltejs/load-config';
import js from '@eslint/js';
import ts from 'typescript-eslint';
import svelte from 'eslint-plugin-svelte';
import { defineConfig } from 'eslint/config';

const svelteConfig = (await loadConfig('./', { traverse: false }))?.config;
if (!svelteConfig) throw new Error('Missing Svelte configuration');

export default defineConfig(
  {
    ignores: [
      'node_modules/**',
      '.svelte-kit/**',
      'build/**',
      'test-results/**',
      'playwright-report/**'
    ]
  },
  js.configs.recommended,
  ts.configs.recommended,
  {
    files: ['**/*.ts', '**/*.svelte'],
    extends: [ts.configs.recommendedTypeChecked],
    languageOptions: {
      parserOptions: { projectService: true, extraFileExtensions: ['.svelte'] }
    }
  },
  svelte.configs.recommended,
  svelte.configs.prettier,
  {
    files: ['src/lib/components/CapabilityGate.svelte'],
    languageOptions: { globals: { document: 'readonly' } }
  },
  {
    files: ['**/*.svelte'],
    languageOptions: { parserOptions: { parser: ts.parser, svelteConfig } },
    rules: { 'svelte/no-inline-styles': 'error' }
  },
  {
    files: ['**/*.js', '**/*.mjs'],
    languageOptions: {
      globals: {
        console: 'readonly',
        process: 'readonly',
        URL: 'readonly',
        Buffer: 'readonly',
        TextDecoder: 'readonly'
      }
    }
  }
);
