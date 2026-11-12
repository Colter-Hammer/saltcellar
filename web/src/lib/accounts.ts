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

// Ordering accounts: they move within their group (Budgeted, Tracking or Closed) and the groups'
// accounts keep the slots they held among all of them, so another group's order never changes
// *(agent)*.

/** The accounts of the group that holds `id`, in order. */
function groupOf(accounts: readonly Account[], id: number): Account[] | undefined {
  return groupAccounts(accounts).find((group) => group.accounts.some((a) => a.id === id))?.accounts
}

/** `accounts` with `moved` (a rearrangement of `within`) written back into `within`'s slots. */
function refill(
  accounts: readonly Account[],
  within: readonly Account[],
  moved: readonly Account[],
) {
  const ids = new Set(within.map((account) => account.id))
  const queue = [...moved]
  return accounts.map((account) => (ids.has(account.id) ? (queue.shift() ?? account) : account))
}

/**
 * All `accounts`, with `id` dropped where the account `onto` is in the same group: it takes
 * `onto`'s place, so dropped downward it lands after it. Null if they're not in one group.
 */
export function dropAccount(accounts: readonly Account[], id: number, onto: number) {
  const within = groupOf(accounts, id)
  if (within === undefined || id === onto) return null
  const index = within.findIndex((account) => account.id === onto)
  const moving = within.find((account) => account.id === id)
  if (index < 0 || moving === undefined) return null
  const moved = within.filter((account) => account.id !== id)
  moved.splice(index, 0, moving)
  return refill(accounts, within, moved)
}

/** All `accounts`, with `id` moved one place up (-1) or down (1) in its group; null at the end. */
export function stepAccount(accounts: readonly Account[], id: number, by: -1 | 1) {
  const within = groupOf(accounts, id)
  const at = within?.findIndex((account) => account.id === id) ?? -1
  const other = within?.[at + by]
  return within === undefined || other === undefined ? null : dropAccount(accounts, id, other.id)
}

/** `id`'s place in its group, for a screen reader: "Rewards card, 2 of 3 in Budgeted". */
export function describeAccount(accounts: readonly Account[], id: number): string {
  const group = groupAccounts(accounts).find((each) => each.accounts.some((a) => a.id === id))
  const at = group?.accounts.findIndex((account) => account.id === id) ?? -1
  const name = group?.accounts[at]?.name ?? ''
  return `${name}, ${String(at + 1)} of ${String(group?.accounts.length ?? 0)} in ${group?.name ?? ''}`
}

/**
 * Whether the account's bank data needs attention (docs/spec/m2-bank-data.md#sync-health-in-the-ui):
 * the accounts list marks it with a ⚠. Stale data is quiet, so it isn't one.
 */
export function needsAttention(account: Account): boolean {
  const state = account.sync?.state
  return state === 'needs_attention' || state === 'needs_reconnecting'
}
