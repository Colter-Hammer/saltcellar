// Accounts (docs/spec/m1-ledger-import.md#accounts): their types and roles as the UI names them,
// the phone list's groups, and where an account's transactions are. Nothing here talks to the
// server.
import type { RouteLocationRaw } from 'vue-router'
import type { Account } from '@/api/accounts'

export const ACCOUNT_TYPES: readonly { value: Account['type']; label: string }[] = [
  { value: 'checking', label: 'Checking' },
  { value: 'savings', label: 'Savings' },
  { value: 'credit_card', label: 'Credit card' },
  { value: 'investment', label: 'Investment' },
  { value: 'loan', label: 'Loan' },
  { value: 'cash', label: 'Cash' },
  { value: 'other', label: 'Other' },
]

export const ACCOUNT_ROLES: readonly { value: Account['role']; label: string }[] = [
  { value: 'budgeted', label: 'Budgeted' },
  { value: 'tracking', label: 'Tracking' },
]

/** The type's name, like "Credit card". */
export function typeLabel(type: Account['type']): string {
  return ACCOUNT_TYPES.find((kind) => kind.value === type)?.label ?? type
}

/** The role's name, like "Budgeted". */
export function roleLabel(role: Account['role']): string {
  return ACCOUNT_ROLES.find((each) => each.value === role)?.label ?? role
}

export interface AccountGroup {
  name: 'Budgeted' | 'Tracking' | 'Closed'
  accounts: Account[]
}

/**
 * The accounts as the phone lists them: open budgeted ones, open tracking ones, then the closed
 * ones whatever their role, each in the server's order. Empty groups are left out.
 */
export function groupAccounts(accounts: readonly Account[]): AccountGroup[] {
  const groups: AccountGroup[] = [
    { name: 'Budgeted', accounts: accounts.filter((a) => !a.closed && a.role === 'budgeted') },
    { name: 'Tracking', accounts: accounts.filter((a) => !a.closed && a.role === 'tracking') },
    { name: 'Closed', accounts: accounts.filter((a) => a.closed) },
  ]
  return groups.filter((group) => group.accounts.length > 0)
}

/**
 * Transactions filtered to the account, as a query parameter the Transactions screen reads once
 * its filter lands *(agent)*. None for a balances-only account, which has no transactions.
 */
export function transactionsLink(account: Account): RouteLocationRaw | null {
  if (account.balances_only) return null
  return { path: '/transactions', query: { account: String(account.id) } }
}
