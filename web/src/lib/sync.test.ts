import { describe, expect, it } from 'vitest'
import type { Bank, SyncRun, SyncState } from '@/api/bank'
import {
  accountCount,
  accountProblem,
  bankState,
  pausedBanner,
  runSummary,
  scheduleLine,
  syncNowWait,
  triggerName,
} from './sync'

function run(change: Partial<SyncRun>): SyncRun {
  return {
    id: 1,
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

function state(change: Partial<SyncState>): SyncState {
  return {
    time: '06:13',
    keep_raw_responses: false,
    request_cap: 12,
    requests_left: 9,
    last_synced_at: null,
    cooldown_until: null,
    limit_until: null,
    ...change,
  }
}

const en = { locale: 'en-US' }

describe('scheduleLine', () => {
  it('says the time loosely and the requests left', () => {
    expect(scheduleLine(state({}), en)).toBe(
      'Syncs daily around 6 AM · 9 of 12 requests left today',
    )
    expect(scheduleLine(state({ time: '15:41', requests_left: 1 }), en)).toBe(
      'Syncs daily around 4 PM · 1 of 12 requests left today',
    )
  })
})

describe('syncNowWait', () => {
  const now = new Date('2026-10-28T09:12:30Z')

  it('is null when Sync now can go', () => {
    expect(syncNowWait(state({}), now, 'Europe/Berlin', en)).toBeNull()
    // A cooldown that has ended doesn't count.
    expect(
      syncNowWait(
        state({ last_synced_at: '2026-10-28T08:00:00Z', cooldown_until: '2026-10-28T08:30:00Z' }),
        now,
        'Europe/Berlin',
        en,
      ),
    ).toBeNull()
  })

  it('says how long ago the last sync was while cooling down', () => {
    const cooling = state({
      last_synced_at: '2026-10-28T09:00:00Z',
      cooldown_until: '2026-10-28T09:30:00Z',
    })
    expect(syncNowWait(cooling, now, 'Europe/Berlin', en)).toBe('Synced 12 min ago')
    expect(syncNowWait(cooling, new Date('2026-10-28T09:00:20Z'), 'UTC', en)).toBe(
      'Synced just now',
    )
  })

  it('says when the limit lifts, in the household timezone, before the cooldown', () => {
    const capped = state({
      last_synced_at: '2026-10-28T09:00:00Z',
      cooldown_until: '2026-10-28T09:30:00Z',
      limit_until: '2026-10-28T14:40:00Z',
    })
    expect(syncNowWait(capped, now, 'Europe/Berlin', en)).toBe(
      'Sync limit reached, try again after 3:40 PM',
    )
  })
})

describe('triggerName', () => {
  it('names each trigger', () => {
    expect(
      ['scheduled', 'manual', 'retry', 'catch_up', 'backfill', 'other'].map(triggerName),
    ).toEqual(['Daily sync', 'Sync now', 'Retry', 'Catch-up', 'Back-fill', 'other'])
  })
})

describe('accountProblem', () => {
  it('names the account once', () => {
    const card = { name: 'Demo Rewards Card', error_code: 'act.missingdata' }
    expect(
      accountProblem({ ...card, error: 'Demo Rewards Card: no transactions were available.' }),
    ).toBe('Demo Rewards Card: no transactions were available.')
    expect(accountProblem({ ...card, error: "The bank didn't send this account this time." })).toBe(
      "Demo Rewards Card: The bank didn't send this account this time.",
    )
    expect(accountProblem({ ...card, error: null })).toBe('Demo Rewards Card: act.missingdata')
    expect(accountProblem({ ...card, error: null, error_code: null })).toBe(
      'Demo Rewards Card: failed',
    )
  })
})

describe('pausedBanner', () => {
  it('says what stopped, and where the way out is', () => {
    const say = (status: string) => {
      const { lead, link, tail } = pausedBanner(status)
      return `${lead}${link}${tail}`
    }
    expect(say('token_revoked')).toBe(
      'Bank sync stopped. Get a new setup token from SimpleFIN and paste it in Settings.',
    )
    expect(say('unpaid')).toContain('SimpleFIN subscription unpaid')
    expect(say('key_mismatch')).toContain('The secret key changed')
    expect(say('anything else')).toContain('Get a new setup token')
  })
})

describe('bankState', () => {
  it('names each state', () => {
    const state = (s: Bank['state']) => bankState({ state: s } as Bank)
    expect(
      (['ok', 'balances_only', 'stale', 'needs_reconnecting', 'needs_attention'] as const).map(
        state,
      ),
    ).toEqual(['OK', 'Balances only', 'Stale', 'Needs reconnecting', 'Needs attention'])
  })

  it('counts accounts', () => {
    expect([accountCount(1), accountCount(3)]).toEqual(['1 account', '3 accounts'])
  })
})
