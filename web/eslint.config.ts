// The frontend lint rules (docs/standards.md#frontend). `npm run lint` allows zero warnings.
import js from '@eslint/js'
import prettier from 'eslint-config-prettier'
import vue from 'eslint-plugin-vue'
import type { Linter } from 'eslint'
import { defineConfig } from 'eslint/config'
import tseslint from 'typescript-eslint'

const fetchOnlyInTheApiClient: Linter.RulesRecord = {
  'no-restricted-globals': [
    'error',
    { name: 'fetch', message: 'Only the API client (src/api/) calls fetch.' },
  ],
  'no-restricted-properties': [
    'error',
    ...['window', 'globalThis', 'self'].map((object) => ({
      object,
      property: 'fetch',
      message: 'Only the API client (src/api/) calls fetch.',
    })),
  ],
}

/** Import boundaries: views → components → API client. Nothing imports upwards. */
function noImportsFrom(...layers: string[]): Linter.RulesRecord {
  return {
    'no-restricted-imports': [
      'error',
      {
        patterns: layers.map((layer) => ({
          group: [`@/${layer}/**`, `**/${layer}/**`],
          message: `Imports go views → components → API client; this file can't import from src/${layer}/.`,
        })),
      },
    ],
  }
}

export default defineConfig(
  { ignores: ['dist/', 'coverage/'] },
  js.configs.recommended,
  tseslint.configs.strictTypeChecked,
  tseslint.configs.stylisticTypeChecked,
  vue.configs['flat/recommended'],
  {
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
        extraFileExtensions: ['.vue'],
        parser: tseslint.parser,
      },
    },
  },
  {
    files: ['src/**/*.{ts,vue}'],
    rules: fetchOnlyInTheApiClient,
  },
  {
    files: ['src/api/**'],
    rules: { 'no-restricted-globals': 'off', 'no-restricted-properties': 'off' },
  },
  { files: ['src/components/**'], rules: noImportsFrom('views') },
  { files: ['src/api/**', 'src/lib/**'], rules: noImportsFrom('views', 'components') },
  prettier,
)
