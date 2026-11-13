import { describe, expect, it } from 'vitest'
import type { Account } from '@/api/accounts'
import { foundLabel, sameAsChoices } from './connectionAccounts'

function account(id: number, closed = false): Account {
  return {
    id,
    name: `Fake account ${String(id)}`,
    institution: null,
    type: 'checking',
    role: 'budgeted',
    closed,
    in_net_worth: true,
    balances_only: false,
    connection_id: null,
    balance_entry: 'manual',
    sync: null,
    balance_minor: null,
    app_balance_minor: null,
    opening_balance_minor: null,
    opening_date: null,
    rows: 0,
    last_transaction_date: null,
  }
}

describe('foundLabel', () => {
  it('counts one and many', () => {
    expect(foundLabel(1)).toBe('1 new account found')
    expect(foundLabel(3)).toBe('3 new accounts found')
  })
})

describe('sameAsChoices', () => {
  it('offers the open accounts, and the suggested one even if closed', () => {
    const accounts = [account(1), account(2, true), account(3)]
    expect(sameAsChoices(accounts, null).map((a) => a.id)).toEqual([1, 3])
    expect(sameAsChoices(accounts, 2).map((a) => a.id)).toEqual([1, 2, 3])
  })
})
