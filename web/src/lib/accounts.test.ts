import { describe, expect, it } from 'vitest'
import type { Account } from '@/api/accounts'
import { groupAccounts, roleLabel, transactionsLink, typeLabel } from './accounts'

function account(id: number, fields: Partial<Account> = {}): Account {
  return {
    id,
    name: `Account ${String(id)}`,
    institution: null,
    type: 'checking',
    role: 'budgeted',
    closed: false,
    balances_only: false,
    balance_minor: 0,
    app_balance_minor: 0,
    opening_balance_minor: 0,
    opening_date: '2026-01-01',
    rows: 0,
    last_transaction_date: null,
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
