import { describe, expect, it } from 'vitest'
import type { Connection } from '@/api/bank'
import { account, sync } from './syncFixtures'
import { quietStates, spot, syncProblems } from './syncHealth'

const connection = (paused = false): Connection => ({
  label: 'SimpleFIN Bridge',
  status: paused ? 'token_revoked' : 'ok',
  paused,
  last_sync: null,
  banks: [],
})
const en = { locale: 'en-US' }

describe('syncProblems', () => {
  it('counts reconnecting, attention and drift; stale is quiet', () => {
    const flag = { drift_minor: -950, since: '2026-11-12', suspect_rows: [] }
    const accounts = [
      account(1, { sync: sync('needs_reconnecting', true) }),
      account(2, { sync: sync('stale') }),
      account(3, { sync: sync('ok'), drift: { ...account(3).drift, flag } }),
      account(4, { sync: sync('needs_attention'), closed: true }),
    ]
    const problems = syncProblems(accounts)
    expect(problems.map((p) => [p.accountId, p.fix.kind])).toEqual([
      [1, 'simplefin'],
      [3, 'suspect-rows'],
    ])
    expect(quietStates(accounts).map((q) => q.accountId)).toEqual([2])
  })

  it('sends a problem with no bank fix to its account', () => {
    const [problem] = syncProblems([account(7, { sync: sync('needs_attention') })])
    expect(problem?.fix).toEqual({ kind: 'account', to: '/accounts/7' })
  })
})

describe('spot', () => {
  const at = '2026-11-17T06:13:00Z'
  it('is absent without a connection', () => {
    expect(spot(null, 0, at, 'UTC', en)).toBeNull()
  })
  it('all good: a hollow dot and the time', () => {
    expect(spot(connection(), 0, at, 'UTC', en)).toEqual({
      filled: false,
      short: '6:13 AM',
      long: 'Synced 6:13 AM',
    })
  })
  it('counts problems', () => {
    expect(spot(connection(), 1, at, 'UTC', en)?.long).toBe('1 sync problem')
    expect(spot(connection(), 2, at, 'UTC', en)).toMatchObject({
      filled: true,
      short: '2 sync problems',
    })
  })
  it('paused wins over problems', () => {
    expect(spot(connection(true), 2, at, 'UTC', en)).toMatchObject({
      filled: true,
      long: 'Sync paused',
    })
  })
})
