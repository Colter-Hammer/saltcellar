import { describe, expect, it } from 'vitest'
import type { Account } from '@/api/accounts'
import {
  describeAccount,
  dropAccount,
  groupAccounts,
  needsAttention,
  roleLabel,
  stepAccount,
  transactionsLink,
  typeLabel,
} from './accounts'

function account(id: number, fields: Partial<Account> = {}): Account {
  return {
    id,
    name: `Account ${String(id)}`,
    institution: null,
    type: 'checking',
    role: 'budgeted',
    closed: false,
    in_net_worth: true,
    balances_only: false,
    connection_id: null,
    balance_entry: 'manual',
    sync: null,
    balance_minor: 0,
    app_balance_minor: 0,
    opening_balance_minor: 0,
    opening_date: '2026-01-01',
    rows: 0,
    last_transaction_date: null,
    drift: { sign_corrected: false, bank_comparison: null, flag: null, adjustments: [] },
    ...fields,
  }
}

describe('groupAccounts', () => {
  it('puts open budgeted, then open tracking, then closed ones whatever their role', () => {
    const accounts = [
      account(1, { closed: true }),
      account(2, { role: 'tracking' }),
      account(3),
      account(4, { role: 'tracking', closed: true }),
      account(5),
    ]
    expect(
      groupAccounts(accounts).map((group) => [group.name, group.accounts.map((a) => a.id)]),
    ).toEqual([
      ['Budgeted', [3, 5]],
      ['Tracking', [2]],
      ['Closed', [1, 4]],
    ])
  })

  it('leaves out empty groups', () => {
    expect(groupAccounts([account(1, { role: 'tracking' })]).map((g) => g.name)).toEqual([
      'Tracking',
    ])
    expect(groupAccounts([])).toEqual([])
  })
})

describe('labels', () => {
  it('names each type and role', () => {
    expect(typeLabel('credit_card')).toBe('Credit card')
    expect(typeLabel('other')).toBe('Other')
    expect(roleLabel('budgeted')).toBe('Budgeted')
    expect(roleLabel('tracking')).toBe('Tracking')
  })
})

describe('transactionsLink', () => {
  it('filters Transactions to the account', () => {
    expect(transactionsLink(account(7))).toEqual({
      path: '/transactions',
      query: { account: '7' },
    })
  })

  it('is none for a balances-only account', () => {
    expect(transactionsLink(account(7, { balances_only: true }))).toBeNull()
  })
})

describe('ordering accounts', () => {
  const ids = (accounts: Account[]) => accounts.map((a) => a.id)
  const all = [
    account(1),
    account(2, { role: 'tracking' }),
    account(3),
    account(4, { closed: true }),
    account(5),
  ]
  it('drops an account where another is, in its own group only', () => {
    // Dragged up it lands before the one it's dropped on, dragged down after it.
    expect(ids(dropAccount(all, 3, 1) ?? [])).toEqual([3, 2, 1, 4, 5])
    expect(ids(dropAccount(all, 1, 5) ?? [])).toEqual([3, 2, 5, 4, 1])
    expect(ids(dropAccount(all, 5, 1) ?? [])).toEqual([5, 2, 1, 4, 3])
    // Tracking and closed accounts keep their slots.
    expect(dropAccount(all, 1, 2)).toBeNull()
    expect(dropAccount(all, 1, 4)).toBeNull()
  })

  it('drops nothing on itself or on nothing', () => {
    expect(dropAccount(all, 1, 1)).toBeNull()
    expect(dropAccount(all, 1, 99)).toBeNull()
    expect(dropAccount(all, 99, 1)).toBeNull()
  })

  it('steps one place up or down in its group, and not past the ends', () => {
    expect(ids(stepAccount(all, 3, -1) ?? [])).toEqual([3, 2, 1, 4, 5])
    expect(ids(stepAccount(all, 1, 1) ?? [])).toEqual([3, 2, 1, 4, 5])
    expect(stepAccount(all, 1, -1)).toBeNull()
    expect(stepAccount(all, 5, 1)).toBeNull()
    expect(stepAccount(all, 2, 1)).toBeNull()
  })

  it('steps closed accounts too', () => {
    const closed = [account(1, { closed: true }), account(2, { closed: true })]
    expect(ids(stepAccount(closed, 2, -1) ?? [])).toEqual([2, 1])
  })

  it('says where an account stands in its group', () => {
    expect(describeAccount(all, 3)).toBe('Account 3, 2 of 3 in Budgeted')
    expect(describeAccount(all, 4)).toBe('Account 4, 1 of 1 in Closed')
  })
})

describe('needsAttention', () => {
  const sync = (state: NonNullable<Account['sync']>['state']): Account['sync'] => ({
    state,
    message: null,
    days: null,
    last_refreshed_at: null,
    fix: false,
  })

  it('is true for an account that needs reconnecting or attention, and not for the quiet ones', () => {
    expect(needsAttention(account(1, { sync: sync('needs_reconnecting') }))).toBe(true)
    expect(needsAttention(account(1, { sync: sync('needs_attention') }))).toBe(true)
    expect(needsAttention(account(1, { sync: sync('stale') }))).toBe(false)
    expect(needsAttention(account(1, { sync: sync('ok') }))).toBe(false)
    expect(needsAttention(account(1, { sync: null }))).toBe(false)
  })
})
