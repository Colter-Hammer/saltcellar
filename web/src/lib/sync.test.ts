import { describe, expect, it } from 'vitest'
import type { SyncRun } from '@/api/bank'
import { runSummary } from './sync'

function run(change: Partial<SyncRun>): SyncRun {
  return {
    trigger: 'manual',
    started_at: '2026-10-28T09:00:00Z',
    finished_at: '2026-10-28T09:00:02Z',
    requests: 1,
    added: 0,
    updated: 0,
    error: null,
    ...change,
  }
}

describe('runSummary', () => {
  it('counts what a run added and updated', () => {
    expect(runSummary(run({ added: 3, updated: 1 }))).toBe('3 rows added, 1 row updated')
    expect(runSummary(run({ added: 1 }))).toBe('1 row added')
    expect(runSummary(run({ updated: 2 }))).toBe('2 rows updated')
    expect(runSummary(run({}))).toBe('No changes')
  })

  it('says a run is going or why it failed', () => {
    expect(runSummary(run({ finished_at: null }))).toBe('Running…')
    expect(
      runSummary(run({ error: 'Sync failed: the SimpleFIN server couldn’t be reached.' })),
    ).toBe('Sync failed: the SimpleFIN server couldn’t be reached.')
  })
})
