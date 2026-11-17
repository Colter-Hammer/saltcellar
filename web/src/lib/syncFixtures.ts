// Fake accounts for the sync health tests.
import type { Account } from '@/api/accounts'

type Sync = NonNullable<Account['sync']>

export function account(id: number, extra: Partial<Account> = {}): Account {
  return {
    id,
    name: `Demo ${String(id)}`,
    institution: 'First Demo Bank',
    type: 'checking',
    role: 'budgeted',
    closed: false,
    in_net_worth: true,
    balances_only: false,
    connection_id: 1,
    balance_entry: 'bank',
    sync: null,
    balance_minor: 0,
    app_balance_minor: 0,
    opening_balance_minor: 0,
    opening_date: '2026-10-27',
    rows: 0,
    last_transaction_date: null,
    drift: { sign_corrected: false, bank_comparison: null, flag: null, adjustments: [] },
    ...extra,
  }
}

export const sync = (state: Sync['state'], fix = false): Sync => ({
  state,
  message: `message ${state}`,
  days: null,
  last_refreshed_at: null,
  fix,
})
