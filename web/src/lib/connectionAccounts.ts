// The new accounts prompt's wording and choices (docs/spec/m2-bank-data.md#accounts).
import type { Account } from '@/api/accounts'

/** The prompt's heading: "1 new account found", "3 new accounts found". */
export function foundLabel(count: number): string {
  return `${String(count)} new account${count === 1 ? '' : 's'} found`
}

/**
 * The accounts a feed account can be the same as: the open ones, in order, and the one already
 * suggested even if it's closed *(agent)*.
 */
export function sameAsChoices(accounts: readonly Account[], suggested: number | null): Account[] {
  return accounts.filter((account) => !account.closed || account.id === suggested)
}
