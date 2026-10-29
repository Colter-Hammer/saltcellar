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

type ImportPattern = Record<'group' | 'message', string | string[]>

/** Import boundaries: views → components → API client. Nothing imports upwards. */
function upwards(...layers: string[]): ImportPattern[] {
  return layers.map((layer) => ({
    group: [`@/${layer}/**`, `**/${layer}/**`],
    message: `Imports go views → components → API client; this file can't import from src/${layer}/.`,
  }))
}

/** The generated types and the typed client stay inside src/api/, which exports what the UI needs. */
const apiInternals: ImportPattern[] = [
  {
    group: ['openapi-fetch', '@/api/generated/**', '**/api/generated/**'],
    message:
      'Only src/api/ uses the generated API types and client; import from its modules instead.',
  },
]

// One rule entry per file group: a later `no-restricted-imports` replaces an earlier one, never merges.
function restrictImports(...patterns: ImportPattern[][]): Linter.RulesRecord {
  return { 'no-restricted-imports': ['error', { patterns: patterns.flat() }] }
}

export default defineConfig(
  // src/api/generated/ is written by scripts/gen-api.sh.
  { ignores: ['dist/', 'coverage/', 'test-results/', 'playwright-report/', 'src/api/generated/'] },
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
  { files: ['src/**/*.{ts,vue}'], rules: restrictImports(apiInternals) },
  { files: ['src/components/**'], rules: restrictImports(apiInternals, upwards('views')) },
  { files: ['src/lib/**'], rules: restrictImports(apiInternals, upwards('views', 'components')) },
  { files: ['src/api/**'], rules: restrictImports(upwards('views', 'components')) },
  prettier,
)
