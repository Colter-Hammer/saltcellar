// The lint rules in docs/standards.md, proved on snippets at the paths they guard.
import { mkdir, rm, writeFile } from 'node:fs/promises'
import { dirname } from 'node:path'
import { ESLint } from 'eslint'
import { beforeAll, describe, expect, it } from 'vitest'

const eslint = new ESLint()

/**
 * The rules that fire on `code` at `filePath`. Type-aware rules need the file on disk, so it's
 * written there for the run (as `__lint_probe__*`, git-ignored) and removed afterwards.
 */
async function ruleIds(filePath: string, code: string): Promise<string[]> {
  const probe = filePath.replace(/[^/]+$/, (name) => `__lint_probe__${name}`)
  await mkdir(dirname(probe), { recursive: true })
  await writeFile(probe, code)
  try {
    const [result] = await eslint.lintFiles([probe])
    return (result?.messages ?? []).map((message) => message.ruleId ?? message.message)
  } finally {
    await rm(probe)
  }
}

const sfc = (script: string, template = '<div />') =>
  `<script setup lang="ts">\n${script}\n</script>\n\n<template>${template}</template>\n`

// The first lint loads the TypeScript program for type-aware rules, which takes several seconds
// on a cold CI runner; pay it once here so each test below keeps the default timeout.
beforeAll(async () => {
  await ruleIds('src/views/WarmUp.vue', sfc(''))
}, 60_000)

describe('import boundaries', () => {
  it('stops a view calling fetch', async () => {
    const ids = await ruleIds('src/views/SomeView.vue', sfc("void fetch('/api/v1/health')"))
    expect(ids).toContain('no-restricted-globals')
  })

  it('stops a view calling fetch through window', async () => {
    const ids = await ruleIds('src/views/SomeView.vue', sfc("void window.fetch('/api/v1/health')"))
    expect(ids).toContain('no-restricted-properties')
  })

  it('stops a component calling fetch', async () => {
    const ids = await ruleIds('src/components/Some.vue', sfc("void fetch('/api/v1/health')"))
    expect(ids).toContain('no-restricted-globals')
  })

  it('lets the API client call fetch', async () => {
    const ids = await ruleIds(
      'src/api/client.ts',
      "export async function health(): Promise<Response> {\n  return fetch('/api/v1/health')\n}\n",
    )
    expect(ids).toEqual([])
  })

  it('stops a component importing a view', async () => {
    const ids = await ruleIds(
      'src/components/Some.vue',
      sfc("import MoreView from '@/views/MoreView.vue'", '<MoreView />'),
    )
    expect(ids).toContain('no-restricted-imports')
  })

  it('stops a component importing a view by a relative path', async () => {
    const ids = await ruleIds(
      'src/components/Some.vue',
      sfc("import MoreView from '../views/MoreView.vue'", '<MoreView />'),
    )
    expect(ids).toContain('no-restricted-imports')
  })

  it('stops the API client importing components', async () => {
    const ids = await ruleIds(
      'src/api/client.ts',
      "import EmptyScreen from '@/components/EmptyScreen.vue'\nexport const screen = EmptyScreen\n",
    )
    expect(ids).toContain('no-restricted-imports')
  })

  it('lets a view import a component', async () => {
    const ids = await ruleIds(
      'src/views/SomeView.vue',
      sfc("import EmptyScreen from '@/components/EmptyScreen.vue'", '<EmptyScreen />'),
    )
    expect(ids).toEqual([])
  })
})

describe('the API contract', () => {
  it('stops a component importing the generated types', async () => {
    const ids = await ruleIds(
      'src/components/Some.vue',
      sfc(
        "import type { components } from '@/api/generated/schema'\ntype H = components['schemas']['Health']\ndefineProps<{ health: H }>()",
      ),
    )
    expect(ids).toContain('no-restricted-imports')
  })

  it('stops a view importing the generated types by a relative path', async () => {
    const ids = await ruleIds(
      'src/views/SomeView.vue',
      sfc("import type { paths } from '../api/generated/schema'\ndefineProps<{ p: paths }>()"),
    )
    expect(ids).toContain('no-restricted-imports')
  })

  it('stops a view making its own typed client', async () => {
    const ids = await ruleIds(
      'src/views/SomeView.vue',
      sfc("import createClient from 'openapi-fetch'\nvoid createClient"),
    )
    expect(ids).toContain('no-restricted-imports')
  })

  it('lets the API client use the generated types', async () => {
    const ids = await ruleIds(
      'src/api/some.ts',
      "import type { components } from './generated/schema'\nexport type Health = components['schemas']['Health']\n",
    )
    expect(ids).toEqual([])
  })
})

describe('type safety', () => {
  it('rejects any', async () => {
    const ids = await ruleIds('src/lib/some.ts', 'export const x: any = 1\n')
    expect(ids).toContain('@typescript-eslint/no-explicit-any')
  })

  it('rejects the non-null assertion', async () => {
    const ids = await ruleIds(
      'src/lib/some.ts',
      'export function first(xs: number[]): number {\n  return xs[0]!\n}\n',
    )
    expect(ids).toContain('@typescript-eslint/no-non-null-assertion')
  })
})
